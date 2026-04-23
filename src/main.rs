#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! switchboard — Main orchestration spine.
//!
//! This module implements the complete application lifecycle (see ARCHITECTURE.md).
//! Responsibilities:
//! - Single-instance mutex enforcement
//! - Cold-start unconditional ENABLE + verify (Safety Invariant 1)
//! - Worker thread (SetupAPI operations) with op_id correlation + stale-result immunity
//! - Message loop: BLE events, tray events, power/session transitions, worker results, sanity timer
//! - apply_policy — the decision core
//! - Quit sequence — Cmd::Shutdown + join + inline ENABLE fallback
//! - --recover argv path — inline ENABLE (no mutex, no worker, exit 0/1)
//!
//! Safety invariants satisfied (see safety-invariants.md):
//! - I1: Cold-start ENABLE first
//! - I2: All recovery ENABLEs verify
//! - I3: Predicate fail-closed
//! - I4: No cache (fresh reads every call)
//! - I5: Resume gating (resume_pending flag)
//! - I6: Worker-dead lockdown
//! - I7: Quit-must-recover
//! - I8: --recover inline path (skip mutex)
//! - I9: Stale-result immunity (op_id < current_generation)
//! - I10: Suspend-must-ENABLE
//! - I12: Tooltip truth

mod autostart;
mod ble;
mod boot_task;
mod device;
mod theme;

use log::{error, info, warn};
use std::env;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem},
    TrayIcon, TrayIconBuilder,
};
use windows::core::*;
use windows::Win32::Foundation::*;

use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::Power::*;
use windows::Win32::System::RemoteDesktop::*;

use windows::Win32::UI::WindowsAndMessaging::*;

// Explicit imports to ensure symbols are in scope (windows::core::* can shadow)
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, KillTimer,
    PostMessageW, PostQuitMessage, SetTimer, SetWindowLongPtrW, TranslateMessage, MSG,
    WINDOW_STYLE, WS_EX_TOOLWINDOW,
};

// Message IDs
const WM_TRAY: u32 = WM_APP + 2;
const WM_WORKER_RESULT: u32 = WM_APP + 3;

// Timer IDs
const TIMER_SANITY: usize = 1;
const TIMER_INITIAL_POLICY: usize = 2;
const SANITY_INTERVAL_MS: u32 = 20_000; // 20 seconds — sanity timer cadence
const INITIAL_POLICY_DELAY_MS: u32 = 3_000; // 3 seconds grace window

// Menu IDs
const ID_ACTIVE: u16 = 1001;
const ID_QUIT: u16 = 1002;
const ID_AUTOSTART: u16 = 1003;
const ID_BOOT_TASK: u16 = 1004;

// Resume timeout (2 minutes)
const RESUME_TIMEOUT: Duration = Duration::from_secs(120);

// Power setting GUIDs for RegisterPowerSettingNotification
// GUID_LIDSWITCH_STATE_CHANGE: {BA3E0F4D-B817-4094-A2D1-D56379E6A0F3}
const GUID_LIDSWITCH_STATE_CHANGE: GUID = GUID::from_u128(0xBA3E0F4D_B817_4094_A2D1_D56379E6A0F3);
// GUID_CONSOLE_DISPLAY_STATE: {6FE69556-704A-47A0-8F24-C28D936FDA47}
const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID::from_u128(0x6FE69556_704A_47A0_8F24_C28D936FDA47);

/// Sentinel value embedded in dwExtraInfo of synthetic modifier-release keystrokes.
/// Used to identify our own injected events and prevent re-processing.
/// Value: 0x5357_5442 = "SWTB" (SWitchBoard) in ASCII hex.
const SWITCHBOARD_SYNTHETIC_SENTINEL: usize = 0x5357_5442;

// Global shutdown cleanup flag (idempotency guard)
static SHUTDOWN_CLEANUP_DONE: AtomicBool = AtomicBool::new(false);

// Shared target for shutdown cleanup
static SHUTDOWN_TARGET: Mutex<Option<device::Target>> = Mutex::new(None);

/// Commands sent from main thread to worker thread.
#[derive(Debug)]
enum Cmd {
    Enable { op_id: u64 },
    Disable { target: device::Target, op_id: u64 },
    Shutdown,
}

/// Result from worker thread after Cmd::Disable (atomic disable+verify ).
#[derive(Debug)]
struct DisableResult {
    op_id: u64,
    disable_ok: bool,
    verify_state: Option<device::KeyboardState>,
    err: Option<String>,
}

/// Application state held by main thread.
struct AppState {
    hwnd: HWND,
    desired_active: bool,    // init true (or false if crash detected)
    current_generation: u64, // monotonic, bumped on state-changing events
    op_id: u64,              // monotonic, each worker Cmd
    resume_pending: bool,
    resume_timestamp: Option<Instant>,
    worker_dead: bool,
    worker_tx: Sender<Cmd>,
    worker_handle: Option<JoinHandle<()>>, // Some until Quit, then None after join attempt
    ble: Option<ble::BleHandle>,
    _tray_icon: TrayIcon,
    active_item: CheckMenuItem,
    autostart_item: CheckMenuItem,
    boot_task_item: CheckMenuItem,
    cached_target: Option<device::Target>, // Cached for shutdown cleanup
    initial_policy_pending: bool,          // True if TIMER_INITIAL_POLICY is active
    is_elevated: bool,                     // True if running as administrator
    current_theme_light: bool,             // Cached taskbar theme; toggled by WM_SETTINGCHANGE
    last_checkmark_resync: Option<Instant>, // Throttle for resync_external_checkmarks
}

impl AppState {
    fn nuphy_connected(&self) -> bool {
        self.ble.as_ref().map(|b| b.is_connected()).unwrap_or(false)
    }
}

/// Shutdown cleanup: re-enable internal keyboard on exit (fail-open invariant).
/// Idempotent - safe to call multiple times.
///
/// Order matters: re-enable the device FIRST, delete `running.lock` LAST. If the
/// process is killed mid-cleanup (e.g. Windows shutdown timeout), the surviving
/// lock file correctly tells the next launch that the previous session crashed
/// without completing recovery, so it starts in inactive (safe) mode.
fn shutdown_cleanup() {
    // Check if already done
    if SHUTDOWN_CLEANUP_DONE.swap(true, Ordering::SeqCst) {
        return; // Already cleaned up
    }

    // Get the cached target
    let target = match SHUTDOWN_TARGET.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            warn!("shutdown: failed to lock SHUTDOWN_TARGET");
            // No target → can't re-enable; still drop the lock so we don't trip
            // crash detection on next launch when there was nothing to recover.
            delete_running_lock();
            return;
        }
    };

    let target = match target {
        Some(t) => t,
        None => {
            info!("shutdown: no target cached, skipping re-enable");
            // No worker ever resolved a target → nothing was disabled, safe to clear.
            delete_running_lock();
            return;
        }
    };

    info!(
        "shutdown: re-enabling target keyboard {}",
        target.instance_id
    );

    match device::enable(&target) {
        Ok(_) => {
            info!("shutdown: re-enable ok");
            // Only after a confirmed re-enable do we mark this exit as clean.
            delete_running_lock();
        }
        Err(e) => {
            warn!("shutdown: re-enable failed: {} — leaving running.lock so next launch enters crash recovery", e);
        }
    }
}

/// Drop guard for shutdown cleanup - belt-and-suspenders safety net.
struct ShutdownGuard;

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        shutdown_cleanup();
    }
}

/// Log a startup phase to %LOCALAPPDATA%\switchboard\phase.log for diagnostics.
/// Used to debug silent failures in windows_subsystem mode.
fn log_phase(phase: &str) {
    use std::io::Write;
    let log_path = std::env::var("LOCALAPPDATA")
        .ok()
        .and_then(|p| {
            let path = std::path::PathBuf::from(p)
                .join("switchboard")
                .join("phase.log");
            std::fs::create_dir_all(path.parent()?).ok()?;
            Some(path)
        })
        .unwrap_or_else(|| std::env::temp_dir().join("switchboard-phase.log"));

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let msg = format!("[UNIX:{}] {}\n", timestamp, phase);

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| f.write_all(msg.as_bytes()));
}

/// Check if the current process is running with administrator privileges.
/// Uses GetTokenInformation with TokenElevation to check elevation status.
fn check_elevation() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut return_length: u32 = 0;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        );

        let _ = CloseHandle(token);

        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

