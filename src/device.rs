//! Surface internal keyboard device control via SetupAPI.
//!
//! This module implements the critical path for disabling and enabling the Surface Laptop 7's
//! built-in keyboard using Windows SetupAPI (`SetupDiCallClassInstaller` with `DIF_PROPERTYCHANGE`).
//!
//! # Safety Contract
//!
//! 1. **Fail closed** — If predicate matches 0 or >1 devices, ENABLE is the safe fallback.
//! 2. **No cache** — `resolve` enumerates fresh every call; never trust stored handles.
//! 3. **Verify by read-back** — After every DISABLE, read `CONFIGFLAG_DISABLED` from registry
//!    to confirm operation succeeded. Do NOT infer state from API return code.
//! 4. **Worker-only (with exceptions)** — All functions in this module perform blocking
//!    SetupAPI calls and MUST be invoked from the worker thread. **Two exceptions:**
//!    - `main.rs` Quit fallback performs inline ENABLE when worker is stuck/dead
//!    - `--recover` argv path performs inline ENABLE without worker thread
//!
//!    These exceptions share the same code path and are safe because they happen during
//!    shutdown/recovery, not during normal operation.
//!
//! # References
//!
//! - ARCHITECTURE.md — SetupAPI mechanism, 3-clause predicate, `apply_policy` contract,
//!   threading model, disable+verify atomicity, public surface.

use log::{error, info, warn};
use std::ptr;
use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Locate_DevNodeW, CM_Reenumerate_DevNode, SetupDiCallClassInstaller,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, SetupDiSetClassInstallParamsW, CM_LOCATE_DEVNODE_NORMAL,
    CM_REENUMERATE_SYNCHRONOUS, DIF_PROPERTYCHANGE, DIGCF_PRESENT, HDEVINFO, SPDRP_CONFIGFLAGS,
    SPDRP_SERVICE, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_Parent, DEVPROPTYPE, DEVPROP_TYPE_STRING,
};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE,
    KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_VALUE_TYPE,
};

// SetupAPI GUID_DEVCLASS_KEYBOARD = {4d36e96b-e325-11ce-bfc1-08002be10318}
const GUID_DEVCLASS_KEYBOARD: windows::core::GUID =
    windows::core::GUID::from_u128(0x4d36e96b_e325_11ce_bfc1_08002be10318);

/// ConfigFlags registry value bit indicating device is disabled.
/// `CONFIGFLAG_DISABLED` persists across reboot.
const CONFIGFLAG_DISABLED: u32 = 0x00000001;

/// DICS_FLAG_GLOBAL = 0x00000001 ("all hardware profiles" — legacy concept, required )
const DICS_FLAG_GLOBAL: u32 = 0x00000001;

/// State change operations for SP_PROPCHANGE_PARAMS.StateChange
const DICS_PROPCHANGE: u32 = 0x00000003; // Re-read config and reapply

/// Represents the target keyboard device for enable/disable operations.
///
/// Returned by `resolve()` on successful 1-match predicate and passed to
/// `enable()` / `disable()` / `current_state()`.
#[derive(Debug, Clone)]
pub struct Target {
    /// PnP InstanceId — regenerates on re-enumeration / hwid-invariants.md I3.
    /// Used for targeting SetupAPI calls.
    pub instance_id: String,

    /// HardwareIds captured for diagnostics. The predicate requires substring `VID_045E&PID_006C`.
    #[allow(dead_code)]
    pub hardware_ids: Vec<String>,

    /// Parent device path. The predicate requires prefix `{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM`.
    pub parent: String,

    /// Service name. The predicate requires exact match `"kbdhid"`.
    pub service: String,
}

/// Keyboard enabled/disabled state, read from CONFIGFLAG_DISABLED registry bit.
///
/// State is verified by reading back after every DISABLE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardState {
    Enabled,
    Disabled,
}

/// Result of `enable()` operation indicating pre-enable state.
///
/// Used for crash detection: if device was disabled at startup and we had to enable it,
/// that indicates the previous instance crashed without cleaning up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnableOutcome {
    /// Device was already enabled (ConfigFlags was 0x00000000 before enable).
    WasAlreadyEnabled,
    /// Device was disabled (ConfigFlags had CONFIGFLAG_DISABLED set before enable).
    WasDisabled,
}

