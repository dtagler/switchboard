//! BLE connection monitoring for the Nuphy Air75 V3 keyboard.
//!
//! This module implements the BLE detection strategy described in ARCHITECTURE.md.
//! It wraps `BluetoothLEDevice::FromBluetoothAddressAsync` against a runtime-configured BD_ADDR
//! (loaded from the `SWITCHBOARD_NUPHY_BD_ADDR` env var, or a `.env` file next to the exe / in
//! the current dir) and subscribes to `ConnectionStatusChanged` events, posting `WM_APP+1`
//! to the message loop on every state transition.
//!
//! ## Why runtime config (not a hardcoded constant)?
//!
//! A Bluetooth MAC is owner-specific PII (it persists per device, can be used to track
//! the keyboard across networks, and uniquely identifies the owner's hardware). To keep
//! the source tree commit-safe, the BD_ADDR is supplied at runtime and never appears in
//! version-controlled files. See `.env.example` for the expected format.
//!
//! ## Key Design Decisions
//!
//! - **No polling, no timers, no threads** — pure event-driven. The sanity timer lives in main.rs.
//! - **Event-token lifecycle** — Token stored in `BleHandle`; Drop unsubscribes (RAII).
//! - **NotConfigured handling** — If `SWITCHBOARD_NUPHY_BD_ADDR` is unset/unparseable, return
//!   `BleError::NotConfigured`. Caller logs loudly and treats `is_connected()` as always-false
//!   for the entire session (same fail-open posture as `NotPaired`).
//! - **NotPaired handling** — If `FromBluetoothAddressAsync` returns null device (never paired),
//!   return `BleError::NotPaired`. Same fail-open behavior.
//! - **`is_connected` fresh read** — Never cached Each call queries `ConnectionStatus`
//!   afresh. On error, logs at warn and returns `false` (fail-safe: treat as not connected → ENABLE).
//! - **HWND passing is sound** — HWND is Copy. PostMessage is thread-safe. Message loop is always
//!   alive while BleHandle lives.
//!
//! ## Proven Pattern
//!
//! Validated in Spike 1.6 v4 under requireAdministrator: 11 clean transitions observed during
//! Nuphy power-cycle (OFF ~5s, ON ~10s, OFF ~5s). Event fires reliably on Connected⇄Disconnected.

use log::{error, info, warn};
use std::path::PathBuf;
use windows::core::*;
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice};
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Env var name carrying the target Bluetooth MAC address.
///
/// Accepted formats (case-insensitive):
///   - Hex with prefix:   `0xAABBCCDDEEFF`
///   - Bare hex (12 hex digits): `AABBCCDDEEFF`
///   - Colon-separated MAC:      `AA:BB:CC:DD:EE:FF`
///   - Hyphen-separated MAC:     `AA-BB-CC-DD-EE-FF`
pub const BD_ADDR_ENV: &str = "SWITCHBOARD_NUPHY_BD_ADDR";

/// Window message for BLE connection state changes (WM_APP+1).
///
/// Fired by the `ConnectionStatusChanged` event handler via PostMessage. The message loop
/// in main.rs receives this and calls `apply_policy()`.
pub const WM_BLE_CONNECTION_CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

/// BLE subsystem errors.
#[derive(Debug)]
pub enum BleError {
    /// `SWITCHBOARD_NUPHY_BD_ADDR` is not set (and no `.env` file supplied it), or its
    /// value couldn't be parsed as a Bluetooth MAC. Caller should log loudly and treat
    /// `is_connected()` as always-false for the session.
    NotConfigured,

    /// The Nuphy has never been paired on this machine. `FromBluetoothAddressAsync` returned null.
    /// Caller should log loudly and treat `is_connected()` as always-false for the session.
    NotPaired,

    /// WinRT call failed. Includes the API name and HRESULT for diagnostics.
    WinRTFailure { api: &'static str, hr: i32 },
}

impl std::fmt::Display for BleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BleError::NotConfigured => write!(
                f,
                "Nuphy BD_ADDR not configured (set {} env var or add it to .env)",
                BD_ADDR_ENV
            ),
            BleError::NotPaired => write!(f, "Nuphy not paired on this machine"),
            BleError::WinRTFailure { api, hr } => write!(f, "{} failed (HRESULT {:#x})", api, hr),
        }
    }
}

