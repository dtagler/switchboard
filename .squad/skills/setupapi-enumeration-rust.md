# Skill: SetupAPI Device Enumeration in Rust (windows-rs)

**Author:** Newman  
**Date:** 2026-04-21  
**Context:** Extracted from `device.rs` implementation during bluetooth-keyboard-app v0.1

---

## Problem

Windows SetupAPI device enumeration in Rust requires:
1. RAII wrapper for `HDEVINFO` to ensure cleanup on all paths (success, error, panic)
2. Two-phase property reads (size query + buffer allocation)
3. UTF-16 ↔ UTF-8 conversion for all device strings
4. Proper error handling (Win32 `GetLastError()`, EOF detection via `ERROR_NO_MORE_ITEMS`)
5. Multi-string parsing (e.g., `SPDRP_HARDWAREID` is double-null-terminated)

This is boilerplate-heavy and error-prone to implement from scratch each time.

---

## Solution Pattern

### 1. RAII Wrapper for HDEVINFO

```rust
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiGetClassDevsW, SetupDiDestroyDeviceInfoList, HDEVINFO, DIGCF_PRESENT
};

struct DeviceInfoSet(HDEVINFO);

impl DeviceInfoSet {
    fn new(class_guid: &windows::core::GUID, flags: u32) -> Result<Self, DeviceError> {
        let hdevinfo = unsafe { SetupDiGetClassDevsW(Some(class_guid), None, None, flags) };

        if hdevinfo.is_invalid() {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(DeviceError::Win32 {
                api: "SetupDiGetClassDevsW",
                last_error: err.0,
            });
        }

        Ok(Self(hdevinfo))
    }

    fn handle(&self) -> HDEVINFO {
        self.0
    }
}

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }
}
```

**Why this works:** `Drop` ensures cleanup even if a `?` early-return or panic occurs during enumeration. No manual cleanup tracking needed.

---

### 2. Two-Phase Property Read Helper

```rust
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiGetDeviceRegistryPropertyW, SP_DEVINFO_DATA
};

fn get_device_registry_property_string(
    hdevinfo: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
    property: u32,
) -> Result<String, DeviceError> {
    let mut required_size: u32 = 0;
    let mut property_type: u32 = 0;

    // Phase 1: Get buffer size
    unsafe {
        let _ = SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            property,
            Some(&mut property_type),
            None,
            &mut required_size,
        );
    }

    if required_size == 0 {
        return Ok(String::new()); // Property doesn't exist or is empty
    }

    // Phase 2: Allocate + read
    let mut buffer = vec![0u16; (required_size as usize + 1) / 2];
    let result = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            property,
            Some(&mut property_type),
            Some(buffer.as_mut_ptr() as *mut u8),
            &mut required_size,
        )
    };

    if result.is_err() {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        return Err(DeviceError::Win32 {
            api: "SetupDiGetDeviceRegistryPropertyW",
            last_error: err.0,
        });
    }

    // Convert UTF-16 → UTF-8
    let null_pos = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..null_pos]).map_err(|_| DeviceError::InvalidString)
}
```

**Pattern applies to:** `SPDRP_SERVICE`, `SPDRP_CLASS`, `SPDRP_DEVICEDESC`, `DEVPKEY_*` properties (via `SetupDiGetDevicePropertyW`), `SetupDiGetDeviceInstanceIdW`. All use the same two-phase read.

---

### 3. Multi-String Parsing (SPDRP_HARDWAREID, SPDRP_COMPATIBLEIDS)

```rust
fn get_hardware_ids(
    hdevinfo: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
) -> Result<Vec<String>, DeviceError> {
    let mut required_size: u32 = 0;
    let mut property_type: u32 = 0;

    unsafe {
        let _ = SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            SPDRP_HARDWAREID,
            Some(&mut property_type),
            None,
            &mut required_size,
        );
    }

    if required_size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u16; (required_size as usize + 1) / 2];
    let result = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            SPDRP_HARDWAREID,
            Some(&mut property_type),
            Some(buffer.as_mut_ptr() as *mut u8),
            &mut required_size,
        )
    };

    if result.is_err() {
        return Ok(Vec::new());
    }

    // Parse double-null-terminated list
    let mut ids = Vec::new();
    let mut start = 0;
    for i in 0..buffer.len() {
        if buffer[i] == 0 {
            if i == start {
                break; // Double null terminator
            }
            if let Ok(s) = String::from_utf16(&buffer[start..i]) {
                ids.push(s);
            }
            start = i + 1;
        }
    }

    Ok(ids)
}
```

**Multi-sz format:** List of null-terminated strings, terminated by an extra null. Loop until double-null encountered.