/// Result of `resolve()` predicate matching.
///
/// Exactly one match is required. 0 or >1 matches produce diagnostic dumps.
#[derive(Debug)]
pub enum ResolveResult {
    /// Predicate matched exactly one device. Safe to disable.
    Ok(Target),

    /// Predicate matched zero devices. `dump` contains enumeration of all keyboard-class devices
    /// for diagnostics (written to log by caller ).
    NoMatch {
        #[allow(dead_code)]
        dump: String,
    },

    /// Predicate matched multiple devices. Caller MUST fall through to ENABLE (fail closed ).
    /// `candidates` lists all matches; `dump` contains full enumeration.
    MultipleMatches {
        candidates: Vec<Target>,
        #[allow(dead_code)]
        dump: String,
    },

    /// SetupAPI enumeration failed. Caller falls through to ENABLE (fail closed).
    EnumerationError(String),
}

/// Errors from enable/disable/current_state operations.
#[derive(Debug)]
pub enum DeviceError {
    /// Win32 API call failed. `api` is the function name; `last_error` is GetLastError() value.
    Win32 { api: &'static str, last_error: u32 },

    /// Target device's InstanceId not found during operation (re-enumeration edge case).
    DeviceNotFound,

    /// UTF-16 conversion failed (should be rare; Windows PnP strings are always valid UTF-16).
    InvalidString,
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceError::Win32 { api, last_error } => {
                write!(f, "{} failed: error 0x{:08X}", api, last_error)
            }
            DeviceError::DeviceNotFound => write!(f, "Target device not found"),
            DeviceError::InvalidString => write!(f, "Invalid UTF-16 string conversion"),
        }
    }
}

/// RAII wrapper for HDEVINFO. Ensures `SetupDiDestroyDeviceInfoList` is called on all paths.
struct DeviceInfoSet(HDEVINFO);

impl DeviceInfoSet {
    fn new(
        class_guid: &windows::core::GUID,
        flags: windows::Win32::Devices::DeviceAndDriverInstallation::SETUP_DI_GET_CLASS_DEVS_FLAGS,
    ) -> Result<Self, DeviceError> {
        let hdevinfo = unsafe { SetupDiGetClassDevsW(Some(class_guid), None, None, flags) }
            .map_err(|e| {
                let code = e.code().0 as u32;
                error!("SetupDiGetClassDevsW failed: error=0x{:08X}", code);
                DeviceError::Win32 {
                    api: "SetupDiGetClassDevsW",
                    last_error: code,
                }
            })?;

        if hdevinfo.is_invalid() {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            error!(
                "SetupDiGetClassDevsW returned invalid handle: error={} (0x{:08X})",
                err.0, err.0
            );
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

/// Helper to read a registry property as a wide string.
fn get_device_registry_property_string(
    hdevinfo: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
    property: windows::Win32::Devices::DeviceAndDriverInstallation::SETUP_DI_REGISTRY_PROPERTY,
) -> Result<String, DeviceError> {
    let mut required_size: u32 = 0;
    let mut property_type: u32 = 0;

    // First call to get required buffer size
    unsafe {
        let _ = SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            property,
            Some(&mut property_type),
            None,
            Some(&mut required_size),
        );
    }

    if required_size == 0 {
        // Property doesn't exist or is empty
        return Ok(String::new());
    }

    let mut buffer = vec![0u16; (required_size as usize).div_ceil(2)];
    let buffer_bytes =
        unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 2) };

    let result = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            property,
            Some(&mut property_type),
            Some(buffer_bytes),
            Some(&mut required_size),
        )
    };

    if result.is_err() {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        return Err(DeviceError::Win32 {
            api: "SetupDiGetDeviceRegistryPropertyW",
            last_error: err.0,
        });
    }

    // Find the null terminator and convert to String
    let null_pos = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..null_pos]).map_err(|_| DeviceError::InvalidString)
}

