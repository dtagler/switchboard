//! Boot-recovery scheduled task management.
//!
//! Wraps Task Scheduler 2.0 COM API to install/uninstall a per-machine
//! task that runs `switchboard.exe --recover` at every system startup as
//! NT AUTHORITY\SYSTEM. Requires admin to call install/uninstall;
//! `is_installed` and `registered_path` are read-only and work without
//! elevation.

use std::env;
use std::path::PathBuf;

use windows::core::{BSTR, GUID, HRESULT, VARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_SERVICE_ACCOUNT,
};

pub const TASK_NAME: &str = "switchboard-boot-recover";
const TASK_DESCRIPTION: &str = "switchboard boot recovery: ensures internal keyboard is enabled at startup (runs before user logon)";

const CLSID_TASK_SCHEDULER: GUID = GUID::from_u128(0x0F87369F_A4E5_4CFC_BD3E_73E6154572DD);
const HRESULT_FILE_NOT_FOUND: HRESULT = HRESULT(0x80070002u32 as i32);

/// Register (or replace) the boot-recovery task pointing at the currently
/// running switchboard.exe. Caller must be elevated.
pub fn install() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("current_exe failed: {e}"))?;
    let abs = exe
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {e}"))?;
    let abs_str = strip_verbatim(abs.to_string_lossy().as_ref());
    let xml = build_task_xml(&abs_str);

    with_folder(|folder| unsafe {
        folder
            .RegisterTask(
                &BSTR::from(TASK_NAME),
                &BSTR::from(xml.as_str()),
                TASK_CREATE_OR_UPDATE.0,
                &VARIANT::default(),
                &VARIANT::default(),
                TASK_LOGON_SERVICE_ACCOUNT,
                &VARIANT::default(),
            )
            .map(|_| ())
    })
    .map_err(|e| format!("Task Scheduler error: {e}"))?;

    Ok(PathBuf::from(abs_str))
}

/// Delete the boot-recovery task. Returns `true` if a task was removed,
/// `false` if no task was registered (idempotent). Caller must be elevated
/// (or the process must own the task).
pub fn uninstall() -> Result<bool, String> {
    with_folder(|folder| unsafe {
        match folder.DeleteTask(&BSTR::from(TASK_NAME), 0) {
            Ok(()) => Ok(true),
            Err(e) if e.code() == HRESULT_FILE_NOT_FOUND => Ok(false),
            Err(e) => Err(e),
        }
    })
    .map_err(|e| format!("Task Scheduler error: {e}"))
}

/// Read-only check: is the task currently registered?
pub fn is_installed() -> bool {
    with_folder(|folder| unsafe {
        match folder.GetTask(&BSTR::from(TASK_NAME)) {
            Ok(_) => Ok(true),
            Err(e) if e.code() == HRESULT_FILE_NOT_FOUND => Ok(false),
            Err(e) => Err(e),
        }
    })
    .unwrap_or(false)
}

/// Return the executable path the task is currently configured to launch,
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

fn build_task_xml(switchboard_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>{desc}</Description>
  </RegistrationInfo>
  <Triggers>
    <BootTrigger>
      <Enabled>true</Enabled>
    </BootTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>S-1-5-18</UserId>
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
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT2M</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{path}</Command>
      <Arguments>--recover</Arguments>
    </Exec>
  </Actions>
</Task>"#,
        desc = xml_escape(TASK_DESCRIPTION),
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- xml_escape tests (3) ---

    #[test]
    fn test_xml_escape_basic() {
        let input = r#"C:\Program Files\app.exe"#;
        let output = xml_escape(input);
        assert_eq!(output, r#"C:\Program Files\app.exe"#);
    }

    #[test]
    fn test_xml_escape_passthrough() {
        let input = "plain text with no special chars";
        let output = xml_escape(input);
        assert_eq!(output, "plain text with no special chars");
    }

    #[test]
    fn test_xml_escape_combined() {
        let input = r#"<foo & "bar" > baz>"#;
        let output = xml_escape(input);
        assert_eq!(output, "&lt;foo &amp; &quot;bar&quot; &gt; baz&gt;");
    }

    // --- strip_verbatim tests (2) ---

    #[test]
    fn test_strip_verbatim_present() {
        let input = r"\\?\C:\Users\test\app.exe";
        let output = strip_verbatim(input);
        assert_eq!(output, r"C:\Users\test\app.exe");
    }

    #[test]
    fn test_strip_verbatim_absent() {
        let input = r"C:\Users\test\app.exe";
        let output = strip_verbatim(input);
        assert_eq!(output, r"C:\Users\test\app.exe");
    }

    // --- extract_command tests (4) ---

    #[test]
    fn test_extract_command_valid() {
        let xml = r#"<Task><Actions><Exec><Command>C:\app.exe</Command></Exec></Actions></Task>"#;
        let cmd = extract_command(xml);
        assert_eq!(cmd, Some("C:\\app.exe".to_string()));
    }

    #[test]
    fn test_extract_command_escaped() {
        let xml = r#"<Command>&lt;C:\app &amp; tool&gt;</Command>"#;
        let cmd = extract_command(xml);
        assert_eq!(cmd, Some("<C:\\app & tool>".to_string()));
    }

    #[test]
    fn test_extract_command_missing() {
        let xml = r#"<Task><Actions><Exec></Exec></Actions></Task>"#;
        let cmd = extract_command(xml);
        assert_eq!(cmd, None);
    }

    #[test]
    fn test_extract_command_empty() {
        let xml = r#"<Command></Command>"#;
        let cmd = extract_command(xml);
        assert_eq!(cmd, Some("".to_string()));
    }

    // --- build_task_xml tests (2 structure checks + 1 snapshot) ---

    #[test]
    fn test_build_task_xml_structure() {
        let xml = build_task_xml(r"C:\switchboard\switchboard.exe");

        // Verify key structural elements
        assert!(xml.contains("<BootTrigger"), "XML must contain BootTrigger");
        assert!(
            xml.contains("<UserId>S-1-5-18</UserId>"),
            "XML must specify SYSTEM SID"
        );
        assert!(
            xml.contains("<Arguments>--recover</Arguments>"),
            "XML must contain --recover argument"
        );
        assert!(
            xml.contains("<ExecutionTimeLimit>PT2M</ExecutionTimeLimit>"),
            "XML must specify 2-minute time limit"
        );
    }

    #[test]
    fn test_build_task_xml_roundtrip() {
        let test_path = r"C:\test\path\switchboard.exe";
        let xml = build_task_xml(test_path);
        let extracted = extract_command(&xml);

        assert_eq!(
            extracted,
            Some(test_path.to_string()),
            "Roundtrip: build_task_xml → extract_command should recover the original path"
        );
    }

    #[test]
    fn test_build_task_xml_snapshot() {
        let xml = build_task_xml(r"C:\Program Files\switchboard\switchboard.exe");
        insta::assert_snapshot!(xml);
    }
}
