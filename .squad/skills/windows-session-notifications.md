# Skill: Windows Session Notifications (WTS APIs)

**Pattern:** Register a window to receive session change notifications (`WM_WTSSESSION_CHANGE`) for events like user login, logout, lock, unlock, remote connect/disconnect. Essential for apps that must adapt behavior during lock screen or RDP sessions.

**When to use:**
- Apps that disable hardware (keyboard, mouse) and must re-enable on lock screen
- Background services that pause work during inactive sessions
- Apps that need to defer policy changes until user unlocks (e.g., resume-from-suspend + lock-screen gating)
- Remote Desktop aware applications (detect RDP connect/disconnect)

**Requirements:**
- `windows` crate with `Win32_System_RemoteDesktop` feature
- HWND (visible or HWND_MESSAGE window)
- Must unregister before destroying window (cleanup on exit)

---

## Pattern Code

### Registration on Startup

```rust
use windows::Win32::System::RemoteDesktop::*;
use windows::Win32::Foundation::*;

fn register_session_notifications(hwnd: HWND) -> Result<(), windows::core::Error> {
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
    }
    println!("Registered for session notifications");
    Ok(())
}
```

### WndProc Handler

```rust
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_WTSSESSION_CHANGE => {
            let event = wparam.0 as u32;
            handle_session_event(event);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn handle_session_event(event: u32) {
    match event {
        WTS_CONSOLE_CONNECT => {
            println!("Session: Console connected");
        }
        WTS_CONSOLE_DISCONNECT => {
            println!("Session: Console disconnected");
        }
        WTS_REMOTE_CONNECT => {
            println!("Session: Remote (RDP) connected");
        }
        WTS_REMOTE_DISCONNECT => {
            println!("Session: Remote (RDP) disconnected");
        }
        WTS_SESSION_LOGON => {
            println!("Session: User logged on");
        }
        WTS_SESSION_LOGOFF => {
            println!("Session: User logged off");
        }
        WTS_SESSION_LOCK => {
            println!("Session: Lock screen activated");
            // Example: Re-enable disabled hardware (keyboard, mouse)
        }
        WTS_SESSION_UNLOCK => {
            println!("Session: User unlocked session");
            // Example: Resume policy enforcement (re-disable hardware if conditions met)
        }
        _ => {
            println!("Session: Unknown event {:#x}", event);
        }
    }
}
```

### Cleanup on Exit

```rust
fn unregister_session_notifications(hwnd: HWND) {
    unsafe {
        if let Err(e) = WTSUnRegisterSessionNotification(hwnd) {
            eprintln!("Failed to unregister session notifications: {:?}", e);
        } else {
            println!("Unregistered session notifications");
        }
    }
}
```

---

## Session Event Types

| Event | Value | Description |
|-------|-------|-------------|
| `WTS_CONSOLE_CONNECT` | 0x1 | User connected to console session (physical login) |
| `WTS_CONSOLE_DISCONNECT` | 0x2 | User disconnected from console session |
| `WTS_REMOTE_CONNECT` | 0x3 | User connected via RDP |
| `WTS_REMOTE_DISCONNECT` | 0x4 | User disconnected from RDP |
| `WTS_SESSION_LOGON` | 0x5 | User logged on to session |
| `WTS_SESSION_LOGOFF` | 0x6 | User logged off from session |
| `WTS_SESSION_LOCK` | 0x7 | Lock screen activated (Win+L, auto-lock) |
| `WTS_SESSION_UNLOCK` | 0x8 | User unlocked session (entered password) |
| `WTS_SESSION_REMOTE_CONTROL` | 0x9 | Session remote control state changed |

**Most common use cases:**
- `WTS_SESSION_LOCK` / `WTS_SESSION_UNLOCK` — Lock screen detection (e.g., re-enable keyboard at lock, re-disable after unlock)
- `WTS_REMOTE_CONNECT` / `WTS_REMOTE_DISCONNECT` — RDP awareness (e.g., disable blocking when RDP active)
- `WTS_SESSION_LOGOFF` — Cleanup before user logs off

---

## Querying Session State

To check current session state (active, disconnected, etc.):

```rust
fn is_session_active() -> bool {
    unsafe {
        let mut info: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut bytes: u32 = 0;
        
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSConnectState,
            &mut info as *mut _ as *mut _,
            &mut bytes,
        )
        .as_bool()
        {
            let state_val = *(info as *const u32);
            WTSFreeMemory(info); // MUST free memory
            
            // WTSActive = 0, WTSConnected = 1, WTSDisconnected = 4, etc.
            state_val == 0 // WTSActive
        } else {
            false
        }
    }
}
```

**Important:** Always call `WTSFreeMemory(info)` after reading. Failing to free causes memory leak.

---

## Common Use Case: Resume Gating (Lock Screen Safety)

Problem: On resume from sleep, Windows fires `WM_POWERBROADCAST` with `PBT_APMRESUMEAUTOMATIC` BEFORE the lock screen appears. If your app re-disables hardware (e.g., internal keyboard) immediately, the user is locked out at the lock screen.

Solution: Set a `resume_pending` flag on resume, defer policy enforcement until `WTS_SESSION_UNLOCK`:

```rust
struct AppState {
    resume_pending: bool,
    // ... other fields
}

// Power event handler
fn handle_power_event(state: &mut AppState, event: u32) {
    match event {
        PBT_APMRESUMEAUTOMATIC => {
            println!("Resume: setting resume_pending, deferring policy");
            state.resume_pending = true;
            // Re-enable hardware defensively (don't re-disable yet)
            enable_hardware();
        }
        _ => {}
    }
}

// Session event handler
fn handle_session_event(state: &mut AppState, event: u32) {
    if event == WTS_SESSION_UNLOCK && state.resume_pending {
        println!("Unlock: clearing resume_pending, resuming policy");
        state.resume_pending = false;
        // Now safe to re-disable hardware if conditions are met
        apply_policy(state);
    }
}
```

**Timeout:** Add a sanity timer (e.g., 2 min) to clear `resume_pending` if unlock never fires (e.g., user walks away, leaving laptop at lock screen). Prevents perpetual deferral.

---

## Safety Notes

1. **Must unregister before window destruction:** If you destroy the HWND without calling `WTSUnRegisterSessionNotification`, Windows may try to deliver notifications to an invalid window handle. Crash or undefined behavior.

2. **NOTIFY_FOR_THIS_SESSION vs NOTIFY_FOR_ALL_SESSIONS:**
   - `NOTIFY_FOR_THIS_SESSION` (0) — Only receive notifications for the session that owns the HWND. Recommended for user apps.
   - `NOTIFY_FOR_ALL_SESSIONS` (1) — Receive notifications for all sessions. Requires admin privileges. Use for services or system-level tools.

3. **Lock screen limitations:** Apps running as standard user cannot interact with secure desktop (UAC prompts, lock screen credential entry). WTS notifications fire, but your app's UI is hidden. Use notifications to adapt behavior (e.g., re-enable hardware), not to display UI.

---

## Related Patterns

- **windows-hidden-message-window.md** — Hidden HWND_MESSAGE window setup (where WTS notifications are received)
- **windows-lparam-box-pattern.md** — Cross-thread communication (if session handler needs to signal worker thread)

---

**Source:** kbblock v0.1 main.rs (Newman, 2026-04-21)  
**Validated on:** Windows 11 ARM64 (Surface Laptop 7)  
**References:** [Microsoft Docs: WTSRegisterSessionNotification](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification)