/// Helper to read SPDRP_HARDWAREID (multi-string) as Vec<String>.
fn get_hardware_ids(
    hdevinfo: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
) -> Result<Vec<String>, DeviceError> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SPDRP_HARDWAREID;

    let mut required_size: u32 = 0;
    let mut property_type: u32 = 0;

    // First call to get required buffer size
    unsafe {
        let _ = SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            SPDRP_HARDWAREID,
            Some(&mut property_type),
            None,
            Some(&mut required_size),
        );
    }

    if required_size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u16; (required_size as usize).div_ceil(2)];
    let buffer_bytes =
        unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 2) };

    let result = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            hdevinfo,
            devinfo_data,
            SPDRP_HARDWAREID,
            Some(&mut property_type),
            Some(buffer_bytes),
            Some(&mut required_size),
        )
    };

    if result.is_err() {
        return Ok(Vec::new());
    }

    // SPDRP_HARDWAREID is a multi-sz (double-null-terminated list of null-terminated strings)
    let mut ids = Vec::new();
    let mut start = 0;
    for i in 0..buffer.len() {
        if buffer[i] == 0 {
            if i == start {
                // Double null terminator
                break;
            }
            if let Ok(s) = String::from_utf16(&buffer[start..i]) {
                ids.push(s);
            }
            start = i + 1;
        }
    }

    Ok(ids)
}

/// Helper to read DEVPKEY_Device_Parent property.
fn get_device_parent(
    hdevinfo: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
) -> Result<String, DeviceError> {
    let mut property_type: DEVPROPTYPE = DEVPROP_TYPE_STRING;
    let mut required_size: u32 = 0;

    // First call to get required buffer size
    let size_result = unsafe {
        windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW(
            hdevinfo,
            devinfo_data,
            &DEVPKEY_Device_Parent,
            &mut property_type,
            None,
            Some(&mut required_size as *mut u32),
            0,
        )
    };

    // First call may fail with insufficient buffer - that's expected
    if required_size == 0 {
        if size_result.is_err() {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            warn!(
                "SetupDiGetDevicePropertyW(Parent) size query failed: error=0x{:08X}",
                err.0
            );
        }
        return Ok(String::new());
    }

    let mut buffer = vec![0u8; required_size as usize];
    let mut actual_size = required_size;

    // NOTE: The final parameter is `flags` (reserved — MUST be 0). Passing
    // anything else yields ERROR_INVALID_FLAGS (0x3EC). The buffer size is
    // communicated via the slice length, not this argument.
    let result = unsafe {
        windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW(
            hdevinfo,
            devinfo_data,
            &DEVPKEY_Device_Parent,
            &mut property_type,
            Some(&mut buffer),
            Some(&mut actual_size as *mut u32),
            0,
        )
    };

    if let Err(e) = result {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        warn!(
            "SetupDiGetDevicePropertyW(Parent) data read failed: error=0x{:08X}, windows_error={:?}",
            err.0, e
        );
        return Ok(String::new());
    }

    // Verify property type is string
    if property_type != DEVPROP_TYPE_STRING {
        warn!(
            "SetupDiGetDevicePropertyW(Parent) returned unexpected type: 0x{:08X} (expected DEVPROP_TYPE_STRING)",
            { property_type.0 }
        );
        return Ok(String::new());
    }

    // Property is wide string, interpret buffer as u16 array
    let u16_slice =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u16, buffer.len() / 2) };
    let null_pos = u16_slice
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(u16_slice.len());
    String::from_utf16(&u16_slice[..null_pos]).map_err(|_| DeviceError::InvalidString)
}

/// Helper to get device InstanceId.
fn get_device_instance_id(
    hdevinfo: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
) -> Result<String, DeviceError> {
    let mut required_size: u32 = 0;

    // First call to get required buffer size
    unsafe {
        let _ = windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceInstanceIdW(
            hdevinfo,
            devinfo_data,
            None,
            Some(&mut required_size),
        );
    }

    if required_size == 0 {
        return Err(DeviceError::Win32 {
            api: "SetupDiGetDeviceInstanceIdW",
            last_error: 0,
        });
    }

    let mut buffer = vec![0u16; required_size as usize];
    let result = unsafe {
        windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceInstanceIdW(
            hdevinfo,
            devinfo_data,
            Some(&mut buffer),
            Some(&mut required_size),
        )
    };

    if result.is_err() {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        return Err(DeviceError::Win32 {
            api: "SetupDiGetDeviceInstanceIdW",
            last_error: err.0,
        });
    }

    let null_pos = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..null_pos]).map_err(|_| DeviceError::InvalidString)
}

/// Internal structure representing a candidate device during enumeration.
struct CandidateInfo {
    instance_id: String,
    hardware_ids: Vec<String>,
    parent: String,
    service: String,
    config_flags: u32,
}

