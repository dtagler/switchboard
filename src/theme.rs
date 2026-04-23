//! Windows taskbar theme detection.
//!
//! Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\
//! SystemUsesLightTheme` (DWORD). 1 = light taskbar, 0 = dark, missing = dark
//! (Win11 default). The tray icon is swapped on `WM_SETTINGCHANGE` with
//! lParam == "ImmersiveColorSet" — see `is_immersive_color_set()` below.

use windows::core::*;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    REG_VALUE_TYPE,
};

const THEME_SUBKEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const VALUE_NAME: PCWSTR = w!("SystemUsesLightTheme");

/// Returns true if the user has selected the light taskbar theme.
/// Defaults to false (dark) on any error or missing value.
pub fn system_uses_light_theme() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, THEME_SUBKEY, 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return false;
        }

        let mut data: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let mut kind = REG_VALUE_TYPE(0);
        let status = RegQueryValueExW(
            hkey,
            VALUE_NAME,
            None,
            Some(&mut kind),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);

        status == ERROR_SUCCESS && kind == REG_DWORD && data == 1
    }
}

/// Test whether a `WM_SETTINGCHANGE` lParam (a wide C string pointer) names
/// the ImmersiveColorSet event — fired when the user toggles light/dark.
///
/// Safe to call with a null pointer; returns false in that case.
pub unsafe fn is_immersive_color_set(lparam_ptr: isize) -> bool {
    if lparam_ptr == 0 {
        return false;
    }
    let target: &[u16] = &[
        b'I' as u16,
        b'm' as u16,
        b'm' as u16,
        b'e' as u16,
        b'r' as u16,
        b's' as u16,
        b'i' as u16,
        b'v' as u16,
        b'e' as u16,
        b'C' as u16,
        b'o' as u16,
        b'l' as u16,
        b'o' as u16,
        b'r' as u16,
        b'S' as u16,
        b'e' as u16,
        b't' as u16,
        0,
    ];
    let p = lparam_ptr as *const u16;
    for (i, &expected) in target.iter().enumerate() {
        let c = *p.add(i);
        if c != expected {
            return false;
        }
        if expected == 0 {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a NUL-terminated UTF-16 buffer from an ASCII str (test fixture).
    fn utf16z(s: &str) -> Vec<u16> {
        let mut v: Vec<u16> = s.encode_utf16().collect();
        v.push(0);
        v
    }

    #[test]
    fn test_is_immersive_color_set_null_pointer() {
        // SAFETY: function is documented safe with a null pointer.
        unsafe {
            assert!(!is_immersive_color_set(0));
        }
    }

    #[test]
    fn test_is_immersive_color_set_exact_match() {
        let buf = utf16z("ImmersiveColorSet");
        // SAFETY: buf outlives the call; pointer is valid + NUL-terminated.
        unsafe {
            assert!(is_immersive_color_set(buf.as_ptr() as isize));
        }
    }

    #[test]
    fn test_is_immersive_color_set_different_event() {
        // Real WM_SETTINGCHANGE often carries unrelated names (e.g. "Environment").
        let buf = utf16z("Environment");
        unsafe {
            assert!(!is_immersive_color_set(buf.as_ptr() as isize));
        }
    }

    #[test]
    fn test_is_immersive_color_set_empty_string() {
        // Empty NUL-terminated string ≠ "ImmersiveColorSet".
        let buf = utf16z("");
        unsafe {
            assert!(!is_immersive_color_set(buf.as_ptr() as isize));
        }
    }

    #[test]
    fn test_is_immersive_color_set_prefix_only() {
        // Truncated name should NOT match — the trailing NUL of the input is
        // hit before all 17 chars of "ImmersiveColorSet" are consumed.
        let buf = utf16z("Immersive");
        unsafe {
            assert!(!is_immersive_color_set(buf.as_ptr() as isize));
        }
    }

    #[test]
    fn test_is_immersive_color_set_case_sensitive() {
        // WM_SETTINGCHANGE names are case-sensitive per Microsoft docs;
        // lock that behavior in so a future "be lenient" change is intentional.
        let buf = utf16z("immersivecolorset");
        unsafe {
            assert!(!is_immersive_color_set(buf.as_ptr() as isize));
        }
    }

    #[test]
    fn test_is_immersive_color_set_longer_than_target() {
        // A name that starts with "ImmersiveColorSet" but has trailing chars
        // is NOT a match — the comparison requires NUL right after the target.
        let buf = utf16z("ImmersiveColorSetX");
        unsafe {
            assert!(!is_immersive_color_set(buf.as_ptr() as isize));
        }
    }
}
