# Skill: Windows Silent Logon Elevation via Task Scheduler

**Pattern:** Achieve elevated logon autostart without UAC prompts using Task Scheduler LogonTrigger + HighestAvailable

**Use Case:** Windows tray apps or services that need admin privileges at user logon but can't tolerate UAC consent dialogs interrupting the logon flow.

## Problem Space

When a Windows executable has `<requestedExecutionLevel level="requireAdministrator" />` in its manifest:
- Double-click from Explorer → UAC consent dialog (expected, one-time)
- HKCU Run-key autostart (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) → **UAC dialog EVERY logon** (user friction, unacceptable for production)
- App can't start elevated at logon without user clicking "Yes" on UAC each time

Standard workarounds have drawbacks:
- **Manifest `asInvoker`:** App can't perform admin operations (e.g., disable devices via SetupAPI, write to HKLM registry)
- **Windows service:** Runs as SYSTEM, can't create tray icons in user session without impersonation complexity
- **Two-exe launcher:** Adds complexity, user confusion

## Solution: LogonTrigger + HighestAvailable

Use a **Task Scheduler 2.0 logon-trigger task** with `<RunLevel>HighestAvailable</RunLevel>` on the Principal:

1. Task Scheduler pre-elevates the token at logon (no UAC prompt)
2. Task runs as the interactive user (not SYSTEM), so tray/UI work correctly
3. `<LogonTrigger><UserId>` scopes to current user (per-user autostart)

## Implementation Pattern (Rust + windows-rs)

### Required Cargo.toml Features

```toml
[dependencies]
windows = { version = "0.58", features = [
  "Win32_System_Com",
  "Win32_System_TaskScheduler",
  "Win32_System_WindowsProgramming",
  "Win32_Foundation",
] }
```

### Task XML Template

```xml
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Your app autostart with silent elevation</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{current_username}</UserId>  <!-- Get via GetUserNameW -->
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{current_username}</UserId>  <!-- Same as trigger -->
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>  <!-- KEY: Silent elevation -->
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>  <!-- Don't clutter Task Scheduler UI -->
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>  <!-- No timeout for long-running apps -->
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{path_to_exe}</Command>  <!-- Absolute path to your .exe -->
    </Exec>
  </Actions>
</Task>
```

### Rust Code Pattern

```rust
use windows::core::{BSTR, GUID, HRESULT, Interface, VARIANT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IRegisteredTask, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
};

const CLSID_TASK_SCHEDULER: GUID = GUID::from_u128(0x0F87369F_A4E5_4CFC_BD3E_73E6154572DD);
const HRESULT_FILE_NOT_FOUND: HRESULT = HRESULT(0x80070002u32 as i32);

pub const TASK_NAME: &str = "your-app-logon";

fn with_folder<R>(f: impl FnOnce(&ITaskFolder) -> windows::core::Result<R>) -> windows::core::Result<R> {
    unsafe {
        // CoInitializeEx returns HRESULT - S_OK (0) on success
        // RPC_E_CHANGED_MODE (0x80010106) means COM already initialized in different mode
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let hr_code = hr.0;
        let needs_uninit = if hr_code == 0 {
            true // S_OK - we initialized COM, must uninit
        } else if hr_code == 0x80010106u32 as i32 {
            false // RPC_E_CHANGED_MODE - already init'd, skip uninit
        } else {
            return Err(windows::core::Error::from(hr)); // Other error - fail
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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn build_task_xml(exe_path: &str, username: &str, description: &str) -> String {
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
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{path}</Command>
    </Exec>
  </Actions>
</Task>"#,
        desc = xml_escape(description),
        user = xml_escape(username),
        path = xml_escape(exe_path)
    )
}

pub fn enable_logon_autostart(exe_path: &str) -> Result<(), String> {
    let username = get_current_username()?;
    let xml = build_task_xml(exe_path, &username, "Your app autostart");

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
    .map_err(|e| format!("Task Scheduler error: {e}"))
}

pub fn disable_logon_autostart() -> Result<(), String> {
    with_folder(|folder| unsafe {
        match folder.DeleteTask(&BSTR::from(TASK_NAME), 0) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == HRESULT_FILE_NOT_FOUND => Ok(()), // Idempotent
            Err(e) => Err(e),
        }
    })
    .map_err(|e| format!("Task Scheduler error: {e}"))
}

pub fn is_logon_autostart_enabled() -> bool {
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
```