/// The 3-clause predicate (see ARCHITECTURE.md).
///
/// All three clauses must hold:
/// 1. Service == "kbdhid"
/// 2. HardwareIds contains substring "VID_045E&PID_006C"
/// 3. Parent starts with "{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM"
///
/// This function is factored out for unit-test fixture validation.
/// Fixture generation: `pnputil /enum-devices /class Keyboard /json` on target Surface.
fn matches(candidate: &CandidateInfo) -> bool {
    // Clause 1: Service == "kbdhid"
    if candidate.service != "kbdhid" {
        return false;
    }

    // Clause 2: HardwareIds contains substring "VID_045E&PID_006C"
    let has_vid_pid = candidate
        .hardware_ids
        .iter()
        .any(|id| id.contains("VID_045E&PID_006C"));
    if !has_vid_pid {
        return false;
    }

    // Clause 3: Parent starts with SAM-bus GUID + Target_SAM
    const SAM_PREFIX: &str = "{2DEDC554-A829-42AB-90E9-E4E4B4772981}\\Target_SAM";
    if !candidate
        .parent
        .to_uppercase()
        .starts_with(&SAM_PREFIX.to_uppercase())
    {
        return false;
    }

    true
}

/// Enumerate all keyboard-class devices and apply the 3-clause predicate.
///
/// Exactly one match is required. Returns:
/// - `ResolveResult::Ok(Target)` if predicate matches exactly 1 device
/// - `ResolveResult::NoMatch` if 0 matches (with full enumeration dump)
/// - `ResolveResult::MultipleMatches` if >1 matches (with candidates + dump, fail closed)
/// - `ResolveResult::EnumerationError` if SetupAPI fails
///
/// **No caching.** This function enumerates fresh every call.
///
/// Diagnostic dump format (one line per keyboard-class device):
/// `instance_id | Service=<service> | HardwareIds=[<ids>] | Parent=<parent> | ConfigFlags=0x<hex>`
pub fn resolve() -> ResolveResult {
    let devinfo_set = match DeviceInfoSet::new(&GUID_DEVCLASS_KEYBOARD, DIGCF_PRESENT) {
        Ok(set) => set,
        Err(e) => {
            error!("Failed to create device info set: {:?}", e);
            return ResolveResult::EnumerationError(format!("{:?}", e));
        }
    };

    let mut candidates = Vec::new();
    let mut all_devices = Vec::new();
    let mut index = 0u32;

    loop {
        let mut devinfo_data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };

        let result =
            unsafe { SetupDiEnumDeviceInfo(devinfo_set.handle(), index, &mut devinfo_data) };

        if result.is_err() {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            if err == ERROR_NO_MORE_ITEMS {
                break; // End of enumeration
            }
            error!("SetupDiEnumDeviceInfo failed at index {}: {:?}", index, err);
            index += 1;
            continue;
        }

        // Read all properties needed for predicate + dump
        let instance_id = match get_device_instance_id(devinfo_set.handle(), &devinfo_data) {
            Ok(id) => id,
            Err(_) => {
                index += 1;
                continue;
            }
        };

        let service =
            get_device_registry_property_string(devinfo_set.handle(), &devinfo_data, SPDRP_SERVICE)
                .unwrap_or_default();

        let hardware_ids =
            get_hardware_ids(devinfo_set.handle(), &devinfo_data).unwrap_or_default();

        let parent = get_device_parent(devinfo_set.handle(), &devinfo_data).unwrap_or_default();

        // Read CONFIGFLAGS for dump
        let mut config_flags: u32 = 0;
        let mut property_type: u32 = 0;
        let mut required_size: u32 = 0;
        let config_flags_bytes = unsafe {
            std::slice::from_raw_parts_mut(
                &mut config_flags as *mut u32 as *mut u8,
                std::mem::size_of::<u32>(),
            )
        };
        unsafe {
            let _ = SetupDiGetDeviceRegistryPropertyW(
                devinfo_set.handle(),
                &devinfo_data,
                SPDRP_CONFIGFLAGS,
                Some(&mut property_type),
                Some(config_flags_bytes),
                Some(&mut required_size),
            );
        }

        let candidate = CandidateInfo {
            instance_id: instance_id.clone(),
            hardware_ids: hardware_ids.clone(),
            parent: parent.clone(),
            service: service.clone(),
            config_flags,
        };

        // Apply predicate
        if matches(&candidate) {
            candidates.push(Target {
                instance_id: instance_id.clone(),
                hardware_ids: hardware_ids.clone(),
                parent: parent.clone(),
                service: service.clone(),
            });
        }

        // Store for dump
        all_devices.push(candidate);

        index += 1;
    }

    // Generate diagnostic dump (one line per device)
    let mut dump = String::new();
    for dev in &all_devices {
        let hw_ids_str = dev.hardware_ids.join(", ");
        dump.push_str(&format!(
            "{} | Service={} | HardwareIds=[{}] | Parent={} | ConfigFlags=0x{:08X}\n",
            dev.instance_id, dev.service, hw_ids_str, dev.parent, dev.config_flags
        ));
    }

    match candidates.len() {
        0 => {
            warn!("resolve: no devices matched 3-clause predicate");
            ResolveResult::NoMatch { dump }
        }
        1 => {
            let target = candidates.into_iter().next().unwrap();
            info!(
                "resolve: matched instance_id={}, service={}, parent={}",
                target.instance_id, target.service, target.parent
            );
            ResolveResult::Ok(target)
        }
        _ => {
            warn!(
                "resolve: predicate matched {} devices (fail closed)",
                candidates.len()
            );
            ResolveResult::MultipleMatches { candidates, dump }
        }
    }
}