impl std::error::Error for BleError {}

impl From<Error> for BleError {
    fn from(e: Error) -> Self {
        BleError::WinRTFailure {
            api: "WinRT call",
            hr: e.code().0,
        }
    }
}

/// RAII handle for the BLE subscription. Drop unsubscribes the event.
pub struct BleHandle {
    device: BluetoothLEDevice,
    token: windows::Foundation::EventRegistrationToken,
}

impl BleHandle {
    /// Check if the Nuphy is currently connected.
    ///
    /// **Fresh read, never cached ** Each call queries `ConnectionStatus` afresh.
    /// Returns `true` if `ConnectionStatus == Connected`, `false` otherwise (including on error).
    ///
    /// On error, logs at warn and returns `false` (fail-safe: treat as not connected → ENABLE).
    pub fn is_connected(&self) -> bool {
        match self.device.ConnectionStatus() {
            Ok(BluetoothConnectionStatus::Connected) => true,
            Ok(_) => false,
            Err(e) => {
                warn!(
                    "BLE: ConnectionStatus() failed (HRESULT {:#x}), treating as disconnected",
                    e.code().0
                );
                false
            }
        }
    }
}

impl Drop for BleHandle {
    fn drop(&mut self) {
        if let Err(e) = self.device.RemoveConnectionStatusChanged(self.token) {
            error!(
                "BLE: failed to unsubscribe ConnectionStatusChanged (HRESULT {:#x})",
                e.code().0
            );
        } else {
            info!("BLE: unsubscribed ConnectionStatusChanged");
        }
        // Balance the CoInitializeEx in start() now that the device and event
        // subscription are being torn down.
        unsafe { CoUninitialize() };
    }
}

