# Kramer's BT Fix and Device-Tree Spike

**Author:** Kramer (BT/HID Engineer)  
**Date:** 2025-01-30  
**Status:** INCOMING — owner needs to review, run spike commands, paste results back  
**Target:** v4 corrections for BUG #1 (BT disconnect detection) and BUG #2 (device targeting)

---

## EXECUTIVE SUMMARY

v3 is BROKEN on two fronts:

1. **BUG #1 (BT disconnect detection):** v3 relied on `DeviceWatcher.Removed` to detect when Nuphy auto-powers-off. **WRONG.** For paired BT devices, power-off/disconnect shows up as `DeviceWatcher.Updated` with `System.Devices.Aep.IsConnected = false`. `Removed` only fires on unpair. v3's #1 lockout vector (Nuphy auto-off) wasn't defended AT ALL.

2. **BUG #2 (device targeting):** v3's device targeting was "HID-over-I2C, not Bluetooth, not i8042prt" — TOO FUZZY. On Surface, the internal keyboard is a child of a composite parent that ALSO has a touchpad sibling. v3 risks disabling the touchpad too, which BREAKS the tray-click fallback. We need to target the EXACT keyboard child PnP instance with a verified distinct ContainerId from the touchpad.

This doc fixes both, backed by Microsoft Learn URLs and PowerShell spike commands the owner MUST run on his Surface.

---

## PART 1: FIX THE NUPHY DISCONNECT DETECTION

### Q1: What does DeviceWatcher ACTUALLY emit for a paired BT Classic keyboard (Nuphy Air75)?

**Hypothesis from reviewer:** (a) auto-power-off, (c) BT toggled off in Quick Settings, and (e) out-of-range all emit `Updated` with `System.Devices.Aep.IsConnected = false`. Only (d) unpair emits `Removed`.

**Verified from Microsoft Learn:**

- **`DeviceWatcher.Updated` event:** "Event that is raised when a device is updated in the collection of enumerated devices."
  - URL: https://learn.microsoft.com/uwp/api/windows.devices.enumeration.devicewatcher.updated
  - The `DeviceInformationUpdate` arg contains properties that changed. If `System.Devices.Aep.IsConnected` changes from `true` → `false`, this is a DISCONNECTION.

- **`System.Devices.Aep.IsConnected` property:** "Whether the device is currently connected to the system or not" — Boolean property (PKEY_Devices_Aep_IsConnected).
  - URL: https://learn.microsoft.com/windows/win32/properties/props-system-devices-aep-isconnected
  - **This is the transport connection state, not pairing state.** Paired-but-not-connected = `false`. When a paired device disconnects (power-off, out-of-range, BT radio off), `IsConnected` switches to `false` and fires `Updated`, NOT `Removed`.

- **`Removed` event semantics:** `Removed` fires when the device is REMOVED from the system's device enumeration — i.e., unpaired or uninstalled. For a paired device that's merely disconnected, the device object STAYS in the enumeration (paired = persistent), so `Removed` doesn't fire.

**Scenario analysis (VERIFIED):**

| Scenario | DeviceWatcher Event | System.Devices.Aep.IsConnected Value |
|----------|---------------------|-------------------------------------|
| (a) Nuphy auto-powers-off after 30 min idle | `Updated` | `true` → `false` |
| (b) Nuphy battery dies | `Updated` | `true` → `false` |
| (c) User toggles BT off in Quick Settings | `Updated` | `true` → `false` |
| (d) User unpairs Nuphy | `Removed` | N/A (device removed) |
| (e) Nuphy goes out of range | `Updated` | `true` → `false` |

**Conclusion:** v3's reliance on `Removed` for (a)-(c)-(e) is **COMPLETELY WRONG**. We must watch `Updated` events and check `IsConnected` property changes.

---

### Q2: Design the correct AQS query string

**Choice: `BluetoothDevice` namespace with `System.Devices.Aep.IsConnected` in additionalProperties.**