/// Helper to find a device by InstanceId in the current enumeration.
fn find_device_by_instance_id(
    hdevinfo: HDEVINFO,
    instance_id: &str,
) -> Result<SP_DEVINFO_DATA, DeviceError> {
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
                return Err(DeviceError::DeviceNotFound);
            }
            index += 1;
            continue;
        }

        let current_id = get_device_instance_id(hdevinfo, &devinfo_data)?;
        if current_id.eq_ignore_ascii_case(instance_id) {
            return Ok(devinfo_data);
        }

        index += 1;
    }
}

/// Write CONFIGFLAG_DISABLED bit to device registry.
///
/// Opens `HKLM\SYSTEM\CurrentControlSet\Enum\{instance_id}` and modifies the `ConfigFlags`
/// REG_DWORD value (creating it if missing, default 0). Sets or clears the CONFIGFLAG_DISABLED
/// bit without disturbing other bits.
///
/// This is the registry-based disable mechanism required for Surface devices that block
/// DICS_DISABLE operations with ERROR_NOT_DISABLEABLE (0xE0000231).
fn write_config_flag(instance_id: &str, disabled: bool) -> Result<EnableOutcome, DeviceError> {
    let registry_path = format!("SYSTEM\\CurrentControlSet\\Enum\\{}", instance_id);
    let registry_path_wide: Vec<u16> = registry_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut hkey = HKEY::default();
    let open_result = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(registry_path_wide.as_ptr()),
            0,
            KEY_READ | KEY_SET_VALUE,
            &mut hkey,
        )
    };

    if open_result != ERROR_SUCCESS {
        error!(
            "RegOpenKeyExW failed for {}: error={} (0x{:08X})",
            instance_id, open_result.0, open_result.0
        );
        return Err(DeviceError::Win32 {
            api: "RegOpenKeyExW",
            last_error: open_result.0,
        });
    }

    let value_name: Vec<u16> = "ConfigFlags"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut old_flags: u32 = 0;
    let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
    let mut value_type = REG_VALUE_TYPE(0);

    let query_result = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            Some(ptr::null_mut()),
            Some(&mut value_type),
            Some(&mut old_flags as *mut u32 as *mut u8),
            Some(&mut data_size),
        )
    };

    if query_result != ERROR_SUCCESS && query_result.0 != 2 {
        // ERROR_FILE_NOT_FOUND (2) is OK - value doesn't exist yet
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        error!(
            "RegQueryValueExW(ConfigFlags) failed for {}: error={} (0x{:08X})",
            instance_id, query_result.0, query_result.0
        );
        return Err(DeviceError::Win32 {
            api: "RegQueryValueExW",
            last_error: query_result.0,
        });
    }

    let was_disabled = (old_flags & CONFIGFLAG_DISABLED) != 0;

    let new_flags = if disabled {
        old_flags | CONFIGFLAG_DISABLED
    } else {
        old_flags & !CONFIGFLAG_DISABLED
    };

    if new_flags == old_flags {
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        info!(
            "config_flags: {} unchanged at 0x{:08X}",
            instance_id, old_flags
        );
        let outcome = if was_disabled {
            EnableOutcome::WasDisabled
        } else {
            EnableOutcome::WasAlreadyEnabled
        };
        return Ok(outcome);
    }

    let set_result = unsafe {
        let data_bytes = std::slice::from_raw_parts(
            &new_flags as *const u32 as *const u8,
            std::mem::size_of::<u32>(),
        );
        RegSetValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            0,
            REG_DWORD,
            Some(data_bytes),
        )
    };

    unsafe {
        let _ = RegCloseKey(hkey);
    }

    if set_result != ERROR_SUCCESS {
        error!(
            "RegSetValueExW(ConfigFlags) failed for {}: error={} (0x{:08X})",
            instance_id, set_result.0, set_result.0
        );
        return Err(DeviceError::Win32 {
            api: "RegSetValueExW",
            last_error: set_result.0,
        });
    }

    info!(
        "config_flags: {} 0x{:08X} -> 0x{:08X}",
        instance_id, old_flags, new_flags
    );

    let outcome = if was_disabled {
        EnableOutcome::WasDisabled
    } else {
        EnableOutcome::WasAlreadyEnabled
    };
    Ok(outcome)
}