fn main() {
    // PANIC HOOK - Install FIRST, before anything else can panic
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let log_path = std::env::var("LOCALAPPDATA")
            .ok()
            .and_then(|p| {
                let path = std::path::PathBuf::from(p)
                    .join("switchboard")
                    .join("panic.log");
                std::fs::create_dir_all(path.parent()?).ok()?;
                Some(path)
            })
            .unwrap_or_else(|| std::env::temp_dir().join("switchboard-panic.log"));

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let msg = format!("[UNIX:{}] PANIC: {}\n", timestamp, info);

        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .and_then(|mut f| f.write_all(msg.as_bytes()));
    }));

    log_phase("panic_hook_installed");

    // Attach to parent console if launched from terminal (CLI subcommands need output)
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    log_phase("attach_console_done");

    // Request a shutdown priority near the top of the user-app range (0x000-0x3FF)
    // so switchboard receives WM_ENDSESSION early and Windows allows it more time to
    // re-enable the internal keyboard before termination. Flag 0x1 = SHUTDOWN_NORETRY,
    // which skips Windows' "this app is preventing shutdown" dialog if our handler
    // is slow.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::SetProcessShutdownParameters;
        let _ = SetProcessShutdownParameters(0x3FF, 0x0000_0001);
    }
    log_phase("shutdown_priority_set");

    // Shutdown guard - ensures cleanup on all exit paths including panic
    let _shutdown_guard = ShutdownGuard;

    // argv --recover FIRST (before mutex)
    if env::args().any(|a| a == "--recover") {
        log_phase("recover_mode_branch");
        recover_mode(); // Does not return
    }

    // Admin subcommands: --install-boot-task / --uninstall-boot-task.
    // These self-elevate via ShellExecuteW("runas") if not already admin,
    // do their work, and exit. They never touch the mutex / tray.
    if let Some(sub) = parse_admin_subcommand() {
        log_phase("admin_subcommand_branch");
        run_admin_subcommand(sub); // Does not return
    }

    log_phase("checking_single_instance");
    // Single-instance mutex
    let mutex_handle = match acquire_single_instance_mutex() {
        Ok(h) => h,
        Err(already_running) => {
            if already_running {
                // ERROR_ALREADY_EXISTS — another instance owns the mutex
                unsafe {
                    MessageBoxW(
                        HWND(std::ptr::null_mut()),
                        w!("SwitchBoard is already running — check the tray."),
                        w!("SwitchBoard"),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
                std::process::exit(1);
            } else {
                // Mutex creation failed for other reason — log and abort
                eprintln!("Failed to create single-instance mutex");
                std::process::exit(1);
            }
        }
    };
    log_phase("single_instance_check_done");

    // Log init ( Row 1, step 3)
    init_logging();
    log_phase("logging_init_done");

    info!("switchboard v{} starting", env!("CARGO_PKG_VERSION"));

    // Check if running as administrator (required for device enable/disable)
    let is_elevated = check_elevation();
    if !is_elevated {
        warn!("Not running as administrator — keyboard control unavailable");
    } else {
        info!("Running as administrator — full functionality available");
    }

    // Install Ctrl-C handler for console-attached scenarios
    unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> BOOL {
        info!("Ctrl-C handler: shutdown cleanup");
        shutdown_cleanup();
        FALSE // Allow default handler to proceed
    }
    unsafe {
        SetConsoleCtrlHandler(Some(ctrl_handler), TRUE).ok();
    }

    // Step 4: COLD-START UNCONDITIONAL ENABLE + verify (Invariant I1)
    // This is BEFORE any BLE subscribe, tray creation, or worker spawn — Recovery primitive.
    log_phase("cold_start_start");
    info!("Cold start: unconditional ENABLE + verify (Invariant I1)");

    // Check for running.lock before we create it (crash signal #1)
    let running_lock_existed = check_running_lock();
    if running_lock_existed {
        warn!("Cold start: running.lock exists — previous instance crashed without cleanup");
    }

    // Create running.lock BEFORE enable
    create_running_lock();

    let target = match device::resolve() {
        device::ResolveResult::Ok(t) => t,
        other => {
            error!("Cold start: resolve failed: {:?}", other);
            log_resolve_dump(&other);
            log_phase("cold_start_resolve_failed_continuing");
            // Cannot proceed without target. Keep tray alive (no BLE, no worker) for manual Quit → retry.
            let (tray_icon, active_item) = create_tray_minimal();
            notify_user("Keyboard target not found on this system — see log.");
            run_minimal_message_loop(tray_icon, active_item);
            // Cleanup
            delete_running_lock();
            unsafe {
                CloseHandle(mutex_handle).ok();
            }
            std::process::exit(1);
        }
    };

    // Inline ENABLE for cold start (no worker exists yet) — capture outcome
    // If not elevated, skip enable and continue to tray
    let enable_outcome = if !is_elevated {
        warn!("Cold start: skipping ENABLE (not elevated)");
        notify_user(
            "SwitchBoard needs to run as administrator. Right-click → Run as administrator.",
        );
        log_phase("cold_start_skipped_not_elevated");
        device::EnableOutcome::WasAlreadyEnabled
    } else {
        match device::enable(&target) {
            Ok(outcome) => {
                log_phase("cold_start_enable_ok");
                outcome
            }
            Err(e) => {
                error!("Cold start ENABLE failed: {}", e);
                notify_user("Recovery failed — see log.");
                log_phase("cold_start_enable_failed_continuing");
                // Continue to tray anyway
                device::EnableOutcome::WasAlreadyEnabled
            }
        }
    };

    log_phase("cold_start_done");

    // First-run "lockout protection" auto-install: if we're elevated and the
    // boot-recovery task isn't installed yet, install it once. The marker file
    // means we won't re-install it after a user explicitly toggles it off from
    // the tray (a respected opt-out). Lockout protection is the only safety
    // net against a hard switchboard crash leaving the internal keyboard disabled
    // across a reboot, so we want it on by default.
    if is_elevated {
        maybe_auto_install_lockout_protection();
    }

    // Verify cold-start ENABLE (Invariant I2) — only if we actually tried to enable
    if is_elevated {
        match device::current_state(&target) {
            Ok(device::KeyboardState::Enabled) => {
                info!("Cold start verify: Enabled (success)");
            }
            Ok(device::KeyboardState::Disabled) => {
                error!("Cold start verify: still Disabled after ENABLE — failed recovery");
                warn!("Continuing to tray creation despite verify failure");
                log_phase("cold_start_verify_failed_continuing");
            }
            Err(e) => {
                error!("Cold start verify error: {}", e);
                warn!("Continuing to tray creation despite verify error");
                log_phase("cold_start_verify_error_continuing");
            }
        }
    } else {
        info!("Cold start verify: skipped (not elevated)");
    }

    // Step 5: Spawn SetupAPI worker thread
    let (worker_tx, worker_rx) = mpsc::channel::<Cmd>();
    let worker_handle = thread::spawn(move || worker_thread_main(worker_rx));

    // Step 6: Create hidden HWND_MESSAGE window
    let hwnd = create_hidden_message_window();

    // Step 7: Register session notifications
    unsafe {
        if let Err(e) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
            warn!("WTSRegisterSessionNotification failed: {:?}", e);
        }

        // Register power setting notifications for lid switch and display state
        // Critical for Modern Standby devices (Surface) that skip PBT_APMSUSPEND
        let lid_handle = RegisterPowerSettingNotification(
            HANDLE(hwnd.0),
            &GUID_LIDSWITCH_STATE_CHANGE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        );
        if let Err(e) = lid_handle {
            warn!("RegisterPowerSettingNotification (lid) failed: {:?}", e);
        } else {
            info!("Power notification: lid switch registered");
        }

        let display_handle = RegisterPowerSettingNotification(
            HANDLE(hwnd.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        );
        if let Err(e) = display_handle {
            warn!("RegisterPowerSettingNotification (display) failed: {:?}", e);
        } else {
            info!("Power notification: console display registered");
        }
    }

    // Step 8: BLE subscribe
    let ble_handle = match ble::start(hwnd) {
        Ok(h) => {
            info!("BLE: subscribed successfully");
            Some(h)
        }
        Err(ble::BleError::NotConfigured) => {
            error!(
                "BLE: BD_ADDR not configured (set {} env var or add to .env) — internal keyboard will stay enabled this session.",
                ble::BD_ADDR_ENV
            );
            notify_user(
                "Nuphy BD_ADDR not configured — internal keyboard will stay enabled. See README.",
            );
            None
        }
        Err(ble::BleError::NotPaired) => {
            error!("BLE: Nuphy not paired — internal keyboard will stay enabled this session.");
            notify_user("Nuphy not paired — internal keyboard will stay enabled this session.");
            None
        }
        Err(e) => {
            error!("BLE: start failed: {}", e);
            notify_user("BLE initialization failed — see log.");
            None
        }
    };

    // Step 9: Crash detection
    // The only reliable crash signal is `running.lock` surviving across launches —
    // it can only persist when a previous switchboard instance was terminated before
    // shutdown_cleanup() finished re-enabling the device. `enable_outcome ==
    // WasDisabled` is NOT a crash signal: many benign causes (manual disable in
    // Device Manager, third-party tools, prior version before lock-file existed)
    // produce the same observation, and the cold-start enable() above already
    // restored I1 safety. Logged below for diagnostic context only.
    let crashed = running_lock_existed;

    // ALWAYS default to active on launch (per user directive 2026-04-22).
    // Cold-start enable() above already restored safety (I1). Crash recovery
    // re-enables the keyboard but no longer forces inactive mode.
    let initial_desired_active = true;

    // Log crash recovery diagnostics but do NOT alter desired_active
    if crashed && is_elevated {
        warn!(
            "CRASH RECOVERY: running.lock existed (enable_outcome={:?}). Internal keyboard re-enabled. App starting active (default).",
            enable_outcome
        );
        notify_user(
            "SwitchBoard recovered from an unclean shutdown. Internal keyboard re-enabled. App is active."
        );
    } else if crashed && !is_elevated {
        warn!(
            "CRASH DETECTED but not elevated (enable_outcome={:?}, running_lock_existed={}) — internal keyboard state confirmed safe",
            enable_outcome, running_lock_existed
        );
    }

    log_phase("tray_init_start");
    // Step 10: Tray (with initial state reflecting crash detection or admin requirement)
    let (tray_icon, active_item, autostart_item, boot_task_item) = if !is_elevated {
        // Not elevated — create tray with "needs admin" state
        log_phase("calling_create_tray_needs_admin");
        create_tray_needs_admin(hwnd)
    } else {
        log_phase("calling_create_tray");
        create_tray(hwnd, initial_desired_active)
    };
    log_phase("tray_menu_built");

    // Step 11: 20-s sanity timer
    unsafe {
        SetTimer(hwnd, TIMER_SANITY, SANITY_INTERVAL_MS, None);
    }

    // Build AppState
    let mut state = AppState {
        hwnd,
        desired_active: initial_desired_active,
        current_generation: 0,
        op_id: 0,
        resume_pending: false,
        resume_timestamp: None,
        worker_dead: false,
        worker_tx,
        worker_handle: Some(worker_handle),
        ble: ble_handle,
        _tray_icon: tray_icon,
        active_item: active_item.clone(),
        autostart_item: autostart_item.clone(),
        boot_task_item: boot_task_item.clone(),
        cached_target: None,
        initial_policy_pending: false,
        is_elevated,
        current_theme_light: theme::system_uses_light_theme(),
        last_checkmark_resync: None,
    };

    update_tooltip(&state);

    // Stale-path detection for both autostart and boot-recovery task.
    // If the registered paths don't match current_exe(), append a ⚠ to
    // the corresponding menu label and pop a single combined warning
    // dialog (on a worker thread so it doesn't block the message loop).
    refresh_stale_indicators(&mut state);

    // Step 12: Initial apply_policy — deferred via 3s timer
    // Start TIMER_INITIAL_POLICY for grace window
    unsafe {
        SetTimer(hwnd, TIMER_INITIAL_POLICY, INITIAL_POLICY_DELAY_MS, None);
    }
    state.initial_policy_pending = true;

    // Update tooltip to show arming status
    update_tooltip(&state);

    // Step 13: Message loop
    info!("Entering message loop");
    run_message_loop(&mut state);

    // Step 13: Post-loop cleanup
    info!("Message loop exited — cleaning up");
    unsafe {
        WTSUnRegisterSessionNotification(hwnd).ok();
        CloseHandle(mutex_handle).ok();
    }

    info!("switchboard v0.1 exiting");
    std::process::exit(0);
}

/// --recover mode: inline ENABLE (no mutex, no worker), verify, exit 0/1.
/// Shares code path with Quit fallback.,, Invariant I8.
fn recover_mode() -> ! {
    // Init logger (append mode)
    init_logging();
    info!("--recover mode: inline ENABLE path");

    let target = match device::resolve() {
        device::ResolveResult::Ok(t) => t,
        device::ResolveResult::NoMatch { dump: _ } => {
            error!("--recover: resolve NoMatch — no matching keyboard");
            std::process::exit(1);
        }
        device::ResolveResult::MultipleMatches {
            candidates: _,
            dump: _,
        } => {
            error!("--recover: resolve MultipleMatches — ambiguous target");
            std::process::exit(1);
        }
        device::ResolveResult::EnumerationError(e) => {
            error!("--recover: enumeration error: {}", e);
            std::process::exit(1);
        }
    };

    // First ENABLE attempt
    if let Err(e) = device::enable(&target) {
        warn!("--recover: first ENABLE failed: {} — retrying once", e);
        std::thread::sleep(Duration::from_millis(500));
        if let Err(e2) = device::enable(&target) {
            error!("--recover: second ENABLE failed: {}", e2);
            std::process::exit(1);
        }
    }

    // Verify
    match device::current_state(&target) {
        Ok(device::KeyboardState::Enabled) => {
            info!("--recover: verify Enabled — success");
            std::process::exit(0);
        }
        Ok(device::KeyboardState::Disabled) => {
            error!("--recover: verify still Disabled after ENABLE — retry once");
            std::thread::sleep(Duration::from_millis(500));
            if let Err(e) = device::enable(&target) {
                error!("--recover: retry ENABLE failed: {}", e);
                std::process::exit(1);
            }
            match device::current_state(&target) {
                Ok(device::KeyboardState::Enabled) => {
                    info!("--recover: retry verify Enabled — success");
                    std::process::exit(0);
                }
                _ => {
                    error!("--recover: retry verify failed");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            error!("--recover: verify error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Acquire single-instance mutex. Returns Ok(handle) on success.
/// Returns Err(true) if ERROR_ALREADY_EXISTS (another instance running).
/// Returns Err(false) on other failure.
fn acquire_single_instance_mutex() -> std::result::Result<HANDLE, bool> {
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\switchboard-singleton-v1"));
        match h {
            Ok(handle) => {
                let last_err = GetLastError();
                if last_err == ERROR_ALREADY_EXISTS {
                    // Mutex exists but we didn't get ownership
                    let _ = CloseHandle(handle);
                    Err(true)
                } else {
                    // WAIT_ABANDONED or fresh creation — we own it
                    Ok(handle)
                }
            }
            Err(_) => Err(false),
        }
    }
}

/// Initialize logging to %LOCALAPPDATA%\switchboard\switchboard.log (append mode).
fn init_logging() {
    let local_app_data =
        env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Public".to_string());
    let log_dir = format!("{}\\switchboard", local_app_data);
    std::fs::create_dir_all(&log_dir).ok();
    let log_path = format!("{}\\switchboard.log", log_dir);

    // Open log file - fail silently if it can't be opened (e.g., no console in windows subsystem)
    let log_file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(_) => return, // Silently skip logging if file can't be opened
    };

    simplelog::WriteLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        log_file,
    )
    .ok(); // If already initialized (e.g., in --recover), ignore error
}

/// Get path to running.lock file in %LOCALAPPDATA%\switchboard\running.lock
fn get_running_lock_path() -> String {
    let local_app_data =
        env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Public".to_string());
    format!("{}\\switchboard\\running.lock", local_app_data)
}

/// Marker indicating the user has been offered (and either accepted or
/// explicitly declined via the tray) lockout protection at least once.
/// Presence of this file suppresses the first-run auto-install.
fn get_lockout_offered_marker_path() -> String {
    let local_app_data =
        env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Public".to_string());
    format!(
        "{}\\switchboard\\lockout-protection-offered",
        local_app_data
    )
}

/// On first elevated launch, install the boot-recovery (lockout protection)
/// task automatically and write a marker so we never re-install it after a
/// user disables it from the tray. No-op if the marker exists or the task is
/// already installed.
fn maybe_auto_install_lockout_protection() {
    let marker = get_lockout_offered_marker_path();
    if std::path::Path::new(&marker).exists() {
        return;
    }

    if boot_task::is_installed() {
        // Already installed (perhaps via a CLI flag or older build) — record
        // that we've considered it so we don't re-prompt later.
        let _ = std::fs::write(&marker, "previously-installed");
        return;
    }

    info!("First elevated launch: auto-installing lockout protection (boot recovery task)");
    match boot_task::install() {
        Ok(_) => {
            info!("Auto-install lockout protection: success");
            // Write marker only on success so a transient COM/RPC failure leaves
            // us free to retry next launch.
            let _ = std::fs::write(&marker, "auto-installed");
            notify_user(
                "Lockout protection enabled. If SwitchBoard ever crashes with the internal keyboard \
                 disabled, Windows will re-enable it on the next boot so you can log in. \
                 Disable from the tray menu if you don't want this.",
            );
        }
        Err(e) => {
            warn!(
                "Auto-install lockout protection failed: {} — will retry next elevated launch",
                e
            );
        }
    }
}

/// Check if running.lock exists (crash signal). Returns true if exists.
fn check_running_lock() -> bool {
    let lock_path = get_running_lock_path();
    std::path::Path::new(&lock_path).exists()
}

/// Create running.lock marker file
fn create_running_lock() {
    let lock_path = get_running_lock_path();
    if let Err(e) = std::fs::write(&lock_path, "running") {
        warn!("Failed to create running.lock: {}", e);
    }
}

/// Delete running.lock marker file (clean shutdown)
fn delete_running_lock() {
    let lock_path = get_running_lock_path();
    if let Err(e) = std::fs::remove_file(&lock_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!("Failed to delete running.lock: {}", e);
        }
    }
}

/// Create a hidden top-level window for receiving BLE / worker / power / session events.
///
/// NOTE: We deliberately do NOT use HWND_MESSAGE here. Message-only windows do not
/// receive broadcast messages such as WM_QUERYENDSESSION / WM_ENDSESSION, which
/// switchboard relies on to re-enable the internal keyboard before Windows reboots
/// or logoff terminates the process. We use a regular top-level window with
/// no caption, no visible style, and WS_EX_TOOLWINDOW so it stays out of the
/// taskbar and Alt-Tab. Style is 0 (no WS_OVERLAPPED) to minimize risk of any
/// accidental visual exposure.
fn create_hidden_message_window() -> HWND {
    use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;

    unsafe {
        let class_name = w!("switchboard_msg_window");
        let hinstance = GetModuleHandleW(None)
            .expect("GetModuleHandleW failed")
            .into();

        // Register class (idempotent — check first)
        let mut existing = WNDCLASSW::default();
        if windows::Win32::UI::WindowsAndMessaging::GetClassInfoW(
            hinstance,
            class_name,
            &mut existing,
        )
        .is_err()
        {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance,
                lpszClassName: class_name,
                ..Default::default()
            };
            let atom = windows::Win32::UI::WindowsAndMessaging::RegisterClassW(&wc);
            if atom == 0 {
                let err = windows::core::Error::from_win32();
                log_phase(&format!("FATAL: RegisterClassW failed: {:?}", err));
                shutdown_cleanup();
                std::process::exit(1);
            }
        }

        CreateWindowExW(
            WS_EX_TOOLWINDOW, // keep out of Alt-Tab and the taskbar
            class_name,
            w!("switchboard"),
            WINDOW_STYLE(0), // no WS_VISIBLE → never shown
            0,
            0,
            0,
            0,
            None, // top-level (parent = NULL) so we receive WM_QUERYENDSESSION / WM_ENDSESSION
            None,
            hinstance,
            None,
        )
        .expect("CreateWindowExW failed")
    }
}

/// Window procedure for hidden message window.
/// Dispatches to handlers based on message type.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Retrieve AppState pointer from window user data (set during message loop)
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
    if state_ptr.is_null() && msg != WM_CREATE && msg != WM_DESTROY {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    match msg {
        ble::WM_BLE_CONNECTION_CHANGED => {
            // BLE event → apply_policy
            info!("WM_BLE_CONNECTION_CHANGED received");
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;

                // Cancel initial policy timer if still pending
                if state.initial_policy_pending {
                    info!("BLE event: canceling initial policy timer");
                    let _ = KillTimer(hwnd, TIMER_INITIAL_POLICY);
                    state.initial_policy_pending = false;
                }

                apply_policy(state);
            }
            LRESULT(0)
        }
        WM_TRAY => {
            // Tray menu event
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                let menu_id = wparam.0 as u16;
                handle_tray_event(state, menu_id);
            }
            LRESULT(0)
        }
        WM_WORKER_RESULT => {
            // Worker result (atomic disable+verify )
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                let result_ptr = lparam.0 as *mut DisableResult;
                if !result_ptr.is_null() {
                    let result = Box::from_raw(result_ptr);
                    handle_worker_result(state, *result);
                }
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            // Power transitions
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                handle_power_event(state, wparam.0 as u32, lparam);
            }
            LRESULT(1) // Return TRUE to not block
        }
        WM_WTSSESSION_CHANGE => {
            // Session unlock
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                handle_session_event(state, wparam.0 as u32);
            }
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            // Light/dark taskbar swap (Soft Accent decision, 2026-04-22).
            // lParam is a wide C string naming the changed setting; only
            // "ImmersiveColorSet" indicates a personalization toggle.
            if !state_ptr.is_null() && theme::is_immersive_color_set(lparam.0) {
                let state = &mut *state_ptr;
                refresh_tray_theme(state);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_QUERYENDSESSION => {
            // Shutdown query — return TRUE immediately. Per Microsoft
            // guidance, apps must return from WM_QUERYENDSESSION quickly; deferring
            // device::enable() (50–200ms+) here can cause Windows to abort us
            // before WM_ENDSESSION ever arrives. Real cleanup happens below.
            info!("WM_QUERYENDSESSION: allowing shutdown");
            LRESULT(1) // TRUE — allow shutdown
        }
        WM_ENDSESSION => {
            // Shutdown final → re-enable + clear lock (the only reliable
            // hook for system shutdown / logoff). Requires a top-level window — see
            // create_hidden_message_window for why HWND_MESSAGE was abandoned.
            if wparam.0 != 0 {
                info!("WM_ENDSESSION: wparam=true, shutdown cleanup");
                shutdown_cleanup();
            }
            LRESULT(0)
        }
        WM_TIMER => {
            // Sanity timer + initial policy timer
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                if wparam.0 == TIMER_SANITY {
                    handle_sanity_timer(state);
                } else if wparam.0 == TIMER_INITIAL_POLICY {
                    // Initial policy grace window expired
                    info!("Initial policy timer fired — applying policy");
                    let _ = KillTimer(hwnd, TIMER_INITIAL_POLICY);
                    state.initial_policy_pending = false;
                    apply_policy(state);
                    update_tooltip(state);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // User closed window → cleanup
            info!("WM_CLOSE: shutdown cleanup");
            shutdown_cleanup();
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            info!("WM_DESTROY: shutdown cleanup");
            shutdown_cleanup();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Create tray icon with menu. Returns (TrayIcon, Active, Autostart, Boot-task) check items.
fn create_tray(
    hwnd: HWND,
    initial_active: bool,
) -> (TrayIcon, CheckMenuItem, CheckMenuItem, CheckMenuItem) {
    let active_item = CheckMenuItem::new("Active", true, initial_active, None);
    let autostart_initial = autostart::is_enabled();
    let autostart_item = CheckMenuItem::new(
        "Auto-start SwitchBoard at login",
        true,
        autostart_initial,
        None,
    );

    // boot_task::is_installed() can fail (COM init, no admin, etc.) - treat as false
    let boot_task_initial = boot_task::is_installed();

    let boot_task_item = CheckMenuItem::new(
        "Lockout protection (recommended)",
        true,
        boot_task_initial,
        None,
    );
    let quit_item = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    menu.append(&active_item).ok();
    menu.append(&autostart_item).ok();
    menu.append(&boot_task_item).ok();
    menu.append(&quit_item).ok();

    let tray_icon = TrayIconBuilder::new()
        .with_icon(current_theme_icon(theme::system_uses_light_theme()))
        .with_tooltip("SwitchBoard | initializing…")
        .with_menu(Box::new(menu))
        .build()
        .unwrap_or_else(|e| {
            log_phase(&format!("FATAL: tray_icon.build() failed: {:?}", e));
            // Cold-start ENABLE already ran (Invariant I1); call shutdown_cleanup
            // defensively in case anything mutated the keyboard, then exit cleanly
            // rather than panicking — this preserves the fail-open posture.
            shutdown_cleanup();
            std::process::exit(1);
        });

    let hwnd_raw = hwnd.0 as isize;
    let active_id = active_item.id().clone();
    let autostart_id = autostart_item.id().clone();
    let boot_task_id = boot_task_item.id().clone();
    let quit_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = if event.id == active_id {
            ID_ACTIVE
        } else if event.id == autostart_id {
            ID_AUTOSTART
        } else if event.id == boot_task_id {
            ID_BOOT_TASK
        } else if event.id == quit_id {
            ID_QUIT
        } else {
            return;
        };
        unsafe {
            PostMessageW(
                HWND(hwnd_raw as *mut _),
                WM_TRAY,
                WPARAM(id as usize),
                LPARAM(0),
            )
            .ok();
        }
    }));

    (tray_icon, active_item, autostart_item, boot_task_item)
}

/// Create tray icon for non-admin mode (needs elevation).
/// Returns same signature as create_tray for drop-in replacement.
fn create_tray_needs_admin(hwnd: HWND) -> (TrayIcon, CheckMenuItem, CheckMenuItem, CheckMenuItem) {
    // Dummy check items (all disabled)
    let active_item = CheckMenuItem::new("Active (needs admin)", false, false, None);
    let autostart_item = CheckMenuItem::new(
        "Auto-start SwitchBoard at login (needs admin)",
        false,
        false,
        None,
    );
    let boot_task_item = CheckMenuItem::new("Lockout protection (needs admin)", false, false, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    menu.append(&active_item).ok();
    menu.append(&autostart_item).ok();
    menu.append(&boot_task_item).ok();
    menu.append(&quit_item).ok();

    let tray_icon = TrayIconBuilder::new()
        .with_icon(current_theme_icon(theme::system_uses_light_theme()))
        .with_tooltip("SwitchBoard | Needs administrator privileges")
        .with_menu(Box::new(menu))
        .build()
        .unwrap_or_else(|e| {
            log_phase(&format!(
                "FATAL: tray_icon.build() (needs_admin) failed: {:?}",
                e
            ));
            shutdown_cleanup();
            std::process::exit(1);
        });

    let hwnd_raw = hwnd.0 as isize;
    let quit_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            unsafe {
                PostMessageW(
                    HWND(hwnd_raw as *mut _),
                    WM_TRAY,
                    WPARAM(ID_QUIT as usize),
                    LPARAM(0),
                )
                .ok();
            }
        }
    }));

    (tray_icon, active_item, autostart_item, boot_task_item)
}