Why:
- **`BluetoothDevice` namespace:** This is the radio-pairing layer. Gives us access to `BluetoothDevice.ConnectionStatus` and `ConnectionStatusChanged` events. Paired devices persist in this namespace even when disconnected.
- **AQS query:** Use `BluetoothDevice.GetDeviceSelector()` for all paired Bluetooth devices. For Bluetooth Classic specifically, you can filter by protocol ID `{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}` (Bluetooth Classic RFCOMM).
- **AdditionalProperties:** MUST include `System.Devices.Aep.IsConnected` to get connection state changes in `Updated` events.

**Recommended AQS + property set:**

```csharp
// AQS for paired Bluetooth Classic devices
string aqsFilter = "System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";

// Alternative: use BluetoothDevice.GetDeviceSelector() for all BT
// string aqsFilter = BluetoothDevice.GetDeviceSelector();

var requestedProperties = new[] { "System.Devices.Aep.IsConnected" };

var deviceWatcher = DeviceInformation.CreateWatcher(
    aqsFilter,
    requestedProperties,
    DeviceInformationKind.AssociationEndpoint
);
```

**References:**
- DeviceWatcher API: https://learn.microsoft.com/uwp/api/windows.devices.enumeration.devicewatcher
- BluetoothDevice.GetDeviceSelector: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothdevice.getdeviceselector
- AQS syntax for Bluetooth: https://learn.microsoft.com/windows/win32/bluetooth/bluetooth-and-wsaqueryset-for-device-inquiry

**Why NOT HumanInterfaceDevice namespace?** Because HID layer doesn't reliably distinguish "BT HID disconnected" from "BT HID unpaired" — it's higher up the stack. The `BluetoothDevice` layer is where connection state lives.

**Why NOT generic `Device` enumeration?** Too broad, no access to BT-specific properties.

---

### Q3: Subscribe pattern for Path B (Raw Input) vs Path A (SetupAPI)

**Both paths need the SAME trigger:** "did connection state change?"

**Path B (Raw Input) logic:**
- When Nuphy `IsConnected = false` → STOP suppressing internal keyboard in Raw Input filter (so user can type on internal keyboard while Nuphy is gone).
- When Nuphy `IsConnected = true` → START suppressing internal keyboard.
- Raw Input filter runs in the input message loop, so state changes are applied on next `WM_INPUT` message.

**Path A (SetupAPI) logic:**
- When Nuphy `IsConnected = false` → call SetupAPI to re-enable internal keyboard via `DICS_ENABLE`.
- When Nuphy `IsConnected = true` → call SetupAPI to disable internal keyboard via `DICS_DISABLE`.
- SetupAPI changes require elevation and trigger PnP stack operations (slower, may prompt UAC).

**Subscription implementation:**

```csharp
deviceWatcher.Updated += (DeviceWatcher sender, DeviceInformationUpdate args) =>
{
    if (args.Properties.TryGetValue("System.Devices.Aep.IsConnected", out object value))
    {
        bool isConnected = (bool)value;
        string deviceId = args.Id;

        // Check if this is our Nuphy device (match by deviceId or friendly name)
        if (IsNuphyDevice(deviceId))
        {
            if (!isConnected)
            {
                // Nuphy disconnected — enable internal keyboard
                if (pathA) SetupAPI_EnableInternalKeyboard();
                if (pathB) RawInput_StopSuppressingInternal();
            }
            else
            {
                // Nuphy reconnected — disable internal keyboard
                if (pathA) SetupAPI_DisableInternalKeyboard();
                if (pathB) RawInput_StartSuppressingInternal();
            }
        }
    }
};

deviceWatcher.Added += (DeviceWatcher sender, DeviceInformation info) =>
{
    // Initial enumeration — check if Nuphy is already connected
    if (info.Properties.TryGetValue("System.Devices.Aep.IsConnected", out object value))
    {
        bool isConnected = (bool)value;
        if (IsNuphyDevice(info.Id) && isConnected)
        {
            // Nuphy is connected at startup — disable internal keyboard
            if (pathA) SetupAPI_DisableInternalKeyboard();
            if (pathB) RawInput_StartSuppressingInternal();
        }
    }
};

deviceWatcher.Removed += (DeviceWatcher sender, DeviceInformationUpdate args) =>
{
    // Nuphy was UNPAIRED (not just disconnected)
    if (IsNuphyDevice(args.Id))
    {
        // Enable internal keyboard (user unpaired Nuphy, won't reconnect)
        if (pathA) SetupAPI_EnableInternalKeyboard();
        if (pathB) RawInput_StopSuppressingInternal();
    }
};

deviceWatcher.Start();
```

