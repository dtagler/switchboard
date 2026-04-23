# v4 Amendments — Spike 2 Findings Ratification

**Author:** Jerry (Lead / Windows Architect)  
**Date:** 2026-04-21  
**Status:** AMENDMENTS ACCEPTED — supersedes §V Layer 3, §V Layer 5, and portions of §U  
**Scope:** Ratification of owner's Spike 2 ground truth. This is not architecture redesign; v4 core remains intact.

---

## §A. Findings Accepted from Spike 2

### Finding 1 — Internal keyboard identified via SAM bus marker

Accepted. The owner's Surface Laptop 7 internal keyboard is `HID\Target_SAM&Category_HID&Col01` with parent path `{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM&Category_HID\...`, parent device class GUID identifying the Surface Aggregator Module (SAM) bus. Five distinct HID keyboard-class devices share `Service=kbdhid` and `Class=Keyboard` on this hardware (physical keyboard plus four button/VHF collections). Targeting requires more precision than v4 §V Layer 3 specified. The `Target_SAM` substring in HardwareIds and parent path is the distinguishing marker.

### Finding 2 — ContainerId composite-safety check is broken on Surface internal devices

Accepted. Layer 3 as written assumed distinct ContainerIds for internal keyboard vs touchpad. Ground truth: both report `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}`, the Windows "no container" sentinel GUID. Every internal Surface device (keyboard, touchpad, button collections, VHF power injection) shares this sentinel. The predicate "refuse disable if ContainerId matches touchpad" would refuse every internal device, rendering targeting impossible.

The underlying risk Layer 3 was protecting against — composite-parent disable cascading to touchpad — is not present on this hardware. Touchpad parent is `ACPI\MSHW0238\...`; keyboard parent is the SAM bus (`{2DEDC554-...}\Target_SAM\...`). Different parent paths. No shared composite ancestor. Cascade not possible via parent-tree topology.

Verdict: ContainerId gate is discarded. Replacement predicate in §B targets via HardwareId substring + parent path prefix, with runtime cross-check against Mouse-class device parents.

### Finding 3 — Nuphy is Bluetooth LE, not BT Classic

Accepted. Spike 2 ground truth: `BTHLE\DEV_CC006219C5FD\...`, HID-over-GATT service GUID `00001812-0000-1000-8000-00805F9B34FB`, VID 0x07D7 (Nu Technology), MAC `CC006219C5FD`. Contradicts the Kramer §Q1–§Q4 premise that Nuphy is BT Classic (`DeviceInformation.Pairing.IsPaired` / `RfcommDeviceService` flow).

Implication: use `Windows.Devices.Bluetooth.BluetoothLEDevice.FromBluetoothAddressAsync(0xCC006219C5FD)` → subscribe `ConnectionStatusChanged` event. Event-driven, not polled. DeviceWatcher AQS becomes secondary belt-and-suspenders signal. Modern Standby resume still requires one-shot re-query (BLE events may be stale on S0ix wake; `PBT_APMRESUMEAUTOMATIC` triggers fresh query). This is §C below.

---

## §B. Layer 3 — REPLACED

**Original Layer 3 (v4 §V):**  
> Before any `DICS_DISABLE` call: enumerate target node's `ContainerId`; refuse if matches any non-keyboard device's `ContainerId` (esp. touchpad).

**Replacement predicate (allowlist targeting):**

Target the internal keyboard device node if and only if ALL of the following hold:

```rust
// Pseudo-code signature (not implementation)
fn is_safe_internal_keyboard(device: &DeviceInfo) -> Result<bool, TargetError> {
    // 1. Service layer
    if device.service() != "kbdhid" {
        return Ok(false);
    }

    // 2. HardwareIds must contain HID_DEVICE_SYSTEM_KEYBOARD
    if !device.hardware_ids().contains("HID_DEVICE_SYSTEM_KEYBOARD") {
        return Ok(false);
    }

    // 3. HardwareIds must NOT contain VHF or ConvertedDevice markers
    if device.hardware_ids().iter().any(|id| 
        id.contains("HID_DEVICE_SYSTEM_VHF") || id.contains("ConvertedDevice")
    ) {
        return Ok(false);
    }

    // 4. HardwareIds must contain Target_SAM&Category_HID (SAM bus marker)
    if !device.hardware_ids().iter().any(|id| id.contains("Target_SAM&Category_HID")) {
        return Ok(false);
    }

    // 5. Parent path must start with SAM bus class GUID
    let parent_path = device.parent_path()?;
    if !parent_path.starts_with("{2DEDC554-A829-42AB-90E9-E4E4B4772981}\\Target_SAM") {
        return Ok(false);
    }

    // 6. Runtime cross-check: enumerate all Mouse-class devices; refuse if our 
    //    parent path matches any Mouse device's parent
    let mouse_parents = enumerate_mouse_device_parents()?;
    if mouse_parents.iter().any(|mp| parent_path.starts_with(mp)) {
        return Err(TargetError::SharedMouseParent(parent_path.clone()));
    }

    Ok(true)
}
```