/// Trigger PnP re-evaluation after registry change.
///
/// First tries DICS_PROPCHANGE via SetupDiCallClassInstaller (fast, lightweight).
/// Falls back to CM_Reenumerate_DevNode if that fails (more intrusive but reliable).
///
/// Logs both attempts; returns Ok if either succeeds. Registry change is authoritative,
/// so failure here is non-fatal (device will update on next reboot).
fn trigger_reeval(
    devinfo_set: HDEVINFO,
    devinfo_data: &SP_DEVINFO_DATA,
    instance_id: &str,
) -> Result<(), DeviceError> {
    // Try DICS_PROPCHANGE first
    #[repr(C)]
    struct PropChangeParams {
        class_install_header:
            windows::Win32::Devices::DeviceAndDriverInstallation::SP_CLASSINSTALL_HEADER,
        state_change: u32,
        scope: u32,
        hw_profile: u32,
    }

    let params = PropChangeParams {
        class_install_header:
            windows::Win32::Devices::DeviceAndDriverInstallation::SP_CLASSINSTALL_HEADER {
                cbSize: std::mem::size_of::<
                    windows::Win32::Devices::DeviceAndDriverInstallation::SP_CLASSINSTALL_HEADER,
                >() as u32,
                InstallFunction: DIF_PROPERTYCHANGE,
            },
        state_change: DICS_PROPCHANGE,
        scope: DICS_FLAG_GLOBAL,
        hw_profile: 0,
    };

    let set_result = unsafe {
        SetupDiSetClassInstallParamsW(
            devinfo_set,
            Some(devinfo_data),
            Some(&params.class_install_header),
            std::mem::size_of::<PropChangeParams>() as u32,
        )
    };

    if set_result.is_ok() {
        let call_result = unsafe {
            SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, devinfo_set, Some(devinfo_data))
        };

        if call_result.is_ok() {
            info!(
                "trigger_reeval: DICS_PROPCHANGE succeeded for {}",
                instance_id
            );
            return Ok(());
        } else {
            let err = unsafe { windows::Win32::Foundation::GetLastError() };
            warn!(
                "trigger_reeval: DICS_PROPCHANGE failed for {}: error={} (0x{:08X}), trying CM_Reenumerate",
                instance_id, err.0, err.0
            );
        }
    } else {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        warn!(
            "trigger_reeval: SetClassInstallParams failed for {}: error={} (0x{:08X}), trying CM_Reenumerate",
            instance_id, err.0, err.0
        );
    }

    // Fallback: CM_Reenumerate_DevNode
    let instance_id_wide: Vec<u16> = instance_id
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut devnode: u32 = 0;

    let locate_result = unsafe {
        CM_Locate_DevNodeW(
            &mut devnode,
            PCWSTR(instance_id_wide.as_ptr()),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    };

    if locate_result.0 != 0 {
        // CR_SUCCESS = 0
        error!(
            "trigger_reeval: CM_Locate_DevNodeW failed for {}: CONFIGRET=0x{:08X}",
            instance_id, locate_result.0
        );
        return Err(DeviceError::Win32 {
            api: "CM_Locate_DevNodeW",
            last_error: locate_result.0,
        });
    }

    let reenumerate_result = unsafe { CM_Reenumerate_DevNode(devnode, CM_REENUMERATE_SYNCHRONOUS) };

    if reenumerate_result.0 != 0 {
        error!(
            "trigger_reeval: CM_Reenumerate_DevNode failed for {}: CONFIGRET=0x{:08X}",
            instance_id, reenumerate_result.0
        );
        return Err(DeviceError::Win32 {
            api: "CM_Reenumerate_DevNode",
            last_error: reenumerate_result.0,
        });
    }

    info!(
        "trigger_reeval: CM_Reenumerate_DevNode succeeded for {}",
        instance_id
    );
    Ok(())
}