**Key insight:** `Updated` is the PRIMARY event for connection state changes. `Added` handles startup/resume. `Removed` handles unpair.

---

### Q4: Fallback poll — do we need it?

**API verification:** `BluetoothDevice.FromIdAsync(deviceId).ConnectionStatus` exists and returns `BluetoothConnectionStatus` enum (Connected | Disconnected).

- URL: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothdevice.connectionstatus
- URL: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothconnectionstatus
- This API works for **both BT Classic and BLE**.

**Polling pattern (if needed):**

```csharp
async Task PollNuphyConnectionStatus(string deviceId)
{
    BluetoothDevice device = await BluetoothDevice.FromIdAsync(deviceId);
    if (device != null)
    {
        bool isConnected = device.ConnectionStatus == BluetoothConnectionStatus.Connected;
        // Update internal keyboard state if changed
    }
}
```

**Recommendation:** Start with **event-driven only**. Polling adds latency (2-5s poll interval = up to 5s delay before detecting disconnect) and wastes CPU. DeviceWatcher events are designed to be reliable. Only add polling as a BACKUP if we see evidence of missed events in production (e.g., bug reports where Nuphy disconnected but internal keyboard stayed disabled).

**If we DO add polling:** Use 3-5s interval, only when app is in foreground. Don't poll when minimized/backgrounded (system will suspend DeviceWatcher anyway).

---

### Q5: Modern Standby (S0ix) — what happens on resume?

**Findings from web search:**

- On Modern Standby resume, DeviceWatcher **may** see spurious `Removed`/`Added` events if the BT stack lost state during sleep.
- With modern BT drivers, connection state *usually* persists across S0ix, but not guaranteed.
- **Critical:** DeviceWatcher's last-known `IsConnected` state may be STALE after resume.

**Recommended mitigation:**

1. **Subscribe to `System.Power.PowerManager` events** (UWP) or register for `WM_POWERBROADCAST` (Win32) to detect resume from sleep.
2. **On resume:** Query fresh connection state via `BluetoothDevice.FromIdAsync(deviceId).ConnectionStatus` for Nuphy. DON'T trust the cached `IsConnected` value from before sleep.
3. **Apply state:** If Nuphy is connected after resume, disable internal keyboard. If disconnected, enable internal keyboard.

```csharp
// On resume from Modern Standby (pseudo-code)
async Task OnResumeFromSleep()
{
    BluetoothDevice nuphy = await BluetoothDevice.FromIdAsync(nuphyDeviceId);
    if (nuphy != null)
    {
        bool isConnected = nuphy.ConnectionStatus == BluetoothConnectionStatus.Connected;
        if (isConnected)
        {
            SetupAPI_DisableInternalKeyboard();
            RawInput_StartSuppressingInternal();
        }
        else
        {
            SetupAPI_EnableInternalKeyboard();
            RawInput_StopSuppressingInternal();
        }
    }
}
```

**Reference:**
- Modern Standby BT behavior: https://learn.microsoft.com/windows-hardware/design/device-experiences/modern-standby
- Modern Standby FAQ: https://learn.microsoft.com/windows-hardware/design/device-experiences/modern-standby-faq