/// Tray icon resource IDs — must match `manifest/switchboard.rc`.
const IDI_TRAY_DARK: u16 = 101;
const IDI_TRAY_LIGHT: u16 = 102;

/// Load the tray icon from the embedded Win32 resources, picked to match the
/// current taskbar theme. Falls back to a flat blue square only if both the
/// themed and dark resources fail to load (should never happen in a normal
/// build — embed-resource compiles them in unconditionally).
fn current_theme_icon(light: bool) -> tray_icon::Icon {
    let primary = if light { IDI_TRAY_LIGHT } else { IDI_TRAY_DARK };
    if let Ok(icon) = tray_icon::Icon::from_resource(primary, None) {
        return icon;
    }
    log_phase(&format!(
        "WARN: Icon::from_resource({}) failed; trying dark fallback",
        primary
    ));
    if let Ok(icon) = tray_icon::Icon::from_resource(IDI_TRAY_DARK, None) {
        return icon;
    }
    log_phase("WARN: both themed icons failed; using flat fallback");
    flat_fallback_icon()
}

/// Solid-colour 16x16 icon used only if every embedded ICO fails to load.
fn flat_fallback_icon() -> tray_icon::Icon {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for _ in 0..(16 * 16) {
        rgba.extend_from_slice(&[0x00, 0x78, 0xD4, 0xFF]); // Surface Blue
    }
    tray_icon::Icon::from_rgba(rgba, 16, 16).unwrap_or_else(|e| {
        log_phase(&format!("FATAL: Icon::from_rgba() failed: {:?}", e));
        shutdown_cleanup();
        std::process::exit(1);
    })
}