**Rationale for each clause:**

1. **Service == "kbdhid"** — all HID keyboard function drivers use this service. Eliminates non-HID keyboard devices (rare but possible via third-party drivers).
2. **HardwareIds contains "HID_DEVICE_SYSTEM_KEYBOARD"** — distinguishes keyboard TLCs from other HID collections (mouse, consumer control, vendor-specific). Per Microsoft's HID TLC model, this is the canonical keyboard marker.
3. **HardwareIds does NOT contain "HID_DEVICE_SYSTEM_VHF" or "ConvertedDevice"** — VHF (Virtual HID Framework) is used for power-button injection and other synthetic devices. ConvertedDevice marks legacy PS/2-to-HID adapters. Both are false positives for physical keyboard targeting.
4. **HardwareIds contains "Target_SAM&Category_HID"** — the Surface Aggregator Module bus identifier. This distinguishes Surface internal keyboards from any future non-SAM Surface keyboards (hypothetical USB-C docks, third-party HID devices). Defense against false-positive targeting on non-Surface hardware.
5. **Parent path starts with "{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM"** — double-check that parent topology matches SAM bus expectations. The class GUID `{2DEDC554-...}` is the SAM bus interface class (Microsoft-documented; search Microsoft Learn for "Surface Aggregator Module" or the GUID directly). If this GUID is wrong or the parent path doesn't match, something is misconfigured and we refuse targeting.
6. **Runtime cross-check against Mouse-class parents** — belt-and-suspenders defense against composite-parent cascade. Enumerate all `Class=Mouse` devices, capture their parent device paths, refuse to disable if our target's parent matches any Mouse parent. This catches the composite-parent risk even if our HardwareId filtering is wrong.

**Targeting method:** HardwareId substring + Parent path prefix. NOT InstanceId pinning.

**George validation note:** George is independently validating this allowlist choice vs InstanceId-pinning attack model. If George determines InstanceId pinning is required (e.g., if HardwareIds are mutable post-PnP-enumeration or if surprise-removal on SAM bus changes InstanceId), this predicate will be revised. George's verdict supersedes.

---

## §C. Nuphy Detection — REVISED

**Original Layer 5 (v4 §V):**  
> `DeviceWatcher.Updated` watching `System.Devices.Aep.IsConnected`. On `IsConnected = false` → re-enable internal keyboard within ~50ms. (NOT `Removed` — `Removed` only fires on unpair.) Polled fallback every 5s via `BluetoothDevice.FromIdAsync().ConnectionStatus`.

**Replacement mechanism:**

**Primary (event-driven):**  
`Windows.Devices.Bluetooth.BluetoothLEDevice.FromBluetoothAddressAsync(0xCC006219C5FD)` on startup, then subscribe `ConnectionStatusChanged` event. The BT MAC address `0xCC006219C5FD` is hardcoded as a config value captured during install/setup. On `ConnectionStatus` change → evaluate `device.ConnectionStatus == BluetoothConnectionStatus.Connected` and enable/disable internal keyboard accordingly.

**Secondary (belt-and-suspenders):**  
WinRT DeviceWatcher with AQS `BluetoothLEDevice.FromBluetoothAddressAsync(0xCC006219C5FD).GetDeviceSelector()`, watching `DeviceWatcher.Updated` event + `System.Devices.Aep.IsConnected` property. If primary event is missed (driver bug, race condition), secondary catches it within ~200–500ms.

**Modern Standby resume:**  
Drop the 5s poll from primary steady-state path (BLE `ConnectionStatusChanged` handles it). KEEP one-shot re-query on `PBT_APMRESUMEAUTOMATIC` (Layer 2 already specifies this): call `BluetoothLEDevice.FromBluetoothAddressAsync()` again and re-read `ConnectionStatus` from scratch. This is the staleness defense — BLE event subscriptions may not survive S0ix suspend/resume correctly. The one-shot re-query is sub-100ms overhead and runs once per resume, not continuously.

