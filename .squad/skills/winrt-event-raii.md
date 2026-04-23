# Skill: WinRT Event Subscription with RAII Cleanup (windows-rs 0.58)

**Pattern Type:** windows-rs / WinRT event handling  
**Language:** Rust  
**Crate:** windows 0.58  
**Provenance:** bluetooth-keyboard-app v0.1 (Kramer, 2026-04-21) — `src/ble.rs` module  
**Validation:** Spike 1.6 v4 on Surface Laptop 7 ARM64 (11 clean transitions under requireAdministrator)

---

## Problem

WinRT events in windows-rs 0.58 use a two-step subscription model:

1. Call `SomeEvent::add(&TypedEventHandler::new(...))` → returns `EventRegistrationToken`
2. Must call `RemoveSomeEvent(token)` to unsubscribe, or the handler leaks

**Key challenges:**
- Token must be stored somewhere so it can be passed to Remove later
- Drop must unsubscribe to prevent resource leaks
- Drop must NOT panic (violates Rust safety — can poison entire process)
- Handler closure often needs to communicate with other parts of the app (e.g., post messages, send on channel)

**This pattern solves all four.**

---

## Solution Pattern

### 1. Store Token in a Struct (RAII)

```rust
pub struct EventHandle {
    device: SomeWinRTDevice,  // Keep the WinRT object alive
    token: windows::Foundation::EventRegistrationToken,
}
```

### 2. Subscribe in a Constructor/Factory

```rust
pub fn start(hwnd: HWND) -> Result<EventHandle, MyError> {
    // Resolve WinRT device (e.g., BluetoothLEDevice::FromBluetoothAddressAsync)
    let device = /* ... */;

    // Subscribe to event
    let token = device.SomeEvent(&TypedEventHandler::new(
        move |sender, args| {
            // Handler logic here
            // Can capture by move (HWND, Arc<...>, mpsc::Sender, etc.)
            unsafe {
                PostMessageW(hwnd, MY_CUSTOM_MESSAGE, None, None);
            }
            Ok(())
        },
    ))?;

    Ok(EventHandle { device, token })
}
```

### 3. Drop Impl Unsubscribes (Fail-Safe Cleanup)

```rust
impl Drop for EventHandle {
    fn drop(&mut self) {
        if let Err(e) = self.device.RemoveSomeEvent(self.token) {
            // Log error but do NOT panic
            log::error!("Failed to unsubscribe event (HRESULT {:#x})", e.code().0);
        } else {
            log::info!("Unsubscribed event");
        }
    }
}
```

**Critical:** Do NOT `unwrap()` or `expect()` in Drop. Always log and proceed. Panicking in Drop during stack unwinding is UB.

---

## Complete Example: BluetoothLEDevice.ConnectionStatusChanged

This is the real implementation from `bluetooth-keyboard-app/src/ble.rs`:

```rust
use log::{error, info, warn};
use windows::core::*;
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice};
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const MY_MESSAGE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

pub struct BleHandle {
    device: BluetoothLEDevice,
    token: windows::Foundation::EventRegistrationToken,
}

impl BleHandle {
    pub fn is_connected(&self) -> bool {
        match self.device.ConnectionStatus() {
            Ok(BluetoothConnectionStatus::Connected) => true,
            Ok(_) => false,
            Err(e) => {
                warn!("ConnectionStatus() failed (HRESULT {:#x}), treating as disconnected", e.code().0);
                false
            }
        }
    }
}

impl Drop for BleHandle {
    fn drop(&mut self) {
        if let Err(e) = self.device.RemoveConnectionStatusChanged(self.token) {
            error!("Failed to unsubscribe ConnectionStatusChanged (HRESULT {:#x})", e.code().0);
        } else {
            info!("Unsubscribed ConnectionStatusChanged");
        }
    }
}

pub fn start(hwnd: HWND, bd_addr: u64) -> Result<BleHandle> {
    let op = BluetoothLEDevice::FromBluetoothAddressAsync(bd_addr)?;
    let device = op.get()?.ok_or_else(|| Error::from(HRESULT(-1)))?;

    let token = device.ConnectionStatusChanged(&TypedEventHandler::new(
        move |dev: &Option<BluetoothLEDevice>, _args| {
            if let Some(d) = dev.as_ref() {
                let status = d.ConnectionStatus().unwrap_or(BluetoothConnectionStatus::Disconnected);
                info!("ConnectionStatusChanged => {:?}", status);

                unsafe {
                    let _ = PostMessageW(hwnd, MY_MESSAGE, None, None);
                }
            }
            Ok(())
        },
    ))?;

    info!("Subscribed to ConnectionStatusChanged");
    Ok(BleHandle { device, token })
}
```

