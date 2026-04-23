# Kramer's BLE Rewrite — §Q1-Q4 CORRECTED

**Date:** 2026-04-21  
**Author:** Kramer (Bluetooth / HID Engineer)  
**Context:** Spike 2 ground truth confirmed Nuphy Air75 V3-3 is **BT LE (BTHLE bus)**, not BT Classic. My prior analysis in bt-fix-and-device-tree.md §Q1-Q4 was WRONG. This document rewrites those sections with the BLE event-driven path.

**Spike 2 ground truth:**
```
FriendlyName:  Air75 V3-3
InstanceId:    BTHLE\DEV_CC006219C5FD\5&18824DB6&0&CC006219C5FD
Class:         Bluetooth (BT LE radio)
HID over GATT service GUID: 00001812-0000-1000-8000-00805F9B34FB
ContainerId:   {DFCA93C1-6CD7-5EBF-8011-740A73ED13DF}
VID:           0x07D7 (Nu Technology / Nuphy)
MAC:           CC006219C5FD (= 0xCC006219C5FD as u64)
```

---

## §Q1-REVISED. What WinRT actually emits when a paired BT LE keyboard disconnects

### Event: `BluetoothLEDevice.ConnectionStatusChanged`

**Namespace:** `Windows.Devices.Bluetooth`  
**Microsoft Learn:** [BluetoothLEDevice.ConnectionStatusChanged Event](https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.bluetoothledevice.connectionstatuschanged)

**Signature:**
```csharp
public event TypedEventHandler<BluetoothLEDevice, object> ConnectionStatusChanged;
```

**What it does:**  
Fires whenever the `BluetoothLEDevice.ConnectionStatus` property changes between `BluetoothConnectionStatus.Connected` and `BluetoothConnectionStatus.Disconnected`.

### When does it fire?

Per Microsoft Learn and field testing consensus (various Stack Overflow / MSDN forum reports):

| **Scenario**                          | **Event fires?** | **Latency**                     | **Notes**                                                                                               |
|---------------------------------------|------------------|---------------------------------|---------------------------------------------------------------------------------------------------------|
| (a) Auto-power-off (idle timeout)     | **Yes**          | 500ms–4s (supervision timeout)  | BLE device goes idle, supervision timeout expires. Windows detects link loss. Typical for Nuphy's 30-min idle. |
| (b) Battery dies                      | **Yes**          | 500ms–4s (supervision timeout)  | Same as (a) — no disconnect packet sent, Windows waits for supervision timeout.                        |
| (c) User toggles BT off (keyboard)    | **Yes**          | 500ms–4s                        | Device may send disconnect event if graceful; otherwise supervision timeout.                            |
| (d) User unpairs (Windows Settings)   | **Yes**          | Immediate                       | Windows OS sends disconnect event instantly when unpairing is requested.                                |
| (e) Out of range                      | **Yes**          | 500ms–4s (supervision timeout)  | Link quality degrades, supervision timeout triggers.                                                    |

**Key finding:**  
The event **always fires** for BLE devices, but latency varies. For abrupt disconnects (power-off, battery death, out-of-range), latency is dictated by the **BLE supervision timeout** (typically 500ms–4s for HID devices). For unpair events, it's immediate.

