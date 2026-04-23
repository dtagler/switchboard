//! Per-user autostart via Task Scheduler logon-trigger task with silent elevation.
//!
//! Uses a scheduled task with LogonTrigger (scoped to current user) and
//! RunLevel=HighestAvailable on the Principal. This pre-elevates the token at
//! logon, avoiding UAC prompts despite requireAdministrator in the manifest.
//! Task runs as the interactive user (not SYSTEM) so tray/UI work correctly.

use std::env;
use std::path::PathBuf;

use windows::core::{BSTR, GUID, HRESULT, VARIANT};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
};

pub const TASK_NAME: &str = "switchboard-logon";
const TASK_DESCRIPTION: &str =
    "switchboard autostart: launches switchboard at user logon with silent elevation";

const CLSID_TASK_SCHEDULER: GUID = GUID::from_u128(0x0F87369F_A4E5_4CFC_BD3E_73E6154572DD);
const HRESULT_FILE_NOT_FOUND: HRESULT = HRESULT(0x80070002u32 as i32);

// Legacy HKCU Run-key constants for migration cleanup
const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "switchboard";

/// Returns true if the logon task is registered and enabled.
pub fn is_enabled() -> bool {
    with_folder(|folder| unsafe {
        match folder.GetTask(&BSTR::from(TASK_NAME)) {
            Ok(task) => {
                let task: IRegisteredTask = task;
                match task.Enabled() {
                    Ok(enabled) => Ok(enabled.as_bool()),
                    Err(_) => Ok(true), // If can't read state, assume enabled
                }
            }
            Err(e) if e.code() == HRESULT_FILE_NOT_FOUND => Ok(false),
            Err(_) => Ok(false),
        }
    })
    .unwrap_or(false)
}

/// Return the executable path the logon task is configured to launch,
/// or `None` if the task isn't registered or its XML can't be parsed.
pub fn registered_path() -> Option<PathBuf> {
    let xml: String = with_folder(|folder| unsafe {
        match folder.GetTask(&BSTR::from(TASK_NAME)) {
            Ok(task) => {
                let task: IRegisteredTask = task;
                let bstr = task.Xml()?;
                Ok(bstr.to_string())
            }
            Err(e) => Err(e),
        }
    })
    .ok()?;

    extract_command(&xml).map(PathBuf::from)
}

/// Register (or replace) the logon task pointing at the currently running exe.
/// Also cleans up legacy HKCU Run-key value from prior installs.
pub fn enable() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| format!("current_exe failed: {e}"))?;
    let abs = exe
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {e}"))?;
    let abs_str = strip_verbatim(abs.to_string_lossy().as_ref());

    // Get current username for LogonTrigger UserId
    let username = get_current_username()?;
    let xml = build_logon_task_xml(&abs_str, &username);

    with_folder(|folder| unsafe {
        folder
            .RegisterTask(
                &BSTR::from(TASK_NAME),
                &BSTR::from(xml.as_str()),
                TASK_CREATE_OR_UPDATE.0,
                &VARIANT::default(),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::default(),
            )
            .map(|_| ())
    })
    .map_err(|e| format!("Task Scheduler error: {e}"))?;

    // Migration: clean up old HKCU Run-key value if it exists
    let _ = delete_legacy_run_key();

    Ok(())
}

/// Delete the logon task. Returns idempotently (success if task not present).
/// Also cleans up legacy HKCU Run-key value from prior installs.
pub fn disable() -> Result<(), String> {
    with_folder(|folder| unsafe {
        match folder.DeleteTask(&BSTR::from(TASK_NAME), 0) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == HRESULT_FILE_NOT_FOUND => Ok(()),
            Err(e) => Err(e),
        }
    })
    .map_err(|e| format!("Task Scheduler error: {e}"))?;

    // Migration: clean up old HKCU Run-key value if it exists
    let _ = delete_legacy_run_key();

    Ok(())
}

// --- helpers ---

fn with_folder<R>(
    f: impl FnOnce(&ITaskFolder) -> windows::core::Result<R>,
) -> windows::core::Result<R> {
    unsafe {
        // CoInitializeEx HRESULTs:
        //   S_OK           (0x00000000) — we initialised; must CoUninitialize on exit
        //   S_FALSE        (0x00000001) — already initialised in the same apartment;
        //                                 we still owe a CoUninitialize call to keep
        //                                 the per-thread refcount balanced
        //   RPC_E_CHANGED_MODE (0x80010106) — already init'd in a different apartment;
        //                                     do NOT call CoUninitialize
        //   other          — genuine failure
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let hr_code = hr.0;
        let needs_uninit = if hr_code == 0 || hr_code == 1 {
            true
        } else if hr_code == 0x80010106u32 as i32 {
            false
        } else {
            return Err(windows::core::Error::from(hr));
        };

        let result = (|| -> windows::core::Result<R> {
            let service: ITaskService =
                CoCreateInstance(&CLSID_TASK_SCHEDULER, None, CLSCTX_INPROC_SERVER)?;
            service.Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )?;
            let folder: ITaskFolder = service.GetFolder(&BSTR::from("\\"))?;
            f(&folder)
        })();

        if needs_uninit {
            CoUninitialize();
        }
        result
    }
}