---

## Key Points

### 1. HWND Passing is Sound

HWND is `Copy`. The closure captures `hwnd` by value. `PostMessageW` is thread-safe (Win32 guarantee). Message loop is always alive while the handle exists. **No Arc/Mutex needed.**

### 2. Other Communication Patterns

- **mpsc channel:** `let (tx, rx) = mpsc::channel(); let token = device.Event(&TypedEventHandler::new(move |...| { tx.send(...); Ok(()) }))?;`
- **Shared state:** `let state = Arc::new(Mutex<MyState>); let s = Arc::clone(&state); let token = device.Event(&TypedEventHandler::new(move |...| { s.lock().unwrap().update(); Ok(()) }))?;`

### 3. COM Initialization

WinRT calls require COM initialization. Call `CoInitializeEx(None, COINIT_APARTMENTTHREADED)` before the first WinRT call. windows-rs 0.58 treats `S_FALSE` (already initialized) as `Ok(())`.

```rust
unsafe {
    CoInitializeEx(None, COINIT_APARTMENTTHREADED)?;
}
```

### 4. Null-Check Device (Common Pattern)

Many WinRT async operations return `Option<T>`. Null means "not found":

```rust
let device = op.get()?.ok_or_else(|| MyError::NotFound)?;
```

### 5. Error Handling in Event Handler

The handler returns `Result<()>`. If you return `Err(...)`, Windows logs it internally but does NOT propagate to your code. **Prefer logging + Ok(()) over Err(...) for diagnostics.**

```rust
TypedEventHandler::new(move |sender, args| {
    match do_work(sender, args) {
        Ok(()) => Ok(()),
        Err(e) => {
            error!("Event handler failed: {}", e);
            Ok(())  // Swallow error; log is sufficient
        }
    }
})
```

---

## Other WinRT Events This Applies To

- **DeviceWatcher:** `Added`, `Updated`, `Removed`, `EnumerationCompleted`, `Stopped`
- **NetworkInformation:** `NetworkStatusChanged`
- **BluetoothLEDevice:** `ConnectionStatusChanged`, `NameChanged`, `GattServicesChanged`
- **Geolocator:** `PositionChanged`, `StatusChanged`
- **All WinRT events in windows-rs that use `TypedEventHandler` + `EventRegistrationToken`**

---

## Common Mistakes

### ❌ Discarding the token

```rust
let _ = device.SomeEvent(&handler)?;  // Token lost! Can't unsubscribe.
```

**Fix:** Always store the token.

### ❌ Panicking in Drop

```rust
impl Drop for Handle {
    fn drop(&mut self) {
        self.device.RemoveSomeEvent(self.token).unwrap();  // PANIC = UB!
    }
}
```

**Fix:** Always log + proceed. Use `if let Err(e) = ...` or `.ok()`.

### ❌ Discarding the WinRT object

```rust
let device = resolve_device()?;
let token = device.SomeEvent(&handler)?;
drop(device);  // Device gone! Event subscription dangling.
```

**Fix:** Store both `device` and `token` in the handle struct.

### ❌ Using unwrap() in the handler

```rust
TypedEventHandler::new(|sender, args| {
    let data = args.unwrap().GetData().unwrap();  // Can panic!
    Ok(())
})
```

**Fix:** Use `?` or `if let Some(...)` + log error + `return Ok(())`.

---

## Validation Checklist

- [ ] Token stored in a struct
- [ ] Drop calls Remove (no panic)
- [ ] Device object kept alive (stored in same struct)
- [ ] Handler does NOT panic (all unwraps replaced with `?` or `.ok()`)
- [ ] Communication mechanism captured in closure (HWND, mpsc, Arc, etc.)
- [ ] COM initialized before first WinRT call (`CoInitializeEx`)

---

## References

- **Source:** `bluetooth-keyboard-app/src/ble.rs` (v0.1, 2026-04-21)
- **Spike validation:** `bluetooth-keyboard-app/spikes/spike1.6/src/main.rs` (11 transitions, requireAdministrator, ARM64)
- **windows-rs docs:** https://microsoft.github.io/windows-docs-rs/doc/windows/
- **WinRT event pattern:** https://learn.microsoft.com/en-us/uwp/api/windows.foundation.typedeventhandler-2

---

**Status:** Production-validated. Safe to copy-paste for any WinRT event subscription.