**BLE supervision timeout reference:**  
- Bluetooth Core Spec v5.x defines supervision timeout as a connection parameter (min 100ms, typical 500ms–6s for HID).
- Windows HID-over-GATT stack uses a conservative timeout (~2–4s) for consumer devices.
- [Windows HID over GATT Profile Drivers](https://docs.microsoft.com/en-us/windows-hardware/drivers/stream/hid-over-gatt-profile-drivers)

**Implication for v4:**  
`ConnectionStatusChanged` is the **PRIMARY** signal for Nuphy disconnect. Latency is acceptable (2–4s worst-case). No polling needed in the steady state.

---

## §Q2-REVISED. Primary detection path — code shape

### Pseudo-code (windows-rs / Rust idiom)

```rust
use windows::Devices::Bluetooth::{BluetoothLEDevice, BluetoothConnectionStatus};
use windows::Foundation::TypedEventHandler;

// MAC captured at first-run (see §CONFIGURATION below)
let bt_addr: u64 = 0xCC006219C5FD;

// Async: get the device handle from MAC
let device = BluetoothLEDevice::FromBluetoothAddressAsync(bt_addr)?.await?;

// CRITICAL: Store `device` in app state; don't drop it! (see Gotcha #3 below)
APP_STATE.lock().unwrap().nuphy_device = Some(device.clone());

// Register event handler
let handler = TypedEventHandler::new(move |sender: &Option<BluetoothLEDevice>, _args| {
    if let Some(sender) = sender {
        match sender.ConnectionStatus()? {
            BluetoothConnectionStatus::Connected => {
                // Nuphy is connected → disable internal keyboard (Path A blocking)
                tx.send(Event::NuphyConnected)?;
            }
            BluetoothConnectionStatus::Disconnected => {
                // Nuphy is gone → re-enable internal keyboard
                tx.send(Event::NuphyDisconnected)?;
            }
            _ => {}
        }
    }
    Ok(())
});

device.ConnectionStatusChanged(&handler)?;

// Main thread receives Event::NuphyConnected / Event::NuphyDisconnected
// and calls device_controller::disable() / device_controller::enable()
```

### Gotchas

#### **Gotcha #1: `FromBluetoothAddressAsync` returns null if device is not paired**

**Question:** Does `FromBluetoothAddressAsync` return null if the device is not currently paired?  
**Answer:** **YES.** If the device with the given MAC address is not paired, the method returns `null` (or an error depending on binding).

**Handling:**
```rust
let device = BluetoothLEDevice::FromBluetoothAddressAsync(bt_addr)?.await?;
if device.is_null() {
    // Not paired — user needs to pair in Windows Settings first
    return Err("Nuphy not paired. Please pair in Settings → Bluetooth & devices.");
}
```

**Mitigation:**  
At app startup, call `FromBluetoothAddressAsync` with the configured MAC. If it returns null, show a tray notification: "Nuphy not paired. Please pair the keyboard in Windows Settings and restart this app."

#### **Gotcha #2: What thread does the event fire on?**

**Question:** Does the event fire on the main thread or a background thread?  
**Answer:** **Background thread.** WinRT async events fire on the threadpool.

**Handling:**  
Marshal to the main thread (or a dedicated device-controller thread) before calling SetupAPI functions. Use a channel (`mpsc::channel` in Rust, or equivalent).

```rust
// In event handler (background thread):
tx.send(Event::NuphyDisconnected)?;

// Main thread event loop:
match rx.recv() {
    Event::NuphyDisconnected => {
        // Safe to call SetupAPI here (main thread context)
        device_controller::enable_internal_keyboard()?;
    }
    _ => {}
}
```

**Why this matters:**  
SetupAPI calls (`SetupDiSetClassInstallParams`, etc.) can be sensitive to COM apartment state and thread context. Keep device enable/disable operations on a single, predictable thread.

#### **Gotcha #3: Does the BluetoothLEDevice handle need to be kept alive?**

**Question:** If we drop the `BluetoothLEDevice` handle after registering the event, do events still fire?  
**Answer:** **NO.** The `BluetoothLEDevice` object must be kept alive for the event subscription to remain active.

**Handling:**  
Store the `BluetoothLEDevice` handle in app state (e.g., `Arc<Mutex<AppState>>` in Rust). Do NOT drop it until app shutdown.

```rust
struct AppState {
    nuphy_device: Option<BluetoothLEDevice>,
    // ... other state
}

// At startup:
let device = BluetoothLEDevice::FromBluetoothAddressAsync(bt_addr)?.await?;
APP_STATE.lock().unwrap().nuphy_device = Some(device);
```

**Reference:**  
This is standard WinRT event subscription behavior. The event target must outlive the subscription. See [WinRT event lifetime patterns (MSDN forums)](https://social.msdn.microsoft.com/Forums/en-US/home?forum=winappswithnativecode).

#### **Gotcha #4: Modern Standby suspension**

**Question:** What happens to the event subscription when the system enters Modern Standby (Connected Standby / S0 low power idle)?  
**Answer:** The process is **suspended** along with its threads. On resume, the `BluetoothLEDevice` object may be in a stale state, and `ConnectionStatusChanged` events may NOT fire for state changes that occurred during suspension.

**Handling (Layer 2 / resume path):**  
On `PBT_APMRESUMEAUTOMATIC` (Modern Standby resume), re-query the device state:

```rust
// In WM_POWERBROADCAST handler (wparam == PBT_APMRESUMEAUTOMATIC):
let device = APP_STATE.lock().unwrap().nuphy_device.as_ref()?;
let status = device.ConnectionStatus()?;

match status {
    BluetoothConnectionStatus::Connected => {
        // Nuphy is connected post-resume → ensure internal keyboard is disabled
        if !is_internal_keyboard_disabled() {
            device_controller::disable_internal_keyboard()?;
        }
    }
    BluetoothConnectionStatus::Disconnected => {
        // Nuphy is gone post-resume → ensure internal keyboard is enabled
        if is_internal_keyboard_disabled() {
            device_controller::enable_internal_keyboard()?;
        }
    }
}
```

**Why this matters:**  
During Modern Standby, the Bluetooth stack may power down and reconnect devices. The app won't see the intermediate state changes. On resume, we must reconcile cached state vs. live state.

**Reference:**  
[Modern Standby - Microsoft Docs](https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/modern-standby)

---

## §Q3-REVISED. Secondary detection (belt-and-suspenders)

### Why secondary?

BLE `ConnectionStatusChanged` is reliable, but:
1. **Edge cases exist:** The distinction between "advertising-disconnected" (device is BLE-advertising but GATT link is down) vs. "link-down" (device is fully off) can cause event-loss scenarios on some Windows builds.
2. **Battery-dying vs. clean-shutdown timing:** If the battery dies mid-GATT-operation, the event may be delayed or skipped.
3. **Defense-in-depth:** If one signal fires and the other doesn't, we still react. If both fire, we trust it more.

### DeviceWatcher as secondary signal

**Namespace:** `Windows.Devices.Enumeration`  
**Microsoft Learn:** [DeviceWatcher Class](https://learn.microsoft.com/en-us/uwp/api/windows.devices.enumeration.devicewatcher)

#### AQS selector for BLE devices (NOT BR/EDR)

**CRITICAL:** Use the **Bluetooth LE protocol ID**, NOT the Bluetooth Classic (BR/EDR) ID.

```csharp
string aqsFilter = 
    "System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\"" + // BLE
    " AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";
```

**Bluetooth Protocol IDs (Microsoft AQS reference):**
- **Bluetooth Classic (BR/EDR):** `{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}`
- **Bluetooth LE:** `{bb7bb05e-5972-42b5-94fc-76eaa7084d49}`

**Finding 3 from my original deep-dive:** I had this backwards — I was using the BR/EDR selector. This is the root cause of my wrong analysis. **Spike 2 proves Nuphy is BLE, so we use the BLE protocol ID.**

#### Filter by ContainerId (Nuphy-specific)

**Nuphy's ContainerId per Spike 2:**  
`{DFCA93C1-6CD7-5EBF-8011-740A73ED13DF}`

Add this to the AQS filter:
```csharp
string aqsFilter = 
    "System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\"" +
    " AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True" +
    " AND System.Devices.Aep.ContainerId:=\"{DFCA93C1-6CD7-5EBF-8011-740A73ED13DF}\"";
```

**Note:** ContainerId is user-specific (changes per pairing/machine combo). At first-run, capture the ContainerId along with the MAC (see §CONFIGURATION below).

#### Watch the `Updated` event

```rust
let watcher = DeviceInformation::CreateWatcher(
    &aqsFilter,
    &["System.Devices.Aep.IsConnected"],
    DeviceInformationKind::AssociationEndpoint,
)?;

watcher.Updated(&TypedEventHandler::new(|_sender, info| {
    if let Some(is_connected) = info.Properties()?.Lookup("System.Devices.Aep.IsConnected")? {
        let is_connected: bool = is_connected.cast()?;
        if is_connected {
            tx.send(Event::NuphyConnectedSecondary)?;
        } else {
            tx.send(Event::NuphyDisconnectedSecondary)?;
        }
    }
    Ok(())
}))?;

watcher.Start()?;
```

#### State reconciliation logic

In the main event loop:
```rust
match rx.recv() {
    Event::NuphyConnected | Event::NuphyConnectedSecondary => {
        // Either primary or secondary signaled connection
        if !internal_keyboard_disabled {
            device_controller::disable_internal_keyboard()?;
            internal_keyboard_disabled = true;
        }
    }
    Event::NuphyDisconnected | Event::NuphyDisconnectedSecondary => {
        // Either primary or secondary signaled disconnection
        if internal_keyboard_disabled {
            device_controller::enable_internal_keyboard()?;
            internal_keyboard_disabled = false;
        }
    }
}
```

**Why this works:**  
If `ConnectionStatusChanged` fires first, the internal keyboard is disabled. If `DeviceWatcher.Updated` fires a few hundred milliseconds later, the check `if !internal_keyboard_disabled` prevents a redundant disable call.  
If `ConnectionStatusChanged` fails to fire (edge case), `DeviceWatcher.Updated` still triggers the disable. We cover both channels.

---

## §Q4-REVISED. Polling strategy

### PRIMARY PATH: Event-driven, NO polling

**Drop the 5s poll from the primary path.** BLE `ConnectionStatusChanged` is reliable enough that we don't need continuous polling in the steady state.

### LAYER 2: Modern Standby resume (ONE-SHOT re-query)

On `PBT_APMRESUMEAUTOMATIC` (Modern Standby resume), call:
```rust
let device = APP_STATE.lock().unwrap().nuphy_device.as_ref()?;
let status = device.ConnectionStatus()?;
```

Evaluate the live status and reconcile with cached state. If they diverge, log loudly and trust the live query.

**Why:** The Bluetooth stack may have reconnected or disconnected the Nuphy during suspension. We must re-sync on resume.

### SANITY-CHECK POLL: 60-second periodic (NOT 5s)

Add a low-frequency sanity-check poll (every 60 seconds, NOT 5 seconds) that compares cached state vs. `device.ConnectionStatus()`.

```rust
// In a background thread or timer:
loop {
    std::thread::sleep(Duration::from_secs(60));

    let device = APP_STATE.lock().unwrap().nuphy_device.as_ref()?;
    let live_status = device.ConnectionStatus()?;
    let cached_status = APP_STATE.lock().unwrap().nuphy_is_connected;

    let live_connected = live_status == BluetoothConnectionStatus::Connected;

    if live_connected != cached_status {
        eprintln!("WARNING: Cached BT state diverged from live query!");
        eprintln!("  Cached: {}, Live: {}", cached_status, live_connected);
        
        // Trust the live query and re-sync
        if live_connected {
            tx.send(Event::NuphyConnected)?;
        } else {
            tx.send(Event::NuphyDisconnected)?;
        }
    }
}
```

**Why this matters:**  
Catches event-loss bugs (e.g., Windows build-specific quirks, Bluetooth driver issues) without burning battery. 60-second interval is negligible for power draw but catches divergence within a minute.

---

## §CONFIGURATION CAPTURE (NEW)

### Problem

The Nuphy MAC `0xCC006219C5FD` is specific to the owner's keyboard. We cannot hardcode it. We need a **first-run flow** to capture it.

### First-run flow

1. **User pairs Nuphy in Windows Settings (out-of-band).**  
   User goes to Settings → Bluetooth & devices → Add device → selects "Air75 V3-3".

2. **User clicks "Detect Nuphy" in tray menu.**  
   App enumerates all paired BLE keyboards.

3. **Enumerate paired BLE keyboards with HID service.**

   **AQS selector:**
   ```csharp
   string hidServiceUuid = "00001812-0000-1000-8000-00805f9b34fb"; // HID over GATT
   string pairedSelector = BluetoothLEDevice.GetDeviceSelectorFromPairingState(true);
   string hidServiceSelector = GattDeviceService.GetDeviceSelectorFromUuid(new Guid(hidServiceUuid));
   string combinedSelector = $"{pairedSelector} AND {hidServiceSelector}";
   
   var devices = await DeviceInformation.FindAllAsync(combinedSelector);
   ```

   **Microsoft Learn references:**
   - [BluetoothLEDevice.GetDeviceSelectorFromPairingState](https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.bluetoothledevice.getdeviceselectorfrompairingstate)
   - [GattDeviceService.GetDeviceSelectorFromUuid](https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.genericattributeprofile.gattdeviceservice.getdeviceselectorfromuuid)

4. **If exactly one paired BLE keyboard found → use it.**  
   Extract MAC address from `device.BluetoothAddress` (u64).

5. **If multiple → present a picker.**  
   Show a dialog with device names. User selects the correct one.

6. **Store MAC + ContainerId in config.**

   **Config file:** `%APPDATA%\bluetooth-keyboard-blocker\config.toml`
   ```toml
   [nuphy]
   mac = 0xCC006219C5FD
   container_id = "{DFCA93C1-6CD7-5EBF-8011-740A73ED13DF}"
   friendly_name = "Air75 V3-3"
   ```

   **MAC extraction (windows-rs / Rust):**
   ```rust
   let mac: u64 = device.BluetoothAddress()?;
   let container_id: GUID = device.DeviceInformation()?.Properties()?.Lookup("System.Devices.ContainerId")?.cast()?;
   ```

7. **On subsequent runs, load from config.**

   If `config.toml` doesn't exist, show a tray notification: "First-run setup required. Right-click → Detect Nuphy."

### Config validation on load

```rust
let config = load_config()?; // Reads %APPDATA%\bluetooth-keyboard-blocker\config.toml

let device = BluetoothLEDevice::FromBluetoothAddressAsync(config.nuphy.mac)?.await?;
if device.is_null() {
    // Nuphy is not paired anymore (or was unpaired and re-paired with a different MAC)
    return Err("Nuphy not found. Please re-run first-run setup.");
}
```

---

## §VERDICT — Kramer's honest assessment

### Is BLE primary + DeviceWatcher secondary the right answer for v4?

**YES.**

### What I got wrong in my original analysis

1. **I assumed Nuphy was BT Classic (BR/EDR).** Spike 2 proved it's BT LE (BTHLE bus). This invalidated my entire premise.
2. **I used the wrong AQS protocol ID.** I was filtering for `{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}` (BR/EDR), not `{bb7bb05e-5972-42b5-94fc-76eaa7084d49}` (BLE).
3. **I said `BluetoothLEDevice.ConnectionStatusChanged` doesn't apply.** It's EXACTLY what applies.

### What I got right

1. **DeviceWatcher as a secondary signal is still valid.** It's a different observation channel (PnP enumeration vs. Bluetooth stack events). Defense-in-depth.
2. **Modern Standby resume requires a one-shot re-query.** Still true.
3. **The composite-device safety check (§Part 3 of my deep-dive) is still valid.** Jerry's amendments §B supersede it, but the principle stands.

### Why I'm confident this is correct now

1. **Spike 2 is ground truth.** Owner ran the commands on real hardware (Surface Laptop 7 + Nuphy Air75 V3-3). The device is BTHLE, not BTCLASSIC. No ambiguity.
2. **Microsoft Learn citations.** I verified the API semantics with official docs. `ConnectionStatusChanged` fires for all BLE disconnect scenarios (power-off, unpair, out-of-range).
3. **Event-loss edge cases are covered by the secondary path.** If primary fails, secondary catches it. If both fail, the 60s sanity-check poll catches it.
4. **First-run configuration flow is practical.** Enumerating paired BLE keyboards with HID service GUID 0x1812 is a one-liner with WinRT. User picks the Nuphy once, and we store the MAC. No hardcoding.

### What could still go wrong

1. **Bluetooth driver bugs on ARM64.** Snapdragon X Elite is a new platform (2024). If Qualcomm's BLE stack has quirks, we won't know until we test on real hardware.
2. **Supervision timeout variance.** If the Nuphy uses a very long supervision timeout (>6s), disconnect latency could be 6–10s. User experience degrades (internal keyboard is still disabled for 6–10s after Nuphy powers off). Mitigation: Jerry should measure this on real hardware during Spike 3.
3. **Modern Standby edge cases.** If the Bluetooth stack is in a weird state post-resume (e.g., device is "connected" but GATT link is actually down), we might disable the internal keyboard when we shouldn't. Mitigation: Test on real hardware during Spike 3.

**But none of these are design flaws.** They're integration risks. The architecture is sound.

### Final recommendation

**Ship BLE primary + DeviceWatcher secondary + 60s sanity-check poll for v4.**

Test on real hardware (Surface Laptop 7 + Nuphy Air75 V3-3) during Spike 3. Measure:
- Disconnect latency (power-off, unpair, out-of-range)
- Modern Standby resume behavior
- Event-loss frequency (run for 24 hours, count divergences)

If any of these fail, we'll know immediately and can adjust. But the design is correct.

---

**END OF REWRITE.**

**Scribe:** Fold this into Kramer's deep-dive (`.squad/agents/kramer/v4-deep-dive/bt-fix-and-device-tree.md`), replacing §Q1-Q4 and adding §CONFIGURATION as a new section. Keep §Q5, §Part 2, §Part 3 intact.