/// Re-read the theme registry value and, if it changed, swap the tray icon.
/// Called from `WM_SETTINGCHANGE` when lParam == "ImmersiveColorSet".
/// `tray_icon::TrayIcon::set_icon` owns the new HICON internally and
/// destroys the previous one, so there is no handle leak from this path.
fn refresh_tray_theme(state: &mut AppState) {
    let now_light = theme::system_uses_light_theme();
    if now_light == state.current_theme_light {
        return;
    }
    info!(
        "Taskbar theme changed: {} → {}",
        if state.current_theme_light {
            "light"
        } else {
            "dark"
        },
        if now_light { "light" } else { "dark" }
    );
    state.current_theme_light = now_light;
    if let Err(e) = state
        ._tray_icon
        .set_icon(Some(current_theme_icon(now_light)))
    {
        warn!("Failed to swap tray icon on theme change: {:?}", e);
    }
}

/// Create minimal tray for error states (no worker, no BLE). Just Quit item.
fn create_tray_minimal() -> (TrayIcon, CheckMenuItem) {
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append(&quit_item).ok();

    let tray_icon = TrayIconBuilder::new()
        .with_icon(current_theme_icon(theme::system_uses_light_theme()))
        .with_tooltip("SwitchBoard (error state)")
        .with_menu(Box::new(menu))
        .build()
        .unwrap_or_else(|e| {
            log_phase(&format!(
                "FATAL: tray_icon.build() (minimal) failed: {:?}",
                e
            ));
            shutdown_cleanup();
            std::process::exit(1);
        });

    // Dummy active item (not used)
    let active_item = CheckMenuItem::new("Active", false, false, None);

    (tray_icon, active_item)
}