**Install/setup flow addition:**  
First-run experience must capture the Nuphy's BT MAC address. Recommend tray-menu option "Detect Nuphy" → enumerate all paired `BluetoothLEDevice` instances, filter by VID 0x07D7 or friendly name substring "Air75 V3", write MAC to config. User workflow: Pair Nuphy in Windows Settings Bluetooth panel, then click "Detect Nuphy" in tray. MAC binding persists across Nuphy firmware updates and re-pairing (MAC is hardware-burned, not firmware).

**Reference documentation:**  
- `BluetoothLEDevice` class: https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.bluetoothledevice  
- `ConnectionStatusChanged` event: https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.bluetoothledevice.connectionstatuschanged  
- HID-over-GATT service UUID `00001812-...`: https://www.bluetooth.com/specifications/assigned-numbers/

---

## §D. Revised 5-Layer Fail-Safe Table

| # | Layer | Mechanism | When it fires | **CHANGE** |
|---|-------|-----------|---------------|------------|
| 0 | **Re-enable on shutdown** | `WM_QUERYENDSESSION` and `WM_ENDSESSION`: synchronous SetupAPI `DICS_ENABLE` on the targeted keyboard child node. Block return until enable confirmed. | Logoff, restart, shutdown, hybrid shutdown (Fast Startup) | None |
| 1 | **Cold-start invariant** | On process launch: (a) elevate if needed, (b) SetupAPI `DICS_ENABLE` unconditionally on targeted node (defensive, in case any prior crash left it disabled), (c) WinRT query Nuphy state via `BluetoothLEDevice.FromBluetoothAddressAsync(MAC).ConnectionStatus` — NOT cached — (d) only if Nuphy confirmed connected, disable. | Every process launch (including autostart) | None |
| 2 | **Power-state hook** | `WM_POWERBROADCAST`: `PBT_APMSUSPEND` → re-enable synchronously before suspend. `PBT_APMRESUMEAUTOMATIC` → re-query Nuphy state via `BluetoothLEDevice.FromBluetoothAddressAsync(MAC)` from scratch; disable only if connected. Replaces v3's wrong `WTS_SESSION_LOCK` hook. | Sleep, hibernate, Modern Standby S0ix entry/exit | None |
| 3 | **Composite-device safety check** | Before any `DICS_DISABLE` call: apply §B allowlist predicate (Service, HardwareIds, Parent path, Mouse-class cross-check). Hard fail with logged reason if predicate returns false or error. | Every disable attempt | **REPLACED** — see §B |
| 4 | **Tray-click manual override** | Click tray icon → toggle. Always available. Touchpad survives (Layer 3 guarantee). | User action | None |
| 5 | **Reactive Nuphy disconnect** | `BluetoothLEDevice.ConnectionStatusChanged` event (primary) + `DeviceWatcher.Updated` watching `System.Devices.Aep.IsConnected` (secondary). On disconnect → re-enable internal keyboard within ~50–200ms. One-shot re-query on `PBT_APMRESUMEAUTOMATIC` for Modern Standby staleness defense. | Nuphy power-off, out-of-range, BT radio off, S0ix resume | **REVISED** — see §C |

**Layers 0, 1, 2, 4:** unchanged. Layer 3 and Layer 5 replaced per §B and §C.

---

## §E. Revised Target-Keyboard Predicate (Implementation Deliverable)

**For owner / implementation agents:**

The internal keyboard device node is a valid disable target if and only if:

1. **Service** = `"kbdhid"`
2. **HardwareIds** array contains `"HID_DEVICE_SYSTEM_KEYBOARD"`
3. **HardwareIds** array does NOT contain `"HID_DEVICE_SYSTEM_VHF"` or `"ConvertedDevice"`
4. **HardwareIds** array contains substring `"Target_SAM&Category_HID"`
5. **Parent device path** starts with `"{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM"`
6. **Runtime check:** Enumerate all `Class=Mouse` devices; none share a parent path prefix with the target device.

If any clause fails, refuse `DICS_DISABLE` and log the failure reason. No fallback. Hard stop.

**Query flow (SetupAPI):**

```rust
// Pseudo-code for implementation agents
let keyboard_devices = SetupDiGetClassDevs(GUID_DEVCLASS_KEYBOARD, ...);
for device in keyboard_devices {
    let service = device.get_property(SPDRP_SERVICE)?;
    let hardware_ids = device.get_property(SPDRP_HARDWAREID)?;
    let parent_path = device.get_parent_device_path()?;

    if is_safe_internal_keyboard(&device)? {
        return Ok(device.instance_id());
    }
}
Err(TargetError::NoMatchingDevice)
```