/// Initialize BLE monitoring for the configured Nuphy BD_ADDR.
///
/// Returns a `BleHandle` that stays subscribed to `ConnectionStatusChanged` until dropped.
/// On every connection state transition, posts `WM_BLE_CONNECTION_CHANGED` to `hwnd`.
///
/// ## Errors
///
/// - `BleError::NotConfigured` if `SWITCHBOARD_NUPHY_BD_ADDR` is unset or unparseable.
///   Caller should log loudly and treat `is_connected()` as always-false for the session.
/// - `BleError::NotPaired` if `FromBluetoothAddressAsync` returns null (device never paired).
///   Same fail-open posture as `NotConfigured`.
/// - `BleError::WinRTFailure` on any other WinRT call failure (COM init, async wait, subscribe).
///
/// ## Threading
///
/// This function initializes COM (`CoInitializeEx`) on the calling thread with `COINIT_APARTMENTTHREADED`.
/// If `CoInitializeEx` itself fails there is nothing to balance, so the function returns immediately.
/// If init succeeds but a later step (resolve, subscribe, NotPaired check) fails, the function
/// calls `CoUninitialize` before returning the error. On success COM stays initialized for the
/// lifetime of the returned `BleHandle` (the handle's Drop uninitializes it), because the device
/// and event subscription require COM on this thread to remain live.
pub fn start(hwnd: HWND) -> std::result::Result<BleHandle, BleError> {
    // Resolve BD_ADDR from env / .env file BEFORE touching COM, so a missing config
    // doesn't leak an Initialize without a balancing Uninitialize.
    let bd_addr = match load_bd_addr() {
        Some(addr) => addr,
        None => return Err(BleError::NotConfigured),
    };

    // Initialize COM for WinRT (STA). Paired with CoUninitialize on every error-return
    // path below, and with BleHandle::drop on success.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() {
            return Err(BleError::WinRTFailure {
                api: "CoInitializeEx",
                hr: hr.0,
            });
        }
    }

    info!("BLE: resolving BluetoothLEDevice for configured BD_ADDR...");

    // Resolve the device via FromBluetoothAddressAsync (blocking wait)
    let op = match BluetoothLEDevice::FromBluetoothAddressAsync(bd_addr) {
        Ok(op) => op,
        Err(e) => {
            unsafe { CoUninitialize() };
            return Err(e.into());
        }
    };
    let device = match op.get() {
        Ok(d) => d,
        Err(e) => {
            unsafe { CoUninitialize() };
            return Err(e.into());
        }
    };

    // Check if device is valid by checking BluetoothAddress (null device returns 0).
    // This is a best-effort heuristic: `FromBluetoothAddressAsync` resolving to a
    // device whose BluetoothAddress is 0 is the observed signature of "never paired".
    if device.BluetoothAddress().unwrap_or(0) == 0 {
        error!("BLE: FromBluetoothAddressAsync returned null device (never paired)");
        unsafe { CoUninitialize() };
        return Err(BleError::NotPaired);
    }

    // Log initial state
    let name = device
        .Name()
        .ok()
        .map(|h| h.to_string_lossy())
        .unwrap_or_else(|| "(no name)".to_string());
    let initial_status = device
        .ConnectionStatus()
        .ok()
        .unwrap_or(BluetoothConnectionStatus::Disconnected);
    info!(
        "BLE: device resolved: name={:?}, initial_status={:?}",
        name,
        status_str(initial_status)
    );

    // Subscribe to ConnectionStatusChanged
    // HWND is *mut c_void and not Send, so we convert to isize for safe capture
    let hwnd_raw = hwnd.0 as isize;
    let token = match device.ConnectionStatusChanged(&TypedEventHandler::new(
        move |dev: &Option<BluetoothLEDevice>, _args: &Option<IInspectable>| {
            if let Some(d) = dev.as_ref() {
                let status = d
                    .ConnectionStatus()
                    .unwrap_or(BluetoothConnectionStatus::Disconnected);
                info!("BLE: ConnectionStatusChanged => {}", status_str(status));

                // Post WM_BLE_CONNECTION_CHANGED to message loop
                // Reconstruct HWND from the captured isize
                let hwnd = HWND(hwnd_raw as *mut _);
                unsafe {
                    let _ = PostMessageW(hwnd, WM_BLE_CONNECTION_CHANGED, None, None);
                }
            } else {
                warn!("BLE: ConnectionStatusChanged fired with null sender");
            }
            Ok(())
        },
    )) {
        Ok(t) => t,
        Err(e) => {
            unsafe { CoUninitialize() };
            return Err(e.into());
        }
    };

    info!("BLE: subscribed to ConnectionStatusChanged");

    Ok(BleHandle { device, token })
}

/// Helper to stringify BluetoothConnectionStatus for logging (Spike 1.6 pattern).
fn status_str(s: BluetoothConnectionStatus) -> &'static str {
    match s {
        BluetoothConnectionStatus::Connected => "Connected",
        BluetoothConnectionStatus::Disconnected => "Disconnected",
        _ => "Unknown",
    }
}

/// Resolve the target BD_ADDR from environment, falling back to a `.env` file
/// next to the exe and then to the current working directory.
///
/// Returns `None` if no source supplies a parseable value. This is the caller's
/// signal to surface `BleError::NotConfigured`.
fn load_bd_addr() -> Option<u64> {
    // 1) Process env (highest precedence)
    if let Ok(val) = std::env::var(BD_ADDR_ENV) {
        if let Some(addr) = parse_bd_addr(&val) {
            return Some(addr);
        }
        warn!(
            "BLE: {} is set but value '{}' could not be parsed as a Bluetooth MAC",
            BD_ADDR_ENV, val
        );
    }

    // 2) .env file next to the exe (typical install layout)
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        if let Some(addr) = read_env_file_for_addr(&exe_dir.join(".env")) {
            return Some(addr);
        }
    }

    // 3) .env in current working directory (dev convenience)
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(addr) = read_env_file_for_addr(&cwd.join(".env")) {
            return Some(addr);
        }
    }

    None
}

