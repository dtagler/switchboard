# Skill: LPARAM Box Pattern (Passing Owned Data via Win32 Messages)

**Pattern:** Pass owned Rust data from worker thread to main thread (or between threads) via Win32 `PostMessageW` by leaking a `Box` as the `LPARAM` parameter. The receiving WndProc unpacks the Box and takes ownership, freeing memory on drop.

**When to use:**
- Worker thread needs to return complex results to main thread's message loop
- Event-driven communication where `mpsc::channel` is awkward (main thread is message-loop-bound)
- Passing non-Copy data (strings, Vecs, custom structs) via Win32 messages
- Single-producer, single-consumer communication where worker posts exactly one message per result

**Requirements:**
- `windows` crate with `Win32_UI_WindowsAndMessaging` feature
- Worker thread has HWND of main thread's message window
- WndProc handler for the custom message unpacks the Box (MUST NOT forget this step — memory leak otherwise)

---

## Pattern Code

### Define Custom Message and Data Type

```rust
use windows::Win32::UI::WindowsAndMessaging::*;

// Custom message ID (WM_APP + N)
const WM_WORKER_RESULT: u32 = WM_APP + 3;

// Data type to pass
#[derive(Debug)]
struct WorkerResult {
    op_id: u64,
    success: bool,
    message: String,
    data: Vec<u8>,
}
```

### Worker Thread: Leak Box as LPARAM

```rust
use windows::Win32::Foundation::*;

fn worker_thread_main(hwnd: HWND) {
    // ... do work
    
    let result = WorkerResult {
        op_id: 42,
        success: true,
        message: "Operation completed".to_string(),
        data: vec![1, 2, 3, 4, 5],
    };
    
    // Leak Box as LPARAM (transfers ownership to message)
    let result_ptr = Box::into_raw(Box::new(result));
    
    unsafe {
        PostMessageW(
            hwnd,
            WM_WORKER_RESULT,
            WPARAM(0), // Can use wparam for small metadata (flags, IDs)
            LPARAM(result_ptr as isize),
        )
        .expect("PostMessageW failed");
    }
    
    // Box is now owned by the message — don't drop it here
}
```

### WndProc Handler: Unpack Box and Take Ownership

```rust
unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_WORKER_RESULT => {
            // Unpack Box from LPARAM
            let result_ptr = lparam.0 as *mut WorkerResult;
            
            if !result_ptr.is_null() {
                // Take ownership — Box will be freed on drop
                let result = Box::from_raw(result_ptr);
                
                println!("Received result: op_id={}, success={}, message={}, data={:?}",
                    result.op_id, result.success, result.message, result.data);
                
                // result is automatically freed here (Drop)
            } else {
                eprintln!("WM_WORKER_RESULT: null pointer!");
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

## Memory Safety

### ✅ Safe Cases

1. **Normal path:** Worker leaks Box → message queued → WndProc unpacks Box → Box freed on drop. No leak, no double-free.

2. **Worker panics with panic="abort":** Process terminates immediately. Box is never unpacked, but process is dead, so no observable leak.

3. **WndProc receives message with null LPARAM:** Null check prevents invalid pointer dereference. Log error, return.

### ⚠️ Unsafe Cases (MUST AVOID)

1. **WndProc forgets to unpack:** If WndProc receives `WM_WORKER_RESULT` but doesn't call `Box::from_raw`, the Box leaks. **CRITICAL: Every message sent must be unpacked exactly once.**

2. **Worker sends multiple messages for same data:** If worker calls `Box::into_raw` twice with the same Box, the second `into_raw` is a use-after-free (Box was already moved). Only leak each Box once.

3. **WndProc unpacks twice:** If WndProc saves the LPARAM pointer and calls `Box::from_raw` twice, it's a double-free (undefined behavior). Only unpack each LPARAM once.

4. **Message queue cleared without processing:** If the message loop exits before processing queued messages, leaked Boxes are never unpacked. Mitigation: clean shutdown sequence (flush message queue before exit, or accept leak on abnormal termination).

---

## Alternative: Smaller Data via WPARAM/LPARAM Encoding

For small data (≤16 bytes on x64, ≤8 bytes on x86), encode directly into WPARAM/LPARAM:

```rust
// Pack two u32s into WPARAM and LPARAM
let op_id: u32 = 42;
let flags: u32 = 0x1234;

unsafe {
    PostMessageW(
        hwnd,
        WM_WORKER_RESULT,
        WPARAM(op_id as usize),
        LPARAM(flags as usize),
    ).ok();
}

// Unpack in WndProc
WM_WORKER_RESULT => {
    let op_id = wparam.0 as u32;
    let flags = lparam.0 as u32;
    // ... no Box allocation or deallocation
    LRESULT(0)
}
```

**Trade-off:** Only works for Copy types ≤ pointer size. Strings, Vecs, or large structs require the Box pattern.

---

## Comparison to `mpsc::channel`

| Criterion | LPARAM Box Pattern | `mpsc::channel` |
|-----------|-------------------|-----------------|
| **Main thread bound to message loop** | ✅ Works — PostMessage integrates with GetMessage loop | ❌ Must poll channel in message loop (e.g., on timer tick) or use `try_recv` in every WndProc message |
| **Single result per operation** | ✅ Natural — one message = one result | ⚠️ Can queue multiple results, but must drain channel |
| **Type safety** | ⚠️ LPARAM is `isize` — any pointer can be cast | ✅ Channel is `Sender<T>` — type-checked at compile time |
| **Error handling** | ⚠️ PostMessage can fail (e.g., window destroyed); must check | ✅ Channel send fails if receiver dropped (explicit `Result`) |
| **Message ordering** | ✅ Guaranteed FIFO (Win32 message queue) | ✅ Guaranteed FIFO (mpsc ordering) |
| **Memory overhead** | ~1 heap allocation per message | ~1 heap allocation per message + channel metadata |

**Recommendation:** Use LPARAM Box pattern when main thread is message-loop-bound and each worker operation sends exactly one result. Use `mpsc::channel` when multiple results per operation, or when worker and main thread are both active loops (not message-driven).

---

## Related Patterns

- **windows-hidden-message-window.md** — WndProc setup for receiving messages
- **rust-bounded-join-panic-abort.md** — Alternative to message passing (blocking join with timeout)

---

**Source:** kbblock v0.1 main.rs (Newman, 2026-04-21)  
**Validated on:** Windows 11 ARM64 (Surface Laptop 7), Rust 1.85+