/// Enable the target keyboard device.
///
/// Uses registry-based approach: clears CONFIGFLAG_DISABLED bit in device's ConfigFlags,
/// then triggers PnP re-evaluation via DICS_PROPCHANGE or CM_Reenumerate_DevNode.
///
/// **Worker thread only**, except for Quit fallback and `--recover` inline paths.
pub fn enable(target: &Target) -> Result<EnableOutcome, DeviceError> {
    let outcome = write_config_flag(&target.instance_id, false)?;

    let devinfo_set = DeviceInfoSet::new(&GUID_DEVCLASS_KEYBOARD, DIGCF_PRESENT)?;
    let devinfo_data = find_device_by_instance_id(devinfo_set.handle(), &target.instance_id)?;

    // Try to trigger re-evaluation, but registry write is authoritative
    if let Err(e) = trigger_reeval(devinfo_set.handle(), &devinfo_data, &target.instance_id) {
        warn!(
            "enable: trigger_reeval failed for {} (non-fatal): {:?}",
            target.instance_id, e
        );
    }

    info!("enable: device {} enabled successfully", target.instance_id);
    Ok(outcome)
}

/// Disable the target keyboard device.
///
/// Uses registry-based approach: sets CONFIGFLAG_DISABLED bit in device's ConfigFlags,
/// then triggers PnP re-evaluation via DICS_PROPCHANGE or CM_Reenumerate_DevNode.
///
/// This registry approach is required for Surface devices that block DICS_DISABLE
/// operations with ERROR_NOT_DISABLEABLE (0xE0000231).
///
/// **Worker thread only**., caller MUST invoke `disable_and_verify` instead
/// of calling `disable()` + `current_state()` separately, to ensure atomicity of the
/// disable+verify operation (single WM_APP+3 message posted to main thread).
pub fn disable(target: &Target) -> Result<(), DeviceError> {
    write_config_flag(&target.instance_id, true)?;

    let devinfo_set = DeviceInfoSet::new(&GUID_DEVCLASS_KEYBOARD, DIGCF_PRESENT)?;
    let devinfo_data = find_device_by_instance_id(devinfo_set.handle(), &target.instance_id)?;

    // Try to trigger re-evaluation, but registry write is authoritative
    if let Err(e) = trigger_reeval(devinfo_set.handle(), &devinfo_data, &target.instance_id) {
        warn!(
            "disable: trigger_reeval failed for {} (non-fatal): {:?}",
            target.instance_id, e
        );
    }

    info!(
        "disable: device {} disabled successfully",
        target.instance_id
    );
    Ok(())
}

/// Read the current enabled/disabled state of the target keyboard device.
///
/// Reads `CONFIGFLAG_DISABLED` bit from device registry via
/// `SetupDiGetDeviceRegistryPropertyW(SPDRP_CONFIGFLAGS)`.
///
/// **Do NOT infer state from enable/disable return codes.** Always verify by reading back.
///
/// Re-enumerates the device (does not trust stored handle) "no cache" contract.
pub fn current_state(target: &Target) -> Result<KeyboardState, DeviceError> {
    let devinfo_set = DeviceInfoSet::new(&GUID_DEVCLASS_KEYBOARD, DIGCF_PRESENT)?;

    let devinfo_data = find_device_by_instance_id(devinfo_set.handle(), &target.instance_id)?;

    let mut config_flags: u32 = 0;
    let mut property_type: u32 = 0;
    let mut required_size: u32 = 0;

    let config_flags_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            &mut config_flags as *mut u32 as *mut u8,
            std::mem::size_of::<u32>(),
        )
    };

    let result = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            devinfo_set.handle(),
            &devinfo_data,
            SPDRP_CONFIGFLAGS,
            Some(&mut property_type),
            Some(config_flags_bytes),
            Some(&mut required_size),
        )
    };

    if result.is_err() {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        // If CONFIGFLAGS doesn't exist, treat as Enabled (default state)
        if err == ERROR_INSUFFICIENT_BUFFER || required_size == 0 {
            return Ok(KeyboardState::Enabled);
        }
        error!(
            "SetupDiGetDeviceRegistryPropertyW(CONFIGFLAGS) failed for {}: error={} (0x{:08X})",
            target.instance_id, err.0, err.0
        );
        return Err(DeviceError::Win32 {
            api: "SetupDiGetDeviceRegistryPropertyW",
            last_error: err.0,
        });
    }

    if config_flags & CONFIGFLAG_DISABLED != 0 {
        Ok(KeyboardState::Disabled)
    } else {
        Ok(KeyboardState::Enabled)
    }
}