fn strip_verbatim(s: &str) -> String {
    s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn get_current_username() -> Result<String, String> {
    use windows::core::PWSTR;
    use windows::Win32::System::WindowsProgramming::GetUserNameW;

    unsafe {
        let mut size: u32 = 256;
        let mut buf = vec![0u16; size as usize];
        match GetUserNameW(PWSTR(buf.as_mut_ptr()), &mut size) {
            Ok(()) => {
                let trimmed: Vec<u16> = buf.iter().take_while(|c| **c != 0).copied().collect();
                Ok(String::from_utf16_lossy(&trimmed))
            }
            Err(_) => Err("GetUserNameW failed".to_string()),
        }
    }
}

fn build_logon_task_xml(switchboard_path: &str, username: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>{desc}</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{path}</Command>
    </Exec>
  </Actions>
</Task>"#,
        desc = xml_escape(TASK_DESCRIPTION),
        user = xml_escape(username),
        path = xml_escape(switchboard_path)
    )
}

/// Extract the `<Command>...</Command>` text from a task XML document.
/// Returns the unescaped command string, or `None` if not found.
fn extract_command(xml: &str) -> Option<String> {
    let start_tag = "<Command>";
    let end_tag = "</Command>";
    let start = xml.find(start_tag)? + start_tag.len();
    let end = xml[start..].find(end_tag)? + start;
    let raw = &xml[start..end];
    Some(
        raw.replace("&quot;", "\"")
            .replace("&gt;", ">")
            .replace("&lt;", "<")
            .replace("&amp;", "&"),
    )
}

/// Delete the legacy HKCU Run-key value from pre-v0.1.2 installs.
/// Returns Ok(()) if deleted or not present; logs but ignores errors.
fn delete_legacy_run_key() -> Result<(), String> {
    use std::iter;
    use windows::core::PCWSTR;

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(iter::once(0)).collect()
    }

    let subkey = to_wide(RUN_SUBKEY);
    let value = to_wide(VALUE_NAME);

    unsafe {
        let mut hkey = HKEY::default();
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            return Ok(()); // Key doesn't exist or can't open - fine
        }

        let del = RegDeleteValueW(hkey, PCWSTR(value.as_ptr()));
        let _ = RegCloseKey(hkey);

        if del == ERROR_SUCCESS || del == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            // Log but don't fail - this is a migration cleanup
            log::warn!(
                "Could not delete legacy HKCU Run-key value: 0x{:08X}",
                del.0
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── xml_escape ──────────────────────────────────────────────────────────
    #[test]
    fn test_xml_escape_basic() {
        assert_eq!(xml_escape("&"), "&amp;");
        assert_eq!(xml_escape("<"), "&lt;");
        assert_eq!(xml_escape(">"), "&gt;");
        assert_eq!(xml_escape("\""), "&quot;");
    }

    #[test]
    fn test_xml_escape_passthrough() {
        assert_eq!(
            xml_escape("plain ascii string 123"),
            "plain ascii string 123"
        );
    }

    #[test]
    fn test_xml_escape_combined() {
        let escaped = xml_escape("<\"foo\" & bar>");
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('>'));
        assert!(!escaped.contains('"'));
        // & only allowed as part of an entity reference
        for (i, _) in escaped.match_indices('&') {
            let tail = &escaped[i..];
            assert!(
                tail.starts_with("&amp;")
                    || tail.starts_with("&lt;")
                    || tail.starts_with("&gt;")
                    || tail.starts_with("&quot;"),
                "unescaped & at position {} in {}",
                i,
                escaped
            );
        }
    }

    // ── strip_verbatim ──────────────────────────────────────────────────────
    #[test]
    fn test_strip_verbatim_present() {
        assert_eq!(strip_verbatim(r"\\?\C:\foo"), r"C:\foo");
    }

    #[test]
    fn test_strip_verbatim_absent() {
        assert_eq!(strip_verbatim(r"C:\foo"), r"C:\foo");
    }

    // ── extract_command ─────────────────────────────────────────────────────
    #[test]
    fn test_extract_command_valid() {
        let xml = r"<Task><Command>C:\path\switchboard.exe</Command></Task>";
        assert_eq!(
            extract_command(xml),
            Some(r"C:\path\switchboard.exe".to_string())
        );
    }

    #[test]
    fn test_extract_command_escaped() {
        let xml = r"<Task><Command>C:\path&amp;X\switchboard.exe</Command></Task>";
        // extract_command unescapes XML entities, so &amp; becomes &
        let got = extract_command(xml).expect("should extract");
        assert_eq!(got, r"C:\path&X\switchboard.exe");
    }

    #[test]
    fn test_extract_command_missing() {
        let xml = "<Task><NoCommand/></Task>";
        assert_eq!(extract_command(xml), None);
    }

    #[test]
    fn test_extract_command_empty() {
        let xml = "<Task><Command></Command></Task>";
        assert_eq!(extract_command(xml), Some(String::new()));
    }

    // ── build_logon_task_xml ────────────────────────────────────────────────
    #[test]
    fn test_logon_task_xml_structure() {
        let xml = build_logon_task_xml(r"C:\switchboard.exe", "testuser");
        assert!(xml.contains("<LogonTrigger"), "missing LogonTrigger");
        assert!(xml.contains("<UserId"), "missing UserId");
        assert!(
            xml.contains("HighestAvailable"),
            "missing HighestAvailable RunLevel"
        );
        assert!(xml.contains("<Command>"), "missing Command");
    }

    #[test]
    fn test_logon_task_xml_roundtrip() {
        let path = r"C:\my path\switchboard.exe";
        let xml = build_logon_task_xml(path, "testuser");
        let extracted = extract_command(&xml);
        assert!(
            extracted.is_some(),
            "extract_command returned None for generated XML"
        );
        assert_eq!(extracted.unwrap(), path);
    }

    // ── snapshot test (insta) ──────────────────────────────────────────────
    #[test]
    fn test_logon_task_xml_snapshot() {
        let xml = build_logon_task_xml(r"C:\switchboard.exe", "testuser");
        insta::assert_snapshot!(xml);
    }
}