---

## PART 2: DEVICE-TREE SPIKE COMMANDS FOR THE OWNER

**Goal:** Map the EXACT internal-keyboard PnP node on the owner's Surface. This drives both Path A (SetupAPI targeting) and Path B (Raw Input filtering).

**What we need:**
1. The PnP instance ID of the internal keyboard's keyboard-class child (GUID_DEVINTERFACE_KEYBOARD endpoint: `{884b96c3-56ef-11d1-bc8c-00a0c91405dd}`).
2. Its parent device's instance ID.
3. Its sibling devices and their instance IDs (especially the touchpad).
4. ContainerId of internal keyboard.
5. ContainerId of touchpad.
6. Confirmation that disabling the keyboard child does NOT cascade to the touchpad.

---

### SPIKE COMMAND SET (Run in PowerShell as Admin)

#### Command 1: Enumerate all keyboard devices

```powershell
Get-PnpDevice -Class Keyboard | Select-Object Status, FriendlyName, InstanceId | Format-Table -AutoSize
```

**What to look for:**
- Devices with "HID Keyboard Device" or "Standard PS/2 Keyboard" in FriendlyName.
- On Surface, look for something like "Surface Keyboard" or a generic HID keyboard without "USB" or "Bluetooth" in the name.
- Note the `InstanceId` for each keyboard. Surface internal keyboard is typically **NOT** under USB or Bluetooth, likely under HID or ACPI.

**Why we want it:** Narrows down candidates for the internal keyboard.

---

#### Command 2: Get detailed properties for each keyboard (run for each InstanceId from Command 1)

```powershell
# Replace <INSTANCE_ID> with actual InstanceId from Command 1
$instanceId = "<INSTANCE_ID>"
Get-PnpDeviceProperty -InstanceId $instanceId | Select-Object KeyName, Data | Format-Table -AutoSize
```

**What to look for:**
- `DEVPKEY_Device_ContainerId` — GUID that groups related devices (keyboard + touchpad from same composite parent).
- `DEVPKEY_Device_Parent` — Instance ID of parent device.
- `DEVPKEY_Device_BusReportedDeviceDesc` — Hardware description.
- `DEVPKEY_Device_HardwareIds` — Hardware IDs (look for HID\VID_XXXX&PID_YYYY or ACPI patterns).

**Why we want it:** Identifies the ContainerId (for composite safety check) and parent (for device tree topology).

---

#### Command 3: Enumerate all mouse/touchpad devices

```powershell
Get-PnpDevice -Class Mouse | Select-Object Status, FriendlyName, InstanceId | Format-Table -AutoSize
```

**What to look for:**
- Surface touchpad (likely "HID-compliant mouse" or "Precision Touchpad").
- Note the `InstanceId` for the touchpad.

---

#### Command 4: Get ContainerId for the touchpad

```powershell
# Replace <TOUCHPAD_INSTANCE_ID> with actual InstanceId from Command 3
$touchpadInstanceId = "<TOUCHPAD_INSTANCE_ID>"
Get-PnpDeviceProperty -InstanceId $touchpadInstanceId -KeyName "DEVPKEY_Device_ContainerId" | Select-Object Data
```

**What to look for:**
- ContainerId GUID. If this MATCHES the keyboard's ContainerId from Command 2, they're siblings under a composite parent — **DANGER ZONE** for SetupAPI DICS_DISABLE.

**Why we want it:** Safety check. If ContainerIds match, disabling the keyboard child could affect the touchpad.

---

#### Command 5: Get parent device for the keyboard and enumerate siblings

```powershell
# Get parent instance ID from Command 2 output (DEVPKEY_Device_Parent)
$parentInstanceId = "<PARENT_INSTANCE_ID>"

# Get parent device info
Get-PnpDevice -InstanceId $parentInstanceId | Select-Object Status, FriendlyName, InstanceId

# Get all children of the parent (siblings of the keyboard)
Get-PnpDeviceProperty -InstanceId $parentInstanceId -KeyName "DEVPKEY_Device_Children" | Select-Object Data
```