## Key Gotchas

1. **RPC_E_CHANGED_MODE tolerance:** If another component (e.g., BLE WinRT APIs) already initialized COM in STA mode, `CoInitializeEx(COINIT_MULTITHREADED)` returns `0x80010106`. Treat this as success and skip `CoUninitialize()` — only uninit if you initialized.

2. **GetUserNameW returns Result<()> in windows-rs 0.58:** Use `match` pattern, not `.as_bool()` (older API).

3. **XML escaping required:** User paths and descriptions can contain `&`, `<`, `>`, `"` — always escape them when building XML strings.

4. **HighestAvailable vs RequireHighest:**
   - `HighestAvailable`: Uses admin token if available, otherwise runs limited (graceful degradation) — **choose this for silent elevation**
   - `RequireHighest`: Always requires admin, will fail if user is not admin — don't use for logon autostart

5. **UserId must match Principal and Trigger:** Same username in `<LogonTrigger><UserId>` and `<Principal><UserId>` — get once via `GetUserNameW`, use in both places.

6. **Task Scheduler service dependency:** If user disables Task Scheduler service, autostart won't work. This is rare (Task Scheduler is core Windows component).

## Testing

1. **Enable test:**
   - Call `enable_logon_autostart()` with current exe path
   - Verify task appears in Task Scheduler GUI (Win+R → `taskschd.msc`, check root folder)

2. **Logon test (CRITICAL):**
   - Reboot system, log in to Windows
   - Verify app launches with **NO UAC prompt**
   - Verify app has elevated privileges (can perform admin operations)

3. **Disable test:**
   - Call `disable_logon_autostart()`
   - Verify task deleted from Task Scheduler GUI

4. **Idempotency:**
   - Call `enable_logon_autostart()` twice → second call succeeds (updates existing task)
   - Call `disable_logon_autostart()` twice → second call succeeds (task already gone)

## Migration from HKCU Run-Key

If migrating from legacy HKCU Run-key autostart:

```rust
fn delete_legacy_run_key() -> Result<(), String> {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, RegOpenKeyExW, RegDeleteValueW, RegCloseKey,
    };
    use windows::Win32::Foundation::{ERROR_SUCCESS, ERROR_FILE_NOT_FOUND};
    use windows::core::PCWSTR;

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let subkey = to_wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
        let value = to_wide("your-app-name");

        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        let open = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()), 0, KEY_SET_VALUE, &mut hkey);
        if open != ERROR_SUCCESS {
            return Ok(()); // Key doesn't exist - fine
        }

        let del = RegDeleteValueW(hkey, PCWSTR(value.as_ptr()));
        let _ = RegCloseKey(hkey);

        if del == ERROR_SUCCESS || del == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            // Log but don't fail - this is migration cleanup
            log::warn!("Could not delete legacy Run-key: 0x{:08X}", del.0);
            Ok(())
        }
    }
}
```

Call `delete_legacy_run_key()` at the end of `enable_logon_autostart()` to clean up orphaned entries.

## When to Use This Pattern

✅ **Good fit:**
- Tray apps that need admin privileges at logon
- Apps with `requireAdministrator` manifest that must auto-start
- Per-user autostart (each user enables separately)
- Elevated operations at logon (device enable/disable, HKLM registry writes, firewall rules)

❌ **Not a fit:**
- Cross-user autostart (use HKLM Run-key or per-machine task instead)
- Apps that don't need elevation (use HKCU Run-key, simpler)
- Services (use Windows Service architecture instead)

## References

- Microsoft Docs: [Task Scheduler 2.0](https://docs.microsoft.com/en-us/windows/win32/taskschd/task-scheduler-start-page)
- Microsoft Docs: [LogonTrigger Element](https://docs.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-logontrigger-triggergroup-element)
- Microsoft Docs: [RunLevel Attribute](https://docs.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-runlevel-principaltype-attribute)
- windows-rs: [Win32::System::TaskScheduler](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/TaskScheduler/)

## Real-World Example

kbblock v0.1.2 uses this pattern in `src/autostart.rs` to achieve silent logon elevation for a keyboard-blocking tray app that needs SetupAPI device control (admin-only operation). See `.squad/decisions/inbox/newman-logon-task-silent-elevation.md` for full decision rationale.