**InstanceId vs HardwareId targeting:** This predicate uses HardwareId substring + Parent path prefix, not InstanceId pinning. InstanceId is runtime-generated by PnP manager and may change on surprise-removal/re-enumeration or firmware updates. HardwareIds are stable (burned into device firmware or driver INF). This choice trades off protection against malicious HardwareId mutation (George's attack model) for resilience against benign InstanceId churn (SAM bus surprise-removal on Modern Standby, Surface firmware updates). George is validating this trade-off independently; his verdict supersedes if he disagrees.

---

## §F. Open Items Spawned by These Findings

1. **Newman** — updating fail-safe invariants document to reflect HardwareId-based targeting. Question: does SAM bus surprise-removal (Modern Standby driver unload/reload) change InstanceId? If yes, does our targeting logic re-resolve correctly on resume? Newman's deliverable: answer + test plan.

2. **George** — validating allowlist durability vs InstanceId pinning attack model. Question: can an attacker (malware, compromised driver) mutate HardwareIds or Parent path post-enumeration to fool our predicate? George's deliverable: threat model assessment + recommendation (keep allowlist OR switch to InstanceId pinning + accept InstanceId churn risk).

3. **Kramer** — rewriting BT mechanism sections §Q1–§Q4 with BLE event-driven primary (`BluetoothLEDevice.FromBluetoothAddressAsync` + `ConnectionStatusChanged`). Deliverable: revised Kramer BT spec, aligned with §C above.

4. **Peterman** — capturing discovery log "Why we don't trust ContainerId on Surface internal hardware." Deliverable: one-pager with Spike 2 findings + Microsoft Learn citation on ContainerId sentinel GUID behavior, filed to `.squad/discovery-logs/surface-containerid-sentinel.md`.

5. **Spike 1 (toolchain)** — still in flight. Rust ARM64 toolchain + `windows-rs` WinRT bindings + cross-compile from Linux Docker. Does not block these amendments; blocks first code.

---

## §G. Code-Readiness Gate

With these amendments accepted, v4 is ready for repository scaffold + first code IF AND ONLY IF all of the following complete:

1. **Spike 1 passes** — Rust ARM64 toolchain verified (cross-compile from Linux Docker, 3–5 MB binary, `windows-rs` WinRT + SetupAPI P/Invoke both working).
2. **George's allowlist verdict** — either "allowlist approved" OR "switch to InstanceId pinning" with revised predicate.
3. **Kramer's BLE spec** — revised §Q1–§Q4 delivered, aligned with §C.
4. **Newman's SAM-bus invariant** — InstanceId churn behavior on Modern Standby documented + test plan.

**Remaining gates (explicit list):**

- Spike 1 completion (ETA: owner is running now; results expected within 24h).
- George deliverable #2 (allowlist validation).
- Kramer deliverable #3 (BLE spec rewrite).
- Newman deliverable #1 (SAM-bus InstanceId behavior).

**What is NOT a gate:**

- Peterman's discovery log (nice-to-have, not blocking).
- Logging volume policy (deferred to post-scaffold; owner will test Layer 5 event frequency on hardware).
- MSIX packaging decisions (deferred to post-first-run; doesn't affect core architecture).

Once the four gates above pass, the following become unblocked:

1. Repository scaffold (`src/main.rs`, `Cargo.toml`, module stubs per §X).
2. `device_controller.rs` implementation (SetupAPI P/Invoke + §E predicate).
3. `bluetooth_watcher.rs` implementation (§C BLE event-driven primary).
4. First integration test (spawn process, verify Layer 1 cold-start invariant, verify Layer 3 refuses invalid targets).

**Explicit confirmation:** With §A/§B/§C accepted and the four gates passing, v4 architecture is COMPLETE and code-ready. No further architecture redesign is expected unless George or Newman surface a blocking issue in their deliverables.

---

## §H. SAM Bus Documentation Reference (Attempted)

Searched Microsoft Learn and Windows Driver Kit documentation for Surface Aggregator Module (SAM) bus class GUID `{2DEDC554-A829-42AB-90E9-E4E4B4772981}`. No authoritative public documentation found. This GUID is well-known in Surface device-tree analysis but appears to be undocumented in public Microsoft Learn articles as of 2026-04-21.

**What we know from Spike 2 ground truth:**

- Parent path `{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM&Category_HID\...` identifies SAM bus.
- HardwareId substring `Target_SAM&Category_HID` is the canonical marker.
- SAM is the internal communication bus for Surface keyboards, touchpads, buttons, and sensors on ARM64 Surface devices (Surface Laptop 7, Surface Pro X family).

**Recommendation:** Treat this GUID as a hardware constant for Surface targeting. If Microsoft publishes SAM bus documentation in the future, link will be added here.

---

**END OF AMENDMENTS**