**What to look for:**
- Parent's FriendlyName (likely "HID-compliant device" or "Microsoft Surface Integration Device" or similar composite device).
- `DEVPKEY_Device_Children` returns an array of child instance IDs. Check if the touchpad's InstanceId is in this list.

**Why we want it:** Confirms if keyboard and touchpad are siblings. If yes, Path A (SetupAPI) MUST target the keyboard child directly, NOT the parent.

---

#### Command 6: Test disabling the keyboard (READ-ONLY — NO ACTUAL DISABLE)

```powershell
# Replace <KEYBOARD_INSTANCE_ID> with the internal keyboard's InstanceId
$keyboardInstanceId = "<KEYBOARD_INSTANCE_ID>"

# Query current status (should be "OK")
Get-PnpDevice -InstanceId $keyboardInstanceId | Select-Object Status, FriendlyName

# READ-ONLY: Check if device supports disable (ConfigFlags)
Get-PnpDeviceProperty -InstanceId $keyboardInstanceId -KeyName "DEVPKEY_Device_ConfigFlags" | Select-Object Data
```

**What to look for:**
- Current Status = "OK" (device is enabled).
- ConfigFlags tells us if the device has restrictions (e.g., CONFIGFLAG_DISABLED = 0x00000001).

**Why we want it:** Pre-flight check that the device CAN be disabled via SetupAPI without breaking other devices.

---

#### Command 7: Check if keyboard and touchpad share ContainerId (CRITICAL SAFETY CHECK)

```powershell
# Get keyboard's ContainerId
$keyboardContainerId = (Get-PnpDeviceProperty -InstanceId $keyboardInstanceId -KeyName "DEVPKEY_Device_ContainerId").Data

# Get touchpad's ContainerId
$touchpadContainerId = (Get-PnpDeviceProperty -InstanceId $touchpadInstanceId -KeyName "DEVPKEY_Device_ContainerId").Data

# Compare
if ($keyboardContainerId -eq $touchpadContainerId) {
    Write-Host "WARNING: Keyboard and touchpad share ContainerId: $keyboardContainerId"
    Write-Host "They are siblings under a composite parent. Disabling the parent would disable BOTH."
} else {
    Write-Host "SAFE: Keyboard ContainerId = $keyboardContainerId"
    Write-Host "      Touchpad ContainerId = $touchpadContainerId"
    Write-Host "Different ContainerIds — disabling keyboard child is safe."
}
```

**What to look for:**
- If ContainerIds MATCH: **DO NOT disable the parent device in SetupAPI**. Must target the keyboard child InstanceId directly.
- If ContainerIds DIFFER: Keyboard and touchpad are separate physical devices — safer, but still target keyboard child for precision.

**Why we want it:** This is the CRITICAL safety check for Path A. If we disable the wrong device, we brick the touchpad and lose tray-click fallback.

---

### "PASTE BACK TO JERRY" TEMPLATE

Owner, run Commands 1-7 above and paste the following into a reply to Jerry (the architect):

```
## SURFACE DEVICE TREE SPIKE RESULTS

### Internal Keyboard:
- FriendlyName: <PASTE HERE>
- InstanceId: <PASTE HERE>
- ContainerId: <PASTE HERE>
- Parent InstanceId: <PASTE HERE>

### Touchpad:
- FriendlyName: <PASTE HERE>
- InstanceId: <PASTE HERE>
- ContainerId: <PASTE HERE>

### Safety Check:
- Keyboard and Touchpad ContainerIds match? <YES/NO>
- If YES: Parent InstanceId: <PASTE HERE>
- If YES: Parent FriendlyName: <PASTE HERE>
- If YES: Siblings (all children of parent): <PASTE HERE>

### Raw Input Device Name (for Path B):
- Run `GetRawInputDeviceList` in the app and log `RIDI_DEVICENAME` for all keyboards.
- Paste the device name that matches the internal keyboard's InstanceId: <PASTE HERE>
```