/// Read `BD_ADDR_ENV` from a single `.env`-style file. Tolerant of comments
/// (`#`), blank lines, optional `export ` prefix, and surrounding quotes.
fn read_env_file_for_addr(path: &std::path::Path) -> Option<u64> {
    let contents = std::fs::read_to_string(path).ok()?;
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line.split_once('=')?;
        if key.trim() != BD_ADDR_ENV {
            continue;
        }
        // Strip surrounding single or double quotes if present.
        let mut v = value.trim();
        if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
            || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2)
        {
            v = &v[1..v.len() - 1];
        }
        if let Some(addr) = parse_bd_addr(v) {
            return Some(addr);
        }
    }
    None
}

/// Parse a Bluetooth MAC into a `u64`. Accepts any of:
///   `0xAABBCCDDEEFF`, `AABBCCDDEEFF`, `AA:BB:CC:DD:EE:FF`, `AA-BB-CC-DD-EE-FF`.
/// Case-insensitive. Returns `None` for any malformed input, the all-zero
/// wildcard MAC, or a result outside the 48-bit BD_ADDR range.
fn parse_bd_addr(s: &str) -> Option<u64> {
    let trimmed = s.trim();
    let stripped = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    let cleaned: String = stripped
        .chars()
        .filter(|c| *c != ':' && *c != '-')
        .collect();
    if cleaned.len() != 12 || !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(&cleaned, 16).ok().filter(|v| *v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ble_error_display_not_paired() {
        let s = format!("{}", BleError::NotPaired);
        // Must NOT leak any literal MAC into runtime output.
        assert!(
            !s.contains("0x"),
            "NotPaired display must not include any hex literal: {}",
            s
        );
        assert!(
            s.contains("not paired"),
            "NotPaired display should indicate pairing issue: {}",
            s
        );
    }

    #[test]
    fn test_ble_error_display_not_configured() {
        let s = format!("{}", BleError::NotConfigured);
        assert!(
            s.contains(BD_ADDR_ENV),
            "NotConfigured display should name the env var: {}",
            s
        );
        assert!(
            s.contains(".env"),
            "NotConfigured display should mention the .env fallback: {}",
            s
        );
    }

    #[test]
    fn test_ble_error_display_winrt_failure() {
        let e = BleError::WinRTFailure {
            api: "FromBluetoothAddressAsync",
            hr: 0x80070002u32 as i32, // ERROR_FILE_NOT_FOUND
        };
        let s = format!("{}", e);
        assert!(
            s.contains("FromBluetoothAddressAsync"),
            "WinRTFailure display should contain API name: {}",
            s
        );
        assert!(
            s.contains("0x80070002"),
            "WinRTFailure display should contain HRESULT: {}",
            s
        );
    }

    #[test]
    fn test_ble_error_from_windows_error() {
        let win_err =
            windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005u32 as i32));
        let ble_err: BleError = win_err.into();
        match ble_err {
            BleError::WinRTFailure { api, hr } => {
                assert_eq!(api, "WinRT call");
                assert_eq!(hr, 0x80004005u32 as i32);
            }
            _ => panic!("From<Error> must produce WinRTFailure"),
        }
    }

    #[test]
    fn test_ble_error_implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&BleError::NotPaired);
        assert_error(&BleError::NotConfigured);
        assert_error(&BleError::WinRTFailure { api: "x", hr: 0 });
    }

    // ── parse_bd_addr ──────────────────────────────────────────────────────

    #[test]
    fn test_parse_bd_addr_hex_prefix() {
        assert_eq!(parse_bd_addr("0x0123456789AB"), Some(0x0123_4567_89ABu64));
    }

    #[test]
    fn test_parse_bd_addr_hex_prefix_lowercase() {
        assert_eq!(parse_bd_addr("0x0123456789ab"), Some(0x0123_4567_89ABu64));
    }

    #[test]
    fn test_parse_bd_addr_hex_prefix_uppercase_x() {
        assert_eq!(parse_bd_addr("0X0123456789AB"), Some(0x0123_4567_89ABu64));
    }

    #[test]
    fn test_parse_bd_addr_bare_hex() {
        assert_eq!(parse_bd_addr("0123456789AB"), Some(0x0123_4567_89ABu64));
    }

    #[test]
    fn test_parse_bd_addr_colon_separated() {
        assert_eq!(
            parse_bd_addr("01:23:45:67:89:AB"),
            Some(0x0123_4567_89ABu64)
        );
    }

    #[test]
    fn test_parse_bd_addr_hyphen_separated() {
        assert_eq!(
            parse_bd_addr("01-23-45-67-89-AB"),
            Some(0x0123_4567_89ABu64)
        );
    }

    #[test]
    fn test_parse_bd_addr_strips_whitespace() {
        assert_eq!(parse_bd_addr("  0123456789AB  "), Some(0x0123_4567_89ABu64));
    }

    #[test]
    fn test_parse_bd_addr_too_short() {
        assert_eq!(parse_bd_addr("0123456789A"), None);
    }

    #[test]
    fn test_parse_bd_addr_too_long() {
        assert_eq!(parse_bd_addr("0123456789ABC"), None);
    }

    #[test]
    fn test_parse_bd_addr_empty() {
        assert_eq!(parse_bd_addr(""), None);
    }

    #[test]
    fn test_parse_bd_addr_non_hex() {
        assert_eq!(parse_bd_addr("ZZZZZZZZZZZZ"), None);
    }

    #[test]
    fn test_parse_bd_addr_zero_rejected() {
        // 00:00:00:00:00:00 is the BLE wildcard / null address — reject it
        // so a misconfigured .env can't silently match the "never paired" path.
        assert_eq!(parse_bd_addr("00:00:00:00:00:00"), None);
    }

    // ── read_env_file_for_addr ────────────────────────────────────────────

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        // (pid, test name) is unique per process — each test passes a distinct `name`.
        p.push(format!(
            "switchboard-bletest-{}-{}.env",
            std::process::id(),
            name
        ));
        std::fs::write(&p, contents).expect("write temp .env");
        p
    }

    #[test]
    fn test_env_file_simple_assignment() {
        let p = write_temp("simple", "SWITCHBOARD_NUPHY_BD_ADDR=0x0123456789AB\n");
        assert_eq!(read_env_file_for_addr(&p), Some(0x0123_4567_89ABu64));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_export_prefix() {
        let p = write_temp("export", "export SWITCHBOARD_NUPHY_BD_ADDR=0123456789AB\n");
        assert_eq!(read_env_file_for_addr(&p), Some(0x0123_4567_89ABu64));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_double_quoted_value() {
        let p = write_temp(
            "dquote",
            "SWITCHBOARD_NUPHY_BD_ADDR=\"01:23:45:67:89:AB\"\n",
        );
        assert_eq!(read_env_file_for_addr(&p), Some(0x0123_4567_89ABu64));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_single_quoted_value() {
        let p = write_temp("squote", "SWITCHBOARD_NUPHY_BD_ADDR='0x0123456789AB'\n");
        assert_eq!(read_env_file_for_addr(&p), Some(0x0123_4567_89ABu64));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_ignores_comments_and_other_keys() {
        let body = "\
            # A comment\n\
            \n\
            UNRELATED=value\n\
            SWITCHBOARD_NUPHY_BD_ADDR=0x0123456789AB\n\
            ANOTHER=value2\n";
        let p = write_temp("mixed", body);
        assert_eq!(read_env_file_for_addr(&p), Some(0x0123_4567_89ABu64));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_missing_key_returns_none() {
        let p = write_temp("nokey", "OTHER=value\n");
        assert_eq!(read_env_file_for_addr(&p), None);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_unparseable_value_returns_none() {
        let p = write_temp("bad", "SWITCHBOARD_NUPHY_BD_ADDR=not-a-mac\n");
        assert_eq!(read_env_file_for_addr(&p), None);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_env_file_missing_path_returns_none() {
        let p = std::env::temp_dir().join("switchboard-bletest-does-not-exist.env");
        // Make sure it really doesn't exist.
        let _ = std::fs::remove_file(&p);
        assert_eq!(read_env_file_for_addr(&p), None);
    }
}
