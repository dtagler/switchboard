# Skill: Hidden HWND_MESSAGE Window + WndProc in Pure Rust

**Pattern:** Create a hidden message-only window for receiving Win32 messages (power, session, custom app messages) without a visible UI. Store application state pointer in window user data for WndProc access.

**When to use:**
- Background apps that need to receive system messages (WM_POWERBROADCAST, WM_DEVICECHANGE, WM_WTSSESSION_CHANGE)
- Cross-thread communication via PostMessage (worker thread → main thread)
- Timer-driven periodic tasks (SetTimer + WM_TIMER)
- Tray apps that don't need a visible window (tray icon provides UI)

**Requirements:**
- `windows` crate with `Win32_UI_WindowsAndMessaging`, `Win32_Foundation` features
- Single-threaded message loop on the thread that creates the window
- Stable application state that outlives all messages (typically stack-allocated in main thread)

---

## Pattern Code

```rust
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct AppState {
    // Your application state
    counter: u32,
}

fn main() {
    let mut state = AppState { counter: 0 };
    
    // Create hidden message window
    let hwnd = create_hidden_message_window();
    
    // Store AppState pointer in window user data for wndproc access
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut state as *mut AppState as isize);
    }
    
    // Message loop
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn create_hidden_message_window() -> HWND {
    unsafe {
        let class_name = w!("my_msg_window");
        
        // Register window class
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);
        
        // Create hidden window (HWND_MESSAGE parent)
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("my_app"),
            WINDOW_STYLE(0),
            0, 0, 0, 0,
            HWND_MESSAGE, // Hidden message-only window
            None,
            None,
            None,
        )
        .expect("CreateWindowExW failed")
    }
}

// Window procedure
unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Retrieve AppState pointer from window user data
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
    
    if state_ptr.is_null() && msg != WM_CREATE && msg != WM_DESTROY {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    
    match msg {
        WM_TIMER => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.counter += 1;
                println!("Timer tick: {}", state.counter);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
```

---

## Safety Notes

1. **Pointer lifetime:** AppState must outlive all messages. If AppState is stack-allocated in main thread and window is destroyed before main returns, this is safe. If AppState is heap-allocated (Box), either leak it or ensure it's dropped AFTER `PostQuitMessage(0)`.

2. **Null check in WndProc:** GWLP_USERDATA is 0 before `SetWindowLongPtrW` is called. WndProc may receive WM_CREATE before main thread sets the pointer. Check for null and use `DefWindowProcW` for early messages.

3. **Message loop thread:** WndProc runs on the thread that called `CreateWindowExW`. This must be the same thread running the message loop (`GetMessageW` / `DispatchMessageW`). Do NOT create the window on one thread and run the message loop on another.

4. **`unsafe` justification:** Raw pointer dereference (`&mut *state_ptr`) is safe because:
   - Pointer is valid (points to stack-allocated AppState in main thread)
   - Lifetime is correct (AppState outlives all messages)
   - No concurrent access (message loop is single-threaded)

---

## Cross-Thread Communication

To post custom messages from worker threads:

```rust
// Define custom message
const WM_WORKER_RESULT: u32 = WM_APP + 1;

// Worker thread
let result = Box::new(MyResult { ... });
unsafe {
    PostMessageW(
        hwnd,
        WM_WORKER_RESULT,
        WPARAM(0),
        LPARAM(Box::into_raw(result) as isize),
    ).ok();
}

// WndProc handler
WM_WORKER_RESULT => {
    let result_ptr = lparam.0 as *mut MyResult;
    if !result_ptr.is_null() {
        let result = Box::from_raw(result_ptr); // Takes ownership
        // ... use result (freed on drop)
    }
    LRESULT(0)
}
```

See `windows-lparam-box-pattern.md` for LPARAM Box lifecycle details.

---

## Related Patterns

- **windows-lparam-box-pattern.md** — Passing owned data via Win32 messages
- **windows-session-notifications.md** — WTSRegisterSessionNotification for session events
- **rust-bounded-join-panic-abort.md** — Thread join with timeout (for worker threads posting to window)

---

**Source:** kbblock v0.1 main.rs (Newman, 2026-04-21)  
**Validated on:** Windows 11 ARM64 (Surface Laptop 7)