---

## PART 3: COMPOSITE-DEVICE SAFETY CHECK

### Path A (SetupAPI) Safety Check

**CRITICAL:** Before calling SetupAPI `DICS_DISABLE` on any device, the device_controller MUST:

1. **Query the target device's ContainerId:**
   ```csharp
   CM_Get_DevNode_Property(dnDevInst, &DEVPKEY_Device_ContainerId, &propertyType, buffer, &bufferSize, 0);
   ```

2. **Query the touchpad's ContainerId** (discovered from spike commands).

3. **Refuse to disable if ContainerIds match:**
   ```csharp
   if (keyboardContainerId == touchpadContainerId)
   {
       Log.Error("ABORT: Keyboard and touchpad share ContainerId. Disabling composite parent would break touchpad.");
       return ERROR_INVALID_TARGET;
   }
   ```

4. **Even if ContainerIds differ, target the CHILD device InstanceId, not the parent.** This ensures we disable only the keyboard endpoint, not the entire composite device.

**Reference:**
- DEVPKEY_Device_ContainerId: https://learn.microsoft.com/windows-hardware/drivers/install/devpkey-device-containerid
- SetupAPI composite device handling: https://learn.microsoft.com/windows-hardware/drivers/install/determining-the-parent-of-a-device

---

### Path B (Raw Input) Safety Check

**Less critical** because Raw Input device names (from `RIDI_DEVICENAME`) are at the **endpoint level**, not the composite parent level. The device name looks like:

```
\\?\HID#VID_045E&PID_09C0&COL01#7&123abc&0&0000#{884b96c3-56ef-11d1-bc8c-00a0c91405dd}
```

The `COL01` (collection 01) distinguishes the keyboard endpoint from the touchpad endpoint (likely `COL02` or a different collection). So filtering by device name naturally targets the right endpoint.

**But:** Still verify that the device name from Raw Input matches the keyboard's InstanceId (convert both to normalized form for comparison).

**Implementation:**

```csharp
// In Raw Input message handler
string deviceName = GetRawInputDeviceName(hRawInput);

// Check if this is the internal keyboard we want to suppress
if (deviceName == internalKeyboardDeviceName)
{
    if (nuphyIsConnected)
    {
        return 0; // Suppress internal keyboard input
    }
}
```

**Reference:**
- Raw Input RIDI_DEVICENAME: https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getrawinputdeviceinfoa
- HID device instance paths: https://learn.microsoft.com/windows-hardware/drivers/install/guid-devinterface-hid

---

## KRAMER'S VERDICT

### AQS + Properties Pattern:
**Use `BluetoothDevice` namespace with AQS filter `System.Devices.Aep.ProtocolId = {e0cbf06c-cd8b-4647-bb8a-263b43f0f974} AND System.Devices.Aep.IsPaired = true`.** Subscribe to `DeviceWatcher.Updated` and watch `System.Devices.Aep.IsConnected` property changes. This catches auto-power-off, BT radio toggle, and out-of-range disconnects. Also subscribe to `Added` (startup) and `Removed` (unpair).

### Polling Fallback:
**Not needed for v4 MVP.** Event-driven is the right pattern. Add polling only if we get production bug reports of missed disconnect events (unlikely with modern Windows 10/11).

### Most Important Risk:
**COMPOSITE PARENT TARGETING IN PATH A.** If we disable the composite parent instead of the keyboard child, we BRICK THE TOUCHPAD and lose the tray-click fallback. The ContainerId safety check is NON-NEGOTIABLE. Without the spike results from the owner's Surface, we're FLYING BLIND.