/// Run minimal message loop for error states (no AppState, just wait for Quit).
fn run_minimal_message_loop(_tray_icon: TrayIcon, _active_item: CheckMenuItem) {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Run full message loop with AppState.
fn run_message_loop(state: &mut AppState) {
    unsafe {
        // Store AppState pointer in window user data for wndproc access
        SetWindowLongPtrW(state.hwnd, GWLP_USERDATA, state as *mut AppState as isize);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Handle tray menu event (Active toggle or Quit).
fn handle_tray_event(state: &mut AppState, menu_id: u16) {
    match menu_id {
        ID_ACTIVE => {
            // Toggle desired_active

            // Cancel initial policy timer if still pending
            if state.initial_policy_pending {
                info!("Tray toggle: canceling initial policy timer");
                unsafe {
                    let _ = KillTimer(state.hwnd, TIMER_INITIAL_POLICY);
                }
                state.initial_policy_pending = false;
            }

            state.desired_active = !state.desired_active;
            state.current_generation += 1;
            state.active_item.set_checked(state.desired_active);
            info!("Tray: Active toggled to {}", state.desired_active);
            update_tooltip(state);
            apply_policy(state);
        }
        ID_AUTOSTART => {
            // Per-user HKCU Run-key toggle. No admin needed.
            let currently_enabled = autostart::is_enabled();
            if currently_enabled {
                match autostart::disable() {
                    Ok(()) => {
                        state.autostart_item.set_checked(false);
                        info!("✓ Autostart disabled");
                    }
                    Err(e) => {
                        error!("Autostart disable failed: {e}");
                        // Re-sync the menu check state to actual registry state.
                        state.autostart_item.set_checked(autostart::is_enabled());
                    }
                }
            } else {
                match autostart::enable() {
                    Ok(()) => {
                        state.autostart_item.set_checked(true);
                        let path = std::env::current_exe()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| "<unknown>".to_string());
                        info!("✓ Autostart enabled: {path}");
                    }
                    Err(e) => {
                        error!("Autostart enable failed: {e}");
                        state.autostart_item.set_checked(autostart::is_enabled());
                    }
                }
            }
            refresh_stale_indicators(state);
        }
        ID_BOOT_TASK => {
            // Boot recovery task — needs admin. Either call directly (if we
            // happen to be elevated) or relaunch self with the appropriate
            // CLI subcommand under "runas" so UAC elevates the child.
            let currently_installed = boot_task::is_installed();
            let target_arg = if currently_installed {
                "--uninstall-boot-task"
            } else {
                "--install-boot-task"
            };

            let result: std::result::Result<(), String> = if is_elevated() {
                if currently_installed {
                    boot_task::uninstall().map(|_| ())
                } else {
                    boot_task::install().map(|_| ())
                }
            } else {
                relaunch_elevated(target_arg).and_then(|exit_code| {
                    if exit_code == 0 {
                        Ok(())
                    } else {
                        Err(format!("elevated child exited with code {exit_code}"))
                    }
                })
            };

            match result {
                Ok(()) => {
                    let now = boot_task::is_installed();
                    state.boot_task_item.set_checked(now);
                    if currently_installed && !now {
                        info!("✓ Boot recovery task removed");
                    } else if !currently_installed && now {
                        info!("✓ Boot recovery task installed");
                    } else {
                        warn!("Boot task toggle: state unchanged (elevation declined?)");
                    }
                }
                Err(e) => {
                    warn!("Boot task toggle failed: {e}");
                    // Re-sync the menu to actual state (handles UAC denial cleanly).
                    state.boot_task_item.set_checked(boot_task::is_installed());
                }
            }
            refresh_stale_indicators(state);
        }
        ID_QUIT => {
            // Row 4b: Quit sequence
            info!("Tray: Quit requested");
            handle_quit(state);
            unsafe {
                PostQuitMessage(0);
            }
        }
        _ => {}
    }
}

/// Handle worker result (WM_WORKER_RESULT / WM_APP+3).
/// result handler and Invariant I9 (stale-result immunity).
fn handle_worker_result(state: &mut AppState, result: DisableResult) {
    // Invariant I9: Ignore stale results
    if result.op_id < state.current_generation {
        info!(
            "Worker result op_id={} < current_generation={} — stale, ignoring",
            result.op_id, state.current_generation
        );
        return;
    }

    info!(
        "Worker result op_id={}: disable_ok={}, verify_state={:?}, err={:?}",
        result.op_id, result.disable_ok, result.verify_state, result.err
    );

    match result.verify_state {
        Some(device::KeyboardState::Disabled) => {
            // Success — keyboard is disabled
            info!("Worker result: Disabled (success)");
            update_tooltip(state);
        }
        Some(device::KeyboardState::Enabled) => {
            // Verify mismatch — DISABLE failed or reverted
            error!(
                "Worker result: verify mismatch (Enabled after DISABLE attempt) — recovery ENABLE"
            );
            enable_via_worker(state);
            state.desired_active = false;
            state.current_generation += 1;
            state.active_item.set_checked(false);
            notify_user("Keyboard disable failed — toggled Active off. Restart app to retry.");
            update_tooltip(state);
        }
        None => {
            // Error during verify
            error!("Worker result: verify error: {:?}", result.err);
            enable_via_worker(state);
            update_tooltip(state);
        }
    }
}

/// Handle power event (WM_POWERBROADCAST).
/// and Invariant I10.
fn handle_power_event(state: &mut AppState, event: u32, lparam: LPARAM) {
    match event {
        PBT_APMSUSPEND => {
            // Suspend → inline ENABLE + verify (cannot use worker — suspend is racing us)
            info!("PBT_APMSUSPEND: inline ENABLE + resume_pending");
            state.resume_pending = true;
            state.resume_timestamp = Some(Instant::now());
            inline_enable(state);
        }
        PBT_APMRESUMEAUTOMATIC => {
            // Resume → ENABLE via worker + set resume_pending (Invariant I5)
            info!("PBT_APMRESUMEAUTOMATIC: ENABLE via worker + resume_pending");
            state.resume_pending = true;
            state.resume_timestamp = Some(Instant::now());
            enable_via_worker(state);
            // Do NOT call apply_policy() — would re-disable at lock screen
        }
        PBT_POWERSETTINGCHANGE => {
            // Power setting change (lid close, display off on Modern Standby devices)
            unsafe {
                if lparam.0 == 0 {
                    return;
                }
                let pbs = lparam.0 as *const POWERBROADCAST_SETTING;
                if pbs.is_null() {
                    return;
                }
                let setting_guid = (*pbs).PowerSetting;

                // Lid switch state change
                if setting_guid == GUID_LIDSWITCH_STATE_CHANGE {
                    let data_len = (*pbs).DataLength as usize;
                    if data_len >= 4 {
                        let data_ptr = (*pbs).Data.as_ptr() as *const u32;
                        let lid_state = *data_ptr;
                        if lid_state == 0 {
                            // Lid closed
                            info!("PBT_POWERSETTINGCHANGE: lid closed → inline ENABLE + resume_pending");
                            state.resume_pending = true;
                            state.resume_timestamp = Some(Instant::now());
                            inline_enable(state);
                        }
                    }
                }
                // Console display state change
                else if setting_guid == GUID_CONSOLE_DISPLAY_STATE {
                    let data_len = (*pbs).DataLength as usize;
                    if data_len >= 4 {
                        let data_ptr = (*pbs).Data.as_ptr() as *const u32;
                        let display_state = *data_ptr;
                        if display_state == 0 {
                            // Display off
                            info!("PBT_POWERSETTINGCHANGE: display off → inline ENABLE + resume_pending");
                            state.resume_pending = true;
                            state.resume_timestamp = Some(Instant::now());
                            inline_enable(state);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Handle session event (WM_WTSSESSION_CHANGE).
/// and Invariant I5 (resume gating).
fn handle_session_event(state: &mut AppState, event: u32) {
    match event {
        WTS_SESSION_LOCK => {
            // Lock → inline ENABLE + set resume_pending
            // Master trigger for lock screen — reliable across all lock paths (Win+L, lid, auto-lock)
            info!("WTS_SESSION_LOCK: inline ENABLE + resume_pending");
            state.resume_pending = true;
            state.resume_timestamp = Some(Instant::now());
            inline_enable(state);
        }
        WTS_SESSION_UNLOCK => {
            // Unlock → if resume_pending, clear it and call apply_policy
            if state.resume_pending {
                info!("WTS_SESSION_UNLOCK: clearing resume_pending, calling apply_policy()");
                state.resume_pending = false;
                state.resume_timestamp = None;
                apply_policy(state);
            }
        }
        _ => {}
    }
}

/// Throttle interval for tray-checkmark resync. Independent of
/// SANITY_INTERVAL_MS so worker-death detection stays at 20s while the
/// (slightly more expensive) registry + COM reads only fire every 60s.
const CHECKMARK_RESYNC_INTERVAL: Duration = Duration::from_secs(60);

/// Re-sync tray checkmarks against ground truth (live OS state).
///
/// Defensive against drift between in-memory menu state and reality. The
/// "Active" checkmark is owned by us (desired_active) and is intentionally
/// NOT touched here. Autostart and Lockout protection reflect external
/// state (HKCU Run-key + Task Scheduler) that can change behind our back
/// (e.g., admin removes the task via Task Scheduler GUI, an upgrade
/// re-registers it, etc.). Throttled to once per CHECKMARK_RESYNC_INTERVAL.
fn resync_external_checkmarks(state: &mut AppState) {
    if let Some(last) = state.last_checkmark_resync {
        if last.elapsed() < CHECKMARK_RESYNC_INTERVAL {
            return;
        }
    }
    state.last_checkmark_resync = Some(Instant::now());

    let autostart_live = autostart::is_enabled();
    let boot_task_live = boot_task::is_installed();
    state.autostart_item.set_checked(autostart_live);
    state.boot_task_item.set_checked(boot_task_live);
}

/// Handle sanity timer (WM_TIMER, TIMER_SANITY).
///
fn handle_sanity_timer(state: &mut AppState) {
    // Defensive: re-sync tray checkmarks for externally-owned state
    // (autostart + lockout protection) against live OS state. Internally
    // throttled to CHECKMARK_RESYNC_INTERVAL (60s) so this is a no-op
    // most ticks.
    resync_external_checkmarks(state);

    // Probe worker liveness (Invariant I6)
    if let Some(ref handle) = state.worker_handle {
        if handle.is_finished() {
            warn!(
                "Sanity timer: worker thread has exited (is_finished=true) — setting worker_dead"
            );
            state.worker_dead = true;
            state.desired_active = false;
            state.active_item.set_checked(false);
            update_tooltip(state);
            // Defensive ENABLE inline
            inline_enable(state);
        }
    }

    // Resume timeout check (2 min)
    if state.resume_pending {
        if let Some(ts) = state.resume_timestamp {
            if ts.elapsed() > RESUME_TIMEOUT {
                warn!("Sanity timer: resume_pending timeout (2 min) — clearing");
                state.resume_pending = false;
                state.resume_timestamp = None;
            }
        }
    }

    // Re-evaluate policy if session is active and not in resume limbo
    if !state.resume_pending {
        // Check if session is WTSActive
        let session_active = unsafe {
            let mut info: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut bytes: u32 = 0;
            if WTSQuerySessionInformationW(
                WTS_CURRENT_SERVER_HANDLE,
                WTS_CURRENT_SESSION,
                WTSConnectState,
                &mut info as *mut _ as *mut _,
                &mut bytes,
            )
            .is_ok()
            {
                let state_val = *(info as *const u32);
                WTSFreeMemory(info);
                state_val == 0 // WTSActive
            } else {
                false
            }
        };

        if session_active {
            apply_policy(state);
        }
    }
}

/// Quit handler (Invariant I7).
/// Sequence: set desired_active=false, send Cmd::Shutdown, join worker (500ms timeout),
/// inline ENABLE + verify (retry once on fail), exit 0 regardless.
fn handle_quit(state: &mut AppState) {
    info!("Quit: setting desired_active=false");
    state.desired_active = false;

    // Send Cmd::Shutdown
    info!("Quit: sending Cmd::Shutdown");
    if let Err(e) = state.worker_tx.send(Cmd::Shutdown) {
        warn!(
            "Quit: failed to send Cmd::Shutdown: {} — worker already dead",
            e
        );
    }

    // Join worker with 500ms timeout (bounded join)
    if let Some(handle) = state.worker_handle.take() {
        info!("Quit: joining worker thread (500ms timeout)");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = handle.join();
            tx.send(result).ok();
        });

        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(())) => {
                info!("Quit: worker exited cleanly");
            }
            Ok(Err(_)) => {
                warn!("Quit: worker panicked");
            }
            Err(_) => {
                warn!("Quit: worker join timeout (500ms) — proceeding with inline ENABLE anyway");
            }
        }
    }

    // Shutdown cleanup: re-enable keyboard using cached target
    shutdown_cleanup();

    // Fallback: if no cached target, do full resolve + retry
    if state.cached_target.is_none() {
        info!("Quit: no cached target, falling back to inline ENABLE + verify");
        inline_enable_with_retry(state);
    }
}

/// apply_policy — the decision core ( pseudocode transcribed).
/// All paths fail-safe to ENABLE. No state machine, no cache.
/// Invariants: I3 (fail-closed), I4 (no cache), I6 (worker-dead lockdown), I9 (op_id correlation).
fn apply_policy(state: &mut AppState) {
    // Step 1: if not desired_active
    if !state.desired_active {
        enable_via_worker(state);
        update_tooltip(state);
        return;
    }

    // Step 2: if not nuphy_connected (fresh read, Invariant I4)
    if !state.nuphy_connected() {
        enable_via_worker(state);
        update_tooltip(state);
        return;
    }

    // Step 3: resolve target fresh (Invariant I4, I3)
    let target = match device::resolve() {
        device::ResolveResult::Ok(t) => t,
        other => {
            // Predicate failed (0 or >1 matches) — fail closed (Invariant I3)
            warn!("apply_policy: resolve failed (fail closed): {:?}", other);
            log_resolve_dump(&other);
            enable_via_worker(state);
            update_tooltip(state);
            return;
        }
    };

    // Cache target for shutdown cleanup
    state.cached_target = Some(target.clone());
    if let Ok(mut guard) = SHUTDOWN_TARGET.lock() {
        *guard = Some(target.clone());
    }

    // Step 4: Send Cmd::Disable (atomic disable+verify )
    state.op_id += 1;
    let op_id = state.op_id;
    state.current_generation += 1; // Any send-intent is a generation bump (Invariant I9)

    // Invariant I6: Worker-dead lockdown
    if state.worker_dead {
        warn!("apply_policy: worker_dead=true, refusing DISABLE, routing ENABLE inline");
        inline_enable(state);
        return;
    }

    info!("apply_policy: sending Cmd::Disable (op_id={})", op_id);
    if let Err(e) = state.worker_tx.send(Cmd::Disable { target, op_id }) {
        error!("apply_policy: worker send failed: {} — setting worker_dead=true, forcing ENABLE inline", e);
        state.worker_dead = true;
        state.desired_active = false;
        state.active_item.set_checked(false);
        inline_enable(state);
        notify_user("Worker crashed — Active toggled off.");
    }
}

/// Send Cmd::Enable to worker (fire-and-forget, no verify on normal ENABLE path ).
/// If worker_dead, route inline instead (Invariant I6).
fn enable_via_worker(state: &mut AppState) {
    if state.worker_dead {
        inline_enable(state);
        return;
    }

    state.op_id += 1;
    let op_id = state.op_id;
    info!("enable_via_worker: sending Cmd::Enable (op_id={})", op_id);
    if let Err(e) = state.worker_tx.send(Cmd::Enable { op_id }) {
        error!(
            "enable_via_worker: worker send failed: {} — setting worker_dead=true, routing inline",
            e
        );
        state.worker_dead = true;
        inline_enable(state);
    }
}

/// Inline ENABLE (bypass worker). Used for recovery paths (Quit, suspend, worker-death).
/// Per Invariant I6, I7, I10.
fn inline_enable(state: &AppState) {
    // Skip when not elevated — can't enable anyway
    if !state.is_elevated {
        return;
    }

    let target = match device::resolve() {
        device::ResolveResult::Ok(t) => t,
        other => {
            error!("inline_enable: resolve failed: {:?}", other);
            log_resolve_dump(&other);
            return;
        }
    };

    info!("inline_enable: calling device::enable()");
    if let Err(e) = device::enable(&target) {
        error!("inline_enable: ENABLE failed: {}", e);
    }

    // Verify (Invariant I2)
    match device::current_state(&target) {
        Ok(device::KeyboardState::Enabled) => {
            info!("inline_enable: verify Enabled (success)");
        }
        Ok(device::KeyboardState::Disabled) => {
            error!("inline_enable: verify still Disabled after ENABLE");
        }
        Err(e) => {
            error!("inline_enable: verify error: {}", e);
        }
    }
}

/// Inline ENABLE with retry (Quit path per Invariant I7).
fn inline_enable_with_retry(state: &AppState) {
    // Skip when not elevated — can't enable anyway
    if !state.is_elevated {
        info!("Shutdown: skipping ENABLE (not elevated)");
        return;
    }

    let target = match device::resolve() {
        device::ResolveResult::Ok(t) => t,
        other => {
            error!("inline_enable_with_retry: resolve failed: {:?}", other);
            log_resolve_dump(&other);
            return;
        }
    };

    if let Err(e) = device::enable(&target) {
        warn!(
            "inline_enable_with_retry: first ENABLE failed: {} — retrying once",
            e
        );
        std::thread::sleep(Duration::from_millis(500));
        if let Err(e2) = device::enable(&target) {
            error!("inline_enable_with_retry: second ENABLE failed: {}", e2);
            return;
        }
    }

    // Verify with retry
    match device::current_state(&target) {
        Ok(device::KeyboardState::Enabled) => {
            info!("inline_enable_with_retry: verify Enabled (success)");
        }
        Ok(device::KeyboardState::Disabled) => {
            error!("inline_enable_with_retry: verify still Disabled — retrying ENABLE once");
            std::thread::sleep(Duration::from_millis(500));
            device::enable(&target).ok();
            match device::current_state(&target) {
                Ok(device::KeyboardState::Enabled) => {
                    info!("inline_enable_with_retry: retry verify Enabled (success)");
                }
                _ => {
                    error!("inline_enable_with_retry: retry verify failed");
                }
            }
        }
        Err(e) => {
            error!("inline_enable_with_retry: verify error: {}", e);
        }
    }
}

/// Update tray tooltip (Invariant I12).
fn update_tooltip(state: &AppState) {
    let active = if state.desired_active { "On" } else { "Off" };
    let nuphy = if state.nuphy_connected() {
        "Connected"
    } else {
        "Disconnected"
    };

    let tooltip = if state.initial_policy_pending {
        format!("SwitchBoard | Active: {active} | Nuphy: {nuphy} — Arming in 3s…")
    } else {
        format!("SwitchBoard | Active: {active} | Nuphy: {nuphy}")
    };

    state._tray_icon.set_tooltip(Some(&tooltip)).ok();
}

/// Show balloon notification (best-effort, log on failure).
fn notify_user(message: &str) {
    info!("Balloon: {}", message);
    // tray-icon 0.21.3 doesn't expose balloon API directly — would need winapi or windows-rs call
    // For v0.1, balloon is logged; future enhancement can use Shell_NotifyIconW with NIF_INFO
}

/// Log resolve dump on predicate failure (Invariant I3 diagnostic).
fn log_resolve_dump(result: &device::ResolveResult) {
    match result {
        device::ResolveResult::NoMatch { dump } => {
            error!("Resolve: NoMatch (0 devices matched predicate)");
            for line in dump.lines() {
                error!("  {}", line);
            }
        }
        device::ResolveResult::MultipleMatches { candidates, dump } => {
            error!(
                "Resolve: MultipleMatches ({} devices matched predicate)",
                candidates.len()
            );
            for line in dump.lines() {
                error!("  {}", line);
            }
        }
        device::ResolveResult::EnumerationError(e) => {
            error!("Resolve: EnumerationError: {}", e);
        }
        _ => {}
    }
}

/// Release all stuck modifier keys by injecting synthetic KEYUP events.
///
/// **Context:** When `device::disable()` disconnects the internal keyboard from the HID stack,
/// any modifier keys held at that instant get "stuck" in the OS's async-key-state table —
/// the OS never sees the KEYUP because the device disappeared mid-press. This causes phantom
/// modifiers on subsequent typing (e.g., capitals, hotkeys firing).
///
/// **Fix:** After every successful `device::disable()`, send synthetic KEYUP events for all
/// eight standard modifiers: LShift, RShift, LCtrl, RCtrl, LAlt, RAlt, LWin, RWin.
///
/// **Sentinel:** Each synthetic event is tagged with `SWITCHBOARD_SYNTHETIC_SENTINEL` in
/// `dwExtraInfo` so any future WH_KEYBOARD_LL hook can identify and pass through our own
/// injections (avoid recursive blocking).
///
/// **Infallibility:** This function MUST NOT panic. If `SendInput` fails, the keyboard is
/// already disabled and the user is stuck — log the error and continue. Synthetic KEYUP
/// for a key that is not held is a harmless no-op at the OS level.
///
/// **Invocation:** Called immediately after `device::disable_and_verify` returns success
/// in the worker thread (Cmd::Disable handler).
fn release_stuck_modifiers() {
    const MODS: [VIRTUAL_KEY; 8] = [
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ];

    let mut inputs: [INPUT; 8] = unsafe { std::mem::zeroed() };

    for (i, vk) in MODS.iter().enumerate() {
        inputs[i].r#type = INPUT_KEYBOARD;
        inputs[i].Anonymous.ki = KEYBDINPUT {
            wVk: *vk,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
            time: 0,
            dwExtraInfo: SWITCHBOARD_SYNTHETIC_SENTINEL,
        };
    }

    unsafe {
        match SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) {
            0 => {
                // SendInput failed — log but do NOT panic (keyboard is already disabled)
                let err = windows::Win32::Foundation::GetLastError();
                warn!(
                    "release_stuck_modifiers: SendInput failed (GetLastError=0x{:08X})",
                    err.0
                );
            }
            count => {
                info!(
                    "release_stuck_modifiers: injected {} KEYUP events (sentinel: 0x{:X})",
                    count, SWITCHBOARD_SYNTHETIC_SENTINEL
                );
            }
        }
    }
}

/// Worker thread main loop (SetupAPI operations).
/// Receives commands via mpsc, performs SetupAPI calls, posts results back to main thread.
/// threading model.
fn worker_thread_main(rx: mpsc::Receiver<Cmd>) {
    info!("Worker thread started");
    loop {
        match rx.recv() {
            Ok(cmd) => match cmd {
                Cmd::Enable { op_id } => {
                    info!("Worker: Cmd::Enable (op_id={})", op_id);
                    let target = match device::resolve() {
                        device::ResolveResult::Ok(t) => t,
                        other => {
                            warn!("Worker: resolve failed for Enable: {:?}", other);
                            continue;
                        }
                    };
                    if let Err(e) = device::enable(&target) {
                        warn!("Worker: ENABLE failed: {}", e);
                    }
                    // Fire-and-forget (no result posted for Enable)
                }
                Cmd::Disable { target, op_id } => {
                    // Atomic disable+verify
                    info!("Worker: Cmd::Disable (op_id={})", op_id);
                    let (disable_res, state_res) = device::disable_and_verify(&target);
                    let (disable_ok, verify_state, err) = match (&disable_res, &state_res) {
                        (Ok(()), Ok(state)) => (true, Some(*state), None),
                        (Err(e), _) => (false, None, Some(e.to_string())),
                        (Ok(()), Err(e)) => (true, None, Some(format!("verify failed: {}", e))),
                    };

                    // If disable succeeded and verified Disabled, flush stuck modifiers
                    if disable_ok && verify_state == Some(device::KeyboardState::Disabled) {
                        release_stuck_modifiers();
                    }

                    // Post result to main thread (leak Box as LPARAM, main thread frees it)
                    let result = Box::new(DisableResult {
                        op_id,
                        disable_ok,
                        verify_state,
                        err,
                    });
                    let hwnd = unsafe { FindWindowW(w!("switchboard_msg_window"), None) }
                        .unwrap_or(HWND(std::ptr::null_mut()));
                    unsafe {
                        PostMessageW(
                            hwnd,
                            WM_WORKER_RESULT,
                            WPARAM(0),
                            LPARAM(Box::into_raw(result) as isize),
                        )
                        .ok();
                    }
                }
                Cmd::Shutdown => {
                    info!("Worker: Cmd::Shutdown received — exiting");
                    break;
                }
            },
            Err(_) => {
                // Channel closed (sender dropped) — main thread exited
                info!("Worker: channel closed — exiting");
                break;
            }
        }
    }
    info!("Worker thread exiting");
}
// =============================================================================
// Admin subcommands: --install-boot-task / --uninstall-boot-task
// =============================================================================

#[derive(Clone, Copy)]
enum AdminSubcommand {
    InstallBootTask,
    UninstallBootTask,
}

impl AdminSubcommand {
    fn arg(self) -> &'static str {
        match self {
            AdminSubcommand::InstallBootTask => "--install-boot-task",
            AdminSubcommand::UninstallBootTask => "--uninstall-boot-task",
        }
    }
}

fn parse_admin_subcommand() -> Option<AdminSubcommand> {
    for a in env::args().skip(1) {
        match a.as_str() {
            "--install-boot-task" => return Some(AdminSubcommand::InstallBootTask),
            "--uninstall-boot-task" => return Some(AdminSubcommand::UninstallBootTask),
            _ => {}
        }
    }
    None
}

/// Run an admin subcommand. Self-elevates via ShellExecuteW("runas") if
/// the current process isn't elevated, then exits the original (non-elevated)
/// invocation. The elevated child re-enters this function with `is_elevated()`
/// returning true and performs the actual work.
fn run_admin_subcommand(sub: AdminSubcommand) -> ! {
    println!("switchboard: {}", sub.arg());

    if !is_elevated() {
        println!("Requesting elevation via UAC...");
        match relaunch_elevated(sub.arg()) {
            Ok(code) => std::process::exit(code as i32),
            Err(e) => {
                eprintln!("Elevation failed: {e}");
                pause_for_keypress();
                std::process::exit(1);
            }
        }
    }

    // We are elevated — do the work.
    let exit_code = match sub {
        AdminSubcommand::InstallBootTask => match boot_task::install() {
            Ok(path) => {
                println!();
                println!("Task name:   {}", boot_task::TASK_NAME);
                println!("Trigger:     At system startup (boot)");
                println!("Principal:   NT AUTHORITY\\SYSTEM (Highest)");
                println!("Action:      \"{}\" --recover", path.display());
                println!();
                println!("[OK] Task registered successfully");
                0
            }
            Err(e) => {
                eprintln!();
                eprintln!("[FAIL] {e}");
                1
            }
        },
        AdminSubcommand::UninstallBootTask => match boot_task::uninstall() {
            Ok(true) => {
                println!();
                println!("[OK] Task '{}' removed", boot_task::TASK_NAME);
                0
            }
            Ok(false) => {
                println!();
                println!(
                    "Task '{}' not registered (nothing to remove)",
                    boot_task::TASK_NAME
                );
                0
            }
            Err(e) => {
                eprintln!();
                eprintln!("[FAIL] {e}");
                1
            }
        },
    };

    pause_for_keypress();
    std::process::exit(exit_code);
}

fn pause_for_keypress() {
    use std::io::{Read, Write};
    print!("\nPress any key to exit...");
    let _ = std::io::stdout().flush();
    let mut buf = [0u8; 1];
    let _ = std::io::stdin().read(&mut buf);
}

// =============================================================================
// Elevation: detect whether current process has admin token; relaunch self
// elevated via ShellExecuteW("runas") and wait for the child.
// =============================================================================

fn is_elevated() -> bool {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Relaunch the current executable with one extra argument under the
/// "runas" verb (UAC consent prompt). Blocks until the child exits and
/// returns its exit code.
fn relaunch_elevated(arg: &str) -> std::result::Result<u32, String> {
    use std::iter;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(iter::once(0)).collect();
    let verb_w: Vec<u16> = "runas".encode_utf16().chain(iter::once(0)).collect();
    let args_w: Vec<u16> = arg.encode_utf16().chain(iter::once(0)).collect();

    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb_w.as_ptr()),
            lpFile: PCWSTR(exe_w.as_ptr()),
            lpParameters: PCWSTR(args_w.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info)
            .map_err(|e| format!("ShellExecuteExW failed: {e} (user may have declined UAC)"))?;
        if info.hProcess.is_invalid() {
            return Err("ShellExecuteExW returned no process handle".to_string());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 1;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        Ok(code)
    }
}

// =============================================================================
// Stale-path detection for autostart + boot recovery task
// =============================================================================

fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    let canon = |p: &std::path::Path| {
        p.canonicalize()
            .ok()
            .and_then(|c| c.to_str().map(|s| s.to_lowercase()))
    };
    match (canon(a), canon(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase(),
    }
}

/// Re-evaluate whether the autostart Run-key value or the boot task points
/// at the currently running switchboard.exe. Updates menu item labels (adding
/// or removing a trailing " ⚠") and pops one combined warning dialog if
/// any are stale.
fn refresh_stale_indicators(state: &mut AppState) {
    let current = match env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    let autostart_stale = autostart::registered_path()
        .map(|p| !paths_equal(&p, &current))
        .unwrap_or(false);

    let boot_task_stale = boot_task::registered_path()
        .map(|p| !paths_equal(&p, &current))
        .unwrap_or(false);

    state.autostart_item.set_text(if autostart_stale {
        "Auto-start SwitchBoard at login ⚠"
    } else {
        "Auto-start SwitchBoard at login"
    });
    state.boot_task_item.set_text(if boot_task_stale {
        "Lockout protection (recommended) ⚠"
    } else {
        "Lockout protection (recommended)"
    });

    if autostart_stale {
        warn!(
            "Autostart path stale: registered={:?} current={:?}",
            autostart::registered_path(),
            current
        );
    }
    if boot_task_stale {
        warn!(
            "Boot task path stale: registered={:?} current={:?}",
            boot_task::registered_path(),
            current
        );
    }

    if autostart_stale || boot_task_stale {
        let mut lines: Vec<String> = Vec::new();
        if autostart_stale {
            lines.push(
                "• \"Auto-start SwitchBoard at login\" points to an old SwitchBoard location."
                    .to_string(),
            );
        }
        if boot_task_stale {
            lines.push(
                "• \"Lockout protection (recommended)\" points to an old SwitchBoard location."
                    .to_string(),
            );
        }
        let body = format!(
            "{}\n\nRight-click the tray icon, then uncheck and re-check the marked item(s) to refresh the stored path to:\n{}",
            lines.join("\n"),
            current.display()
        );
        show_warning_dialog("SwitchBoard — stale autostart path", &body);
    }
}

/// Pop a non-blocking modal warning dialog on a background thread.
/// (tray-icon 0.21 has no balloon API; Shell_NotifyIconW with NIF_INFO
/// would require maintaining a second tray icon. A modal dialog is more
/// visible for stale-path conditions that need user action anyway.)
fn show_warning_dialog(title: &str, body: &str) {
    use std::iter;
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        let title_w: Vec<u16> = title.encode_utf16().chain(iter::once(0)).collect();
        let body_w: Vec<u16> = body.encode_utf16().chain(iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                HWND(std::ptr::null_mut()),
                PCWSTR(body_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
            );
        }
    });
}

#[cfg(test)]
mod paths_equal_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_paths_equal_same() {
        assert!(paths_equal(
            Path::new(r"C:\foo\bar"),
            Path::new(r"C:\foo\bar")
        ));
    }

    #[test]
    fn test_paths_equal_case_insensitive() {
        assert!(paths_equal(
            Path::new(r"C:\Foo\Bar"),
            Path::new(r"c:\foo\bar")
        ));
    }

    #[test]
    fn test_paths_equal_different() {
        assert!(!paths_equal(Path::new(r"C:\foo"), Path::new(r"C:\bar")));
    }
}