/// Atomic disable+verify operation for worker thread.
///
/// `Cmd::Disable` performs both operations atomically and posts a single
/// `WM_APP+3` message. This is the ONLY way the worker performs disable operations.
///
/// Returns tuple: (disable_result, verify_result).
/// - Caller checks `verify_result` — if not `Ok(Disabled)`, must ENABLE and flip `desired_active=false`.
/// - The verify step re-reads `CONFIGFLAG_DISABLED` from registry, ensuring ground-truth state.
pub fn disable_and_verify(
    target: &Target,
) -> (Result<(), DeviceError>, Result<KeyboardState, DeviceError>) {
    let disable_result = disable(target);
    let verify_result = current_state(target);
    (disable_result, verify_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a target candidate that should MATCH the predicate.
    fn target_candidate() -> CandidateInfo {
        CandidateInfo {
            instance_id: "HID\\INTC6C1\\3&30C19A8C&0&0000".to_string(),
            hardware_ids: vec![
                "HID\\VID_045E&PID_006C&REV_0001&Col01".to_string(),
                "HID\\VID_045E&PID_006C&Col01".to_string(),
            ],
            parent: "{2DEDC554-A829-42AB-90E9-E4E4B4772981}\\Target_SAM".to_string(),
            service: "kbdhid".to_string(),
            config_flags: 0,
        }
    }

    #[test]
    fn test_matches_exact_target() {
        assert!(matches(&target_candidate()), "Exact target should match");
    }

    #[test]
    fn test_matches_wrong_service() {
        let mut c = target_candidate();
        c.service = "i8042prt".to_string();
        assert!(!matches(&c), "Wrong service should NOT match");
    }

    #[test]
    fn test_matches_wrong_vid_pid() {
        let mut c = target_candidate();
        c.hardware_ids = vec!["HID\\VID_045E&PID_0000&REV_0001".to_string()];
        assert!(!matches(&c), "Wrong VID/PID should NOT match");
    }

    #[test]
    fn test_matches_wrong_parent() {
        let mut c = target_candidate();
        c.parent = "USB\\ROOT_HUB30\\4&3B0D5C8&0&0".to_string();
        assert!(!matches(&c), "Wrong parent should NOT match");
    }

    #[test]
    fn test_matches_empty_hardware_ids() {
        let mut c = target_candidate();
        c.hardware_ids = vec![];
        assert!(!matches(&c), "Empty hardware IDs should NOT match");
    }

    #[test]
    fn test_matches_case_insensitive_parent() {
        let mut c = target_candidate();
        // SAM GUID prefix in lowercase should still match
        c.parent = "{2dedc554-a829-42ab-90e9-e4e4b4772981}\\target_sam".to_string();
        assert!(matches(&c), "Parent comparison should be case-insensitive");
    }

    #[test]
    fn test_matches_multiple_hwids() {
        let mut c = target_candidate();
        // Insert a non-matching hwid at the front; the second one should still match
        c.hardware_ids
            .insert(0, "HID\\VID_NOPE&PID_NOPE".to_string());
        assert!(
            matches(&c),
            "Should match if at least one hwid matches VID/PID"
        );
    }

    #[test]
    fn test_matches_partial_vid_pid() {
        let mut c = target_candidate();
        // VID only, no PID - should NOT match (requires both)
        c.hardware_ids = vec!["HID\\VID_045E".to_string()];
        assert!(!matches(&c), "VID without PID should NOT match");
    }
}