---

### 4. Enumeration Loop with EOF Detection

```rust
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiEnumDeviceInfo, SP_DEVINFO_DATA
};
use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;

fn enumerate_all_devices(hdevinfo: HDEVINFO) -> Vec<DeviceInfo> {
    let mut devices = Vec::new();
    let mut index = 0u32;

    loop {
        let mut devinfo_data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };

        let result = unsafe { SetupDiEnumDeviceInfo(hdevinfo, index, &mut devinfo_data) };

        if result.is_err() {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            if err == ERROR_NO_MORE_ITEMS {
                break; // Normal EOF
            }
            // Log error, skip this index
            index += 1;
            continue;
        }

        // Read properties, apply predicate, etc.
        // ...

        index += 1;
    }

    devices
}
```

**Key:** Check for `ERROR_NO_MORE_ITEMS` (not just any error). Other errors (e.g., device disappeared mid-enumeration) should skip index and continue.

---

### 5. Error Enum for Win32 Failures

```rust
#[derive(Debug)]
pub enum DeviceError {
    Win32 {
        api: &'static str,
        last_error: u32,
    },
    DeviceNotFound,
    InvalidString,
}
```

Always include the Win32 function name and `GetLastError()` value for debugging. Use `&'static str` for function names (zero-cost at runtime).

---

## Usage Example: Find Device by Predicate

```rust
fn find_keyboard_by_vid_pid(vid: u16, pid: u16) -> Result<String, DeviceError> {
    let devinfo_set = DeviceInfoSet::new(&GUID_DEVCLASS_KEYBOARD, DIGCF_PRESENT)?;

    let mut index = 0u32;
    loop {
        let mut devinfo_data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };

        let result = unsafe { SetupDiEnumDeviceInfo(devinfo_set.handle(), index, &mut devinfo_data) };

        if result.is_err() {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            index += 1;
            continue;
        }

        let hardware_ids = get_hardware_ids(devinfo_set.handle(), &devinfo_data)?;
        let target_hwid = format!("VID_{:04X}&PID_{:04X}", vid, pid);

        if hardware_ids.iter().any(|id| id.contains(&target_hwid)) {
            return get_device_instance_id(devinfo_set.handle(), &devinfo_data);
        }

        index += 1;
    }

    Err(DeviceError::DeviceNotFound)
}
```

---

## Gotchas

1. **cbSize initialization:** Every `SP_*` structure requires `cbSize = std::mem::size_of::<T>()`. Forgetting this causes `ERROR_INVALID_PARAMETER`.

2. **UTF-16 buffer sizing:** Windows reports sizes in *bytes*, but `Vec<u16>` is sized in *elements*. Divide by 2 (or use `(required_size + 1) / 2` for safety).

3. **HDEVINFO validity:** Check `hdevinfo.is_invalid()` after `SetupDiGetClassDevsW`. Invalid handles cannot be passed to `SetupDiDestroyDeviceInfoList`.

4. **Multi-sz EOF:** Must check for double-null (`buffer[i] == 0 && i == start`), not just single null. Otherwise parser continues into uninitialized memory.

5. **Property non-existence:** If a property doesn't exist, `required_size` is 0 and `GetLastError()` may be `ERROR_INSUFFICIENT_BUFFER`. Treat 0-size as "property absent" (return empty string / empty vec).

---

## When to Use This Pattern

- Enumerating devices by class GUID (keyboard, mouse, HID, network adapters, etc.)
- Filtering devices by HardwareId, InstanceId, Parent, Service, Class, etc.
- Reading device state (`SPDRP_CONFIGFLAGS`, `DEVPKEY_Device_DevNodeStatus`)
- Targeting devices for `SetupDiCallClassInstaller` operations (enable/disable/restart)

**Not for:** Driver installation / inf parsing (different SetupAPI subset). This pattern is for *enumeration + property reads* only.

---

## Dependencies (Cargo.toml)

```toml
[dependencies]
windows = { version = "0.58", features = [
    "Win32_Devices_DeviceAndDriverInstallation",
    "Win32_Foundation",
] }
log = "0.4"
```

---

## Testing Strategy

Generate fixture from real hardware via `pnputil`:

```powershell
pnputil /enum-devices /class <ClassName> /json > fixtures/devices.json
```

Parse JSON, assert predicate selects expected device(s). Do NOT mock SetupAPI — Windows device tree is too complex to fake realistically. Use real hardware snapshots.

---

**Status:** Proven pattern. Extracted from `device.rs` after successful structural compilation. Ready for reuse in future SetupAPI-based Rust projects.