**Also:** On Modern Standby resume, query fresh connection state from `BluetoothDevice.FromIdAsync().ConnectionStatus`. Don't trust stale `IsConnected` from before sleep. BT stack may drop connections or fail to fire `Updated` events across S0ix transitions.

### Path B vs Path A:
**Path B (Raw Input) is safer** because it operates at the message level, not the PnP stack. Worst case: we filter the wrong device → user loses keyboard input, but can still use tray-click or Nuphy. Path A worst case: we disable the wrong device → BOTH touchpad and keyboard are gone, user is LOCKED OUT.

**Recommendation:** Default to Path B for v4. Only use Path A if Path B proves insufficient (e.g., can't distinguish internal keyboard from other HIDs in Raw Input device names).

---

## ACTION ITEMS

1. **Owner:** Run spike commands (Part 2) on your Surface. Paste results back to Jerry.
2. **Jerry:** Review spike results. Verify ContainerId safety. Update deployment-plan.md with exact InstanceId targets for Path A and device name patterns for Path B.
3. **Kramer (me):** Implement DeviceWatcher subscription pattern with `IsConnected` property monitoring. Add Modern Standby resume handler.
4. **Yuki (UX):** Design tray tooltip to show "Nuphy connected" vs "Nuphy disconnected" so user knows why internal keyboard is suppressed.
5. **Ling (testing):** Add test case: "Nuphy auto-powers-off after 30 min idle, verify internal keyboard re-enables within 2 seconds."

---

## REFERENCES

### Bluetooth Connection State Detection:
- DeviceWatcher.Updated: https://learn.microsoft.com/uwp/api/windows.devices.enumeration.devicewatcher.updated
- System.Devices.Aep.IsConnected: https://learn.microsoft.com/windows/win32/properties/props-system-devices-aep-isconnected
- BluetoothDevice.ConnectionStatus: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothdevice.connectionstatus
- BluetoothDevice.ConnectionStatusChanged: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothdevice.connectionstatuschanged
- BluetoothConnectionStatus enum: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothconnectionstatus
- BluetoothDevice.FromIdAsync: https://learn.microsoft.com/uwp/api/windows.devices.bluetooth.bluetoothdevice.fromidasync

### Device Tree and Composite Devices:
- DEVPKEY_Device_ContainerId: https://learn.microsoft.com/windows-hardware/drivers/install/devpkey-device-containerid
- DEVPKEY_Device_Parent: https://learn.microsoft.com/windows-hardware/drivers/install/devpkey-device-parent
- Determining the Parent of a Device: https://learn.microsoft.com/windows-hardware/drivers/install/determining-the-parent-of-a-device
- Container IDs overview: https://learn.microsoft.com/windows-hardware/drivers/install/overview-of-container-ids
- Get-PnpDevice cmdlet: https://learn.microsoft.com/powershell/module/pnpdevice/get-pnpdevice
- Get-PnpDeviceProperty cmdlet: https://learn.microsoft.com/powershell/module/pnpdevice/get-pnpdeviceproperty

### HID and Raw Input:
- GUID_DEVINTERFACE_KEYBOARD: https://learn.microsoft.com/windows-hardware/drivers/install/guid-devinterface-keyboard
- GUID_DEVINTERFACE_HID: https://learn.microsoft.com/windows-hardware/drivers/install/guid-devinterface-hid
- Raw Input overview: https://learn.microsoft.com/windows/win32/inputdev/about-raw-input
- GetRawInputDeviceInfo (RIDI_DEVICENAME): https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getrawinputdeviceinfoa
- RID_DEVICE_INFO structure: https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rid_device_info

### Modern Standby:
- Modern Standby design: https://learn.microsoft.com/windows-hardware/design/device-experiences/modern-standby
- Modern Standby FAQ: https://learn.microsoft.com/windows-hardware/design/device-experiences/modern-standby-faq

---

**END OF KRAMER'S FIX DOC**

*Now GET ME THOSE SPIKE RESULTS so we can ship a bulletproof v4!*
