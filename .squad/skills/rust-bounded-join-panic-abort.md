# Skill: Bounded Join with panic="abort"

**Pattern:** Implement `JoinHandle::join()` with timeout when `panic = "abort"` is set in Cargo.toml. The standard library doesn't provide `join_timeout()`, and panics kill the entire process when panic="abort" is configured.

**When to use:**
- Shutdown sequences that must not hang indefinitely
- Graceful degradation when worker threads are stuck in blocking calls
- Resource cleanup paths that need bounded waiting
- Any join where "proceed with fallback" is safer than "wait forever"

**Requirements:**
- `std::thread` + `std::sync::mpsc`
- `panic = "abort"` in `[profile.release]` (or any profile)
- Fallback logic that can proceed if worker doesn't exit cleanly

---

## Pattern Code

```rust
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn bounded_join<T>(handle: JoinHandle<T>, timeout: Duration) -> Result<T, JoinError>
where
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    
    // Spawn helper thread that joins the worker
    thread::spawn(move || {
        let result = handle.join();
        tx.send(result).ok(); // Ignore send failure (rx dropped = caller timed out)
    });
    
    // Wait for result with timeout
    match rx.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),              // Clean exit
        Ok(Err(_)) => Err(JoinError::Panicked),  // Worker panicked (shouldn't happen with panic=abort)
        Err(_) => Err(JoinError::Timeout),       // Timeout — worker stuck
    }
}

#[derive(Debug)]
enum JoinError {
    Timeout,
    Panicked,
}

// Example usage: Quit sequence with 500ms timeout
fn handle_quit(worker_handle: JoinHandle<()>) {
    println!("Sending shutdown signal...");
    // (send Cmd::Shutdown via channel)
    
    println!("Joining worker thread (500ms timeout)...");
    match bounded_join(worker_handle, Duration::from_millis(500)) {
        Ok(()) => {
            println!("Worker exited cleanly");
        }
        Err(JoinError::Timeout) => {
            println!("Worker join timeout — proceeding with fallback");
            // Fallback: inline cleanup (worker is stuck in blocking call)
        }
        Err(JoinError::Panicked) => {
            println!("Worker panicked (unexpected with panic=abort)");
        }
    }
    
    // Always proceed with fallback (e.g., inline ENABLE for keyboard app)
    // Never hang — timeout is the escape hatch
}
```

---

## Why This Works

1. **Helper thread isolation:** The helper thread calls `handle.join()`, which blocks until the worker exits. If the worker is stuck, the helper blocks too — but the helper is disposable. The main thread waits on the channel with `recv_timeout`, which is bounded.

2. **Channel send failure is safe:** If the main thread times out and drops `rx` before the helper sends, `tx.send()` returns `Err(_)`. We ignore this error (`.ok()`) because the result is no longer needed.

3. **Panic behavior with panic="abort":** If the worker panics with `panic="abort"`, the entire process terminates immediately. The `Ok(Err(_))` branch (worker panicked) should never execute in production. It's included for completeness (if panic="unwind" was accidentally enabled in dev profile, or in tests).

4. **Timeout = stuck in blocking call:** The only way `recv_timeout` returns `Err(_)` (timeout) is if the helper thread is still blocked on `handle.join()`, which means the worker thread is still running (stuck in a blocking syscall or infinite loop). With panic="abort", panics don't leave threads running — they kill the process.

---

## Safety Notes

1. **Worker thread must not hold resources that need cleanup:** If the worker holds a lock or file handle and times out, those resources leak. This is acceptable if the process is about to exit anyway (e.g., Quit sequence). If you need to retry or continue running, ensure the worker uses RAII (Drop implementations) for critical resources.

2. **No cancellation mechanism:** This pattern does NOT cancel the worker thread. The worker keeps running after timeout. If the worker eventually completes, the helper thread exits cleanly, but the main thread has already moved on. This is fine for shutdown sequences where "proceed with fallback" is the correct behavior.

3. **Memory overhead:** Each bounded_join spawns a helper thread. For high-frequency joins, this adds overhead. For shutdown sequences (once per app lifetime), it's negligible.

---

## Alternative: Thread + Arc + AtomicBool

If you need true cancellation (e.g., worker checks a flag and exits early), use `Arc<AtomicBool>`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn spawn_cancellable_worker() -> (JoinHandle<()>, Arc<AtomicBool>) {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = cancel_flag.clone();
    
    let handle = thread::spawn(move || {
        loop {
            if flag_clone.load(Ordering::Relaxed) {
                println!("Worker: cancel requested, exiting");
                break;
            }
            // ... do work
        }
    });
    
    (handle, cancel_flag)
}

// Caller
let (handle, cancel_flag) = spawn_cancellable_worker();
// ... later, to cancel:
cancel_flag.store(true, Ordering::Relaxed);
let _ = bounded_join(handle, Duration::from_millis(500));
```

**Trade-off:** Worker must check the flag periodically. If the worker is blocked on a syscall (e.g., `recv()` on a channel with no timeout), it won't respond to the flag until the syscall completes.

---

## Related Patterns

- **windows-hidden-message-window.md** — Main thread message loop (where join happens)
- **windows-lparam-box-pattern.md** — Worker thread → main thread communication (alternative to join)

---

**Source:** kbblock v0.1 main.rs (Newman, 2026-04-21)  
**Validated on:** Windows 11 ARM64 (Surface Laptop 7), Rust 1.85+, panic="abort"
