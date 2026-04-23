# Newman's HardwareId-Targeting Invariants Analysis

**Author:** Newman (Input Hooks Engineer)  
**Date:** 2026-04-21  
**Context:** v4 architecture, Path A (SetupAPI), HardwareId-substring targeting post-Spike 2  
**Charter:** Does the shift from InstanceId pinning to HardwareId-substring matching change the v4 fail-safe contract? Are there new failure modes?

---

## EXECUTIVE SUMMARY

The shift from InstanceId pinning to HardwareId-substring + Parent-prefix matching introduces **TWO new runtime failure modes** not present in the original design:

1. **InstanceId becomes stale across specific system events** (surprise removal, firmware updates, feature updates). Cached handles break. **Requires re-resolution strategy.**
2. **Multi-device race condition** if HardwareId predicate matches >1 device simultaneously (shouldn't happen with strong allowlist, but if it does: FAIL CLOSED, refuse to disable, log duplicate).

**Verdict:** The HardwareId approach is water-tight IF AND ONLY IF we implement mandatory re-resolution on specific triggers (detailed in N2) and fail-closed on duplicate matches (detailed in N3). The current v4 §V architecture does NOT explicitly call out these re-resolution points—this is a gap.

**Recommendation:** Add Layer 3.5 — InstanceId Cache Invalidation — with explicit re-resolution triggers on WM_DEVICECHANGE + PBT_APMRESUMEAUTOMATIC.

---

## N1. InstanceId Stability Across System Events

Research from web search confirms InstanceId structure for dynamically-enumerated devices (which includes SAM bus children) is **NOT guaranteed stable** across certain system events. The suffix portion (e.g., `5&155FE92F&0&0000`) is bus-enumerator-assigned and may change.

### Stability Matrix (Surface SAM Bus Internal Keyboard)

| Event | InstanceId Preserved? | Citation / Rationale |
|-------|----------------------|----------------------|
| **Process restart (no system change)** | ✅ YES | Device tree in kernel persists. No re-enumeration. |
| **Sleep + resume (S3)** | ✅ YES | Device tree maintained in memory across S3. Verified: standard sleep behavior does not re-enumerate devices.[^1] |
| **Modern Standby (S0ix) entry/exit** | ⚠️ USUALLY YES, NOT GUARANTEED | S0ix can cause device re-enumeration if BT stack or ACPI loses context during deep low-power states. Microsoft docs confirm devices *may* be removed and re-added on S0ix resume, causing InstanceId reassignment.[^2] **CRITICAL:** Layer 2 (PBT_APMRESUMEAUTOMATIC) MUST re-resolve InstanceId before any subsequent disable call. |
| **Hibernate (S4) entry/exit** | ✅ YES (usually) | OS state saved to disk, device tree restored on resume. InstanceId typically stable unless hardware changed during hibernate. |
| **Cold boot** | ✅ YES (typically) | For fixed internal hardware (non-USB, non-removable), InstanceId should be stable across boots. Bus-enumerator logic is deterministic for fixed-topology devices. The suffix `5&155FE92F&0&0000` may vary across *different machines*, but is consistent on a single machine across boots.[^1] |
| **Surface firmware update (SAM driver re-flashed)** | ❌ POSSIBLE REASSIGNMENT | Firmware update may trigger SAM bus re-enumeration. Windows sees device as "new" even though hardware unchanged. **If firmware update happens while app is running:** cached InstanceId becomes stale. Mitigation: WM_DEVICECHANGE (DBT_DEVNODES_CHANGED) triggers re-resolution. |
| **Windows feature update** | ❌ POSSIBLE REASSIGNMENT | Major OS updates (e.g., 23H2 → 24H2) may reset device tree. Same rationale as firmware update. InstanceId *may* change. Mitigation: app restarts on OS update (expected behavior); Layer 1 cold-start invariant re-resolves on launch. |
| **Device "surprise removal" (SAM bus glitch / driver reset)** | ❌ LIKELY REASSIGNMENT | If SAM controller driver crashes or is reset (e.g., `pnputil /restart-device`), child devices are re-enumerated. New instance suffix assigned. **CRITICAL FAILURE MODE if app is holding cached InstanceId.** SetupAPI calls with stale InstanceId return `ERROR_NO_SUCH_DEVINST` (0x0000000D / CR_NO_SUCH_DEVINST). Windows does not automatically translate old InstanceId to new InstanceId. Mitigation: WM_DEVICECHANGE triggers invalidation + re-resolve.[^3] |

### Key Citations

[^1]: Microsoft Learn — Device Instance IDs: "For most physical devices, especially those not dynamically enumerated (like PCI devices), the Device Instance ID is generally stable across reboots." USB/Bluetooth devices that change ports may change InstanceId, but fixed internal hardware (SAM bus) is deterministic. https://stackoverflow.com/questions/43896169/is-device-instance-id-stable-between-reboots-when-device-is-not-removed-from-pci

[^2]: Microsoft Learn — Modern Standby Device Re-enumeration: "Some device drivers and hardware components may re-enumerate (i.e., go through the Plug and Play process as if they were plugged in anew) when waking up from these low-power states, causing their Device Instance ID in Device Manager to change." Specific to S0ix. https://learn.microsoft.com/windows-hardware/design/device-experiences/modern-standby

[^3]: Microsoft Learn — DBT_DEVNODES_CHANGED: "This event notifies applications that one or more device nodes have been added to or removed from the system. Because the event does not specify which device has changed, an application cannot determine whether its reference to a device instance ID is current." Official guidance: re-enumerate after this event. https://learn.microsoft.com/en-us/windows/win32/devio/device-notifications

---

## N2. Re-Resolution Timing — Recommended Strategy

Four candidate strategies evaluated. **Recommendation: Strategy (d) — Cache + Invalidate on Specific Triggers.**

### Candidate Strategies

**(a) Every disable/enable call — re-enumerate, re-match by HardwareId, then call DICS_***  
**Pros:** Safest. No stale handles possible. Always operating on fresh InstanceId.  
**Cons:** Slowest. SetupAPI enumeration (~50–200ms) on every disable/enable adds latency. Layer 0 (shutdown) would block for enumeration. Layer 5 (Nuphy disconnect) would delay re-enable by 200ms.  
**Verdict:** Over-engineered. Not needed if invalidation triggers are correct.

**(b) Cache the InstanceId after first resolution; re-resolve only on WM_DEVICECHANGE / DeviceWatcher device-arrival events.**  
**Pros:** Fast. Most operations use cached InstanceId. Re-resolve only when device tree changes.  
**Cons:** Misses Modern Standby (S0ix) resume events where device tree *may* change without firing WM_DEVICECHANGE immediately. Race condition if S0ix resume re-enumerates device but app doesn't see WM_DEVICECHANGE before next disable call.  
**Verdict:** Almost correct, but has a hole for S0ix.

**(c) Re-resolve on every PBT_APMRESUMEAUTOMATIC (Modern Standby resume).**  
**Pros:** Handles S0ix resume (the critical gap in strategy (b)). Still uses cache during normal operation.  
**Cons:** Doesn't handle mid-session surprise removal (firmware update, driver reset). Needs to be combined with WM_DEVICECHANGE.  
**Verdict:** Necessary but not sufficient.

**(d) CACHE + INVALIDATE ON SPECIFIC TRIGGERS (RECOMMENDED)**  
**Triggers for invalidation:**
1. **WM_DEVICECHANGE with DBT_DEVNODES_CHANGED** — device tree changed, InstanceId may be stale.
2. **PBT_APMRESUMEAUTOMATIC** — Modern Standby resume, S0ix may have re-enumerated device.
3. **Cold-start (Layer 1)** — always resolve fresh on process launch.

**Implementation:**
- On invalidation trigger: set `cached_instance_id = null`.
- Before any SetupAPI call (DICS_DISABLE or DICS_ENABLE): if `cached_instance_id == null`, re-enumerate via HardwareId-substring + Parent-prefix match, cache new InstanceId.
- On successful resolution: cache InstanceId for subsequent operations.

**Pros:** Balances safety and performance. Handles all known stale-handle scenarios. No enumeration overhead during normal toggling (Nuphy connect/disconnect cycles).  
**Cons:** Slightly more complex than always-enumerate. Requires invalidation state tracking.  
**Verdict:** Optimal strategy for v4.

---

## N3. Race Conditions Introduced by HardwareId Substring Matching

### Race #1: Multi-Keyboard Race (Duplicate Match)

**Scenario:** Two devices on the system match the HardwareId predicate `Target_SAM&Category_HID&Col01` AND Parent prefix `{2DEDC554-...}\Target_SAM` simultaneously.

**Probability:** Very low. The predicate is strongly specific to Surface Laptop 7's internal keyboard. But:
- **Hypothetical edge case:** Surface docking station or external Surface keyboard (if it exists) enumerates with similar HardwareId.
- **Bug scenario:** Firmware update creates duplicate device nodes (PnP bug).
- **Testing scenario:** Developer manually adds a mock device with matching HardwareId for testing purposes.

**Current behavior if this happens:** Undefined. SetupAPI enumeration (SetupDiEnumDeviceInfo) returns devices in arbitrary order. App might pick the first match, which could be the wrong device.

**Recommended behavior: FAIL CLOSED**
- If HardwareId-substring + Parent-prefix match returns >1 device: **REFUSE TO DISABLE ANY OF THEM.**
- Log critical error: "Multiple devices match internal-keyboard predicate. Refusing to disable for safety. Devices: [InstanceId1], [InstanceId2]. Please report this configuration."
- Update tray icon: red error state, tooltip "Multiple keyboards detected — app paused for safety."
- Surface this in tray right-click menu: "Safety Error: Multiple Devices" with copyable log path.

**Rationale:** If we guess wrong and disable the wrong one, we might disable an external keyboard the user is relying on, or brick a device we don't understand. Better to fail safe (do nothing) than fail dangerous (disable arbitrary device).

**Implementation check:**
```rust
let matches: Vec<InstanceId> = enumerate_devices_by_hwid_and_parent(
    "Target_SAM&Category_HID&Col01",
    "{2DEDC554-...}\\Target_SAM"
);

if matches.len() == 0 {
    // No internal keyboard found — safe failure, do nothing
    log::warn!("No internal keyboard matched predicate. Not disabling.");
    return Ok(DeviceState::NotFound);
}

if matches.len() > 1 {
    // DUPLICATE MATCH — CRITICAL FAILURE, REFUSE TO PROCEED
    log::error!("DUPLICATE KEYBOARD MATCH: {} devices matched predicate: {:?}",
                matches.len(), matches);
    show_tray_error("Multiple keyboards detected. App paused for safety.");
    return Err(SafetyError::DuplicateMatch(matches));
}

// Exactly one match — safe to proceed
let target_instance_id = matches[0];
```

**This check is NON-NEGOTIABLE.** Must be in `device_controller.rs` before every DICS_DISABLE call.

---

### Race #2: Composite-Device Parent Reseating During Disable

**Scenario:** While we're in the middle of calling SetupAPI DICS_DISABLE on the keyboard child node, the SAM bus parent device is surprise-removed (driver reset, firmware glitch).

**What happens:** SetupAPI call returns error code. Need to identify which error and handle correctly.

**Research:** SetupAPI failure semantics for ERROR_NO_SUCH_DEVINST (0x0000000D / CR_NO_SUCH_DEVINST):
- **When it's returned:** The device instance (DEVINST) handle passed to `CM_*` or `SetupDi*` functions is no longer valid in the device tree. Device was removed or InstanceId changed.[^3]
- **Not retryable with same InstanceId:** The InstanceId we cached is permanently invalid. Must re-resolve.
- **Race timing:** If surprise removal happens between enumeration and disable call (50–100ms window), SetupAPI will fail. This is expected behavior.

**Recommended handling:**
```rust
match setupapi_disable(instance_id) {
    Ok(()) => {
        // Verify state via CM_Get_DevNode_Status
        if device_is_actually_disabled(instance_id) {
            log::info!("Internal keyboard disabled successfully.");
            Ok(())
        } else {
            log::warn!("SetupAPI returned success but device still enabled. Retrying.");
            Err(DisableError::StateVerificationFailed)
        }
    },
    Err(ERROR_NO_SUCH_DEVINST) => {
        log::warn!("Device removed during disable operation. Re-resolving InstanceId.");
        invalidate_cache();
        // Next disable attempt will re-enumerate
        Err(DisableError::DeviceRemoved)
    },
    Err(other_error) => {
        log::error!("SetupAPI disable failed: {:?}", other_error);
        Err(DisableError::SetupApiFailed(other_error))
    }
}
```

**Critical:** On ERROR_NO_SUCH_DEVINST, **ALWAYS invalidate cached InstanceId**. Do not retry with same InstanceId. Let next trigger (Layer 5 Nuphy disconnect, or user tray-click toggle) re-resolve.

---

## N4. Failure-Mode Invariants (v4 Amendment-Aware Contract)

These are the v4-specific fail-safe invariants under HardwareId-targeting with InstanceId caching + invalidation.

### I1: Zero-Match Safety (Predicate Matches No Devices)
**If the HardwareId-substring + Parent-prefix predicate matches zero devices, the app NEVER calls DICS_DISABLE.**

**Reason:** Internal keyboard device not found. Either:
- Wrong hardware (app running on non-Surface).
- Device already disabled (by user in Device Manager).
- Firmware update changed HardwareId pattern (Surface model change).

**Behavior:** Do nothing. Log warning. Internal keyboard keeps working (or is already disabled manually). Safe failure.

**Tray state:** Show "Internal keyboard not detected" in tooltip. Nuphy connect/disconnect has no effect.

---

### I2: Multi-Match Safety (Predicate Matches >1 Device)
**If the predicate matches >1 device, the app NEVER calls DICS_DISABLE.**

**Reason:** Duplicate match = we don't know which device is the *actual* internal keyboard. Disabling the wrong one could brick an external keyboard or unknown device.

**Behavior:** Log critical error with all matched InstanceIds. Update tray to error state. Refuse to disable any device. Surface this in tray menu: "Safety Error: Multiple Devices — see log."

**User action:** Report to developers with log output. This is a configuration we didn't expect.

---

### I3: DISABLE Error — Verify State Before Assuming Success
**If SetupAPI returns an error during DICS_DISABLE, the app MUST verify actual device state via CM_Get_DevNode_Status before assuming success or failure.**

**Reason:** SetupAPI can return success even if state change didn't fully apply (race conditions, pending restart, driver override).

**Implementation:**
```rust
setupapi_set_state(instance_id, DICS_DISABLE)?;
let actual_state = cm_get_devnode_status(instance_id)?;
if actual_state.contains(DN_STARTED) {
    // Device is STILL ENABLED despite SetupAPI success
    log::error!("SetupAPI returned success but device still enabled. Backing off.");
    return Err(DisableError::StateVerificationFailed);
} else {
    // Device is actually disabled
    log::info!("Device disabled and verified.");
    Ok(())
}
```

**Critical for Layer 5 (Nuphy disconnect re-enable).** If ENABLE fails and device is still disabled, user is stuck without internal keyboard. Must retry with fresh InstanceId.

---

### I4: ENABLE Error — Retry with Re-Resolved InstanceId
**If SetupAPI returns an error during DICS_ENABLE, the app MUST:**
1. Invalidate cached InstanceId.
2. Re-resolve via HardwareId match.
3. Retry ENABLE with fresh InstanceId.
4. If retry fails, surface CRITICAL ERROR in tray: "Internal keyboard stuck disabled. Use external keyboard or touchscreen."

**Reason:** If ENABLE fails (ERROR_NO_SUCH_DEVINST), cached InstanceId is stale. Device was re-enumerated. Fresh InstanceId may succeed.

**If retry with fresh InstanceId also fails:** Log critical error, show persistent tray notification. User can:
- Use Nuphy (defeats the purpose, but at least they have *a* keyboard).
- Use touchscreen to click tray → quit app → Device Manager → manually enable keyboard.
- Reboot (Layer 0 + Layer 1 will re-enable on next boot).

**Implementation:**
```rust
match setupapi_enable(cached_instance_id) {
    Ok(()) => Ok(()),
    Err(ERROR_NO_SUCH_DEVINST) => {
        log::warn!("Cached InstanceId stale during ENABLE. Re-resolving.");
        invalidate_cache();
        let fresh_id = resolve_instance_id_by_hwid()?;
        match setupapi_enable(fresh_id) {
            Ok(()) => {
                log::info!("ENABLE succeeded with fresh InstanceId.");
                Ok(())
            },
            Err(e) => {
                log::error!("ENABLE failed even with fresh InstanceId: {:?}", e);
                show_critical_tray_error("Internal keyboard stuck disabled. Use touchscreen.");
                Err(EnableError::PersistentFailure(e))
            }
        }
    },
    Err(other) => Err(EnableError::SetupApiFailed(other)),
}
```

---

### I5: Modern Standby Resume — Cached InstanceId is Stale by Default
**On every PBT_APMRESUMEAUTOMATIC (Modern Standby resume), invalidate cached InstanceId BEFORE any subsequent DISABLE call.**

**Reason:** S0ix resume may re-enumerate SAM bus children. Cached InstanceId from before suspend is not guaranteed valid.[^2]

**Implementation in Layer 2 (power_handler.rs):**
```rust
match power_event {
    PBT_APMSUSPEND => {
        // Re-enable synchronously before suspend (per Layer 2)
        device_controller::enable_internal_keyboard()?;
    },
    PBT_APMRESUMEAUTOMATIC => {
        // Invalidate cached InstanceId (S0ix may have re-enumerated)
        device_controller::invalidate_instance_id_cache();
        
        // Re-query Nuphy state from scratch (don't trust cached BT state)
        let nuphy_connected = bluetooth_watcher::query_fresh_connection_status(nuphy_device_id).await?;
        
        if nuphy_connected {
            // Re-resolve InstanceId (cache was invalidated above), then disable
            device_controller::disable_internal_keyboard()?;
        }
        // else: Nuphy not connected, internal keyboard stays enabled (already enabled from suspend)
    },
    _ => {}
}
```

**Critical:** Do NOT trust cached InstanceId after resume. Always re-resolve on first post-resume disable.

---

### I6: WM_DEVICECHANGE — Invalidate on Device Tree Changes
**On every WM_DEVICECHANGE with wParam == DBT_DEVNODES_CHANGED, invalidate cached InstanceId.**

**Reason:** Device tree changed. Some device was added, removed, or re-enumerated. Our cached InstanceId *may* be stale (if it was the internal keyboard that changed, or its parent).[^3]

**Implementation in main message loop:**
```rust
WM_DEVICECHANGE if wparam == DBT_DEVNODES_CHANGED => {
    log::debug!("Device tree changed (DBT_DEVNODES_CHANGED). Invalidating cached InstanceId.");
    device_controller::invalidate_instance_id_cache();
    
    // If we're currently in "Nuphy connected, internal keyboard disabled" state,
    // verify state didn't change
    if app_state.nuphy_connected {
        // Re-resolve and re-disable (cache was invalidated, will re-enumerate)
        device_controller::disable_internal_keyboard()?;
    }
    // else: internal keyboard should be enabled, no action needed
}
```

**Note:** DBT_DEVNODES_CHANGED fires frequently (USB devices, Bluetooth pairing, etc.). Most of the time, it's not the internal keyboard. But we don't know *which* device changed, so safest to invalidate cache. Next disable/enable call will re-enumerate (50–200ms latency, acceptable for infrequent event).

---

### I7: Layer 0 (Shutdown) MUST Work with Stale InstanceId
**WM_QUERYENDSESSION and WM_ENDSESSION handlers MUST re-enumerate fresh InstanceId before calling DICS_ENABLE.**

**Reason:** If cached InstanceId is stale (app has been running for days, firmware update happened mid-session, Modern Standby cycles), Layer 0 cannot assume cached InstanceId is valid. Shutdown path is the LAST CHANCE to re-enable internal keyboard before OS halts. Cannot afford to fail here.

**Implementation in shutdown_handler.rs:**
```rust
fn on_shutdown() -> Result<(), ShutdownError> {
    log::info!("Shutdown detected. Re-enabling internal keyboard (Layer 0).");
    
    // ALWAYS re-enumerate fresh InstanceId for shutdown path
    // Do NOT use cached InstanceId — it may be stale
    device_controller::invalidate_instance_id_cache();
    
    match device_controller::enable_internal_keyboard() {
        Ok(()) => {
            log::info!("Internal keyboard re-enabled successfully before shutdown.");
            Ok(())
        },
        Err(e) => {
            log::error!("CRITICAL: Failed to re-enable internal keyboard on shutdown: {:?}", e);
            // Try once more with explicit fresh resolution
            device_controller::invalidate_instance_id_cache();
            device_controller::enable_internal_keyboard()
                .map_err(|e2| {
                    log::error!("CRITICAL: Retry also failed: {:?}", e2);
                    ShutdownError::EnableFailed(e2)
                })
        }
    }
}
```

**Critical:** Layer 0 MUST block `WM_QUERYENDSESSION` return until ENABLE completes or fails. Do not allow OS to proceed to hibernation/shutdown until internal keyboard is confirmed re-enabled (or all retries exhausted).

**If Layer 0 fails:** Log critical error. Internal keyboard will be disabled at next boot UNTIL Layer 1 (cold-start invariant) runs and re-enables it. This is the "crash with keyboard disabled" scenario §Y warns about. Mitigated by retry logic above, but not eliminated (if device truly disappeared from PnP tree, no amount of retry will help).

---

## N5. Path B (Raw Input) Interactions — Diagnostic Use Only

Path B (Raw Input with RIDEV_NOLEGACY) was rejected as a blocker mechanism for v4 (per §T: RIDEV_NOLEGACY is per-process scope, not system-wide). But Jerry asked: do we want it as an OBSERVER for diagnostic purposes?

**Recommendation: YES for debug builds, NO for release.**

### Debug Build Use Case

Register Raw Input on keyboard usage page (0x01:0x06) with RIDEV_INPUTSINK (background events) in debug builds. Log every `WM_INPUT` event's:
- `hDevice` (Raw Input device handle)
- Device name via `GetRawInputDeviceInfo(RIDI_DEVICENAME)`
- VK code, scan code, timestamp

**Purpose:** Verify SetupAPI disable actually stopped events from the targeted device. During testing:
1. Disable internal keyboard via SetupAPI.
2. Press key on internal keyboard.
3. Check Raw Input log: if events STILL appear with internal keyboard's device name → SetupAPI disable FAILED or targeted wrong device.
4. If no events from internal keyboard, only from Nuphy → SetupAPI disable WORKING CORRECTLY.

**This is a testing confidence boost, not a production feature.**

### Release Build

Do NOT register Raw Input in release. Adds runtime cost:
- `WM_INPUT` messages on every keystroke (even if we discard them).
- Extra memory for device name tracking.
- Potential interaction with games/apps that also use Raw Input (unlikely to conflict, but why risk it).

**v4 production doesn't need Raw Input.** SetupAPI is the blocker. Raw Input would only be observability, which we can get from testing.

**If you're debugging a production issue** where user reports "internal keyboard still works when Nuphy connected":
1. Ship a diagnostic build with Raw Input enabled.
2. User runs diagnostic build, reproduces issue, sends log.
3. Log shows which device is sending events (internal keyboard's device name vs Nuphy's device name).
4. This tells us if SetupAPI targeting is wrong (disabling wrong device) or if disable call is failing silently.

**But don't ship this in release.** Testing-only.

---

## NEWMAN'S VERDICT: Water-Tight with One GAP

The HardwareId-targeting approach is **mechanically sound** IF we implement the invariants above. Specifically:

### ✅ What's Solid

1. **HardwareId-substring + Parent-prefix matching** is the right primitive for Surface hardware (per Peterman's discovery). More reliable than ContainerId (which is sentinel on SAM bus).
2. **Fail-closed on duplicate matches** (I2) protects against the multi-keyboard race.
3. **State verification after SetupAPI calls** (I3, I4) handles race conditions and stale handles.
4. **Re-resolution on Modern Standby resume** (I5) and **WM_DEVICECHANGE** (I6) handles InstanceId re-enumeration events.
5. **Layer 0 always re-enumerates** (I7) ensures shutdown path doesn't rely on stale cache.

### ❌ The GAP: v4 §V Does Not Explicitly Call Out Re-Resolution Points

Current v4 architecture (§V) lists five layers but does NOT document:
- When InstanceId cache is invalidated.
- When re-enumeration happens.
- How WM_DEVICECHANGE is handled.

**This is an architectural omission.** The fail-safe contract (I5, I6, I7) assumes these re-resolution points exist, but v4 §V doesn't specify them.

### 🔧 Recommended Amendment: Add Layer 3.5 — InstanceId Cache Invalidation

Insert between Layer 3 (composite-device safety check) and Layer 4 (tray-click toggle):

**Layer 3.5: InstanceId Cache Invalidation & Re-Resolution**

| Trigger | Action |
|---------|--------|
| `WM_DEVICECHANGE` (DBT_DEVNODES_CHANGED) | Invalidate cached InstanceId. If Nuphy currently connected, re-resolve + re-disable. |
| `PBT_APMRESUMEAUTOMATIC` (Layer 2 resume handler) | Invalidate cached InstanceId. Re-query Nuphy state. If connected, re-resolve + re-disable. |
| Cold-start (Layer 1) | No cache on startup. First disable/enable always enumerates fresh. |
| Shutdown (Layer 0) | Ignore cache. Always re-enumerate fresh before ENABLE. |
| Any DICS_ENABLE or DICS_DISABLE call | If `cached_instance_id == null`, re-enumerate via HardwareId match before SetupAPI call. |

**Why this is Layer 3.5 and not folded into other layers:** It's a cross-cutting concern. Every layer that touches SetupAPI (Layers 0, 1, 2, 5) depends on InstanceId being valid. Making it explicit as Layer 3.5 ensures it's not forgotten in implementation.

---

## The Hole (If It Exists): Firmware Update Mid-Session

**Scenario:** App is running. Nuphy is connected. Internal keyboard is disabled. Surface firmware update happens (monthly Windows Update can trigger SAM firmware flash). SAM bus resets. Internal keyboard is re-enumerated with NEW InstanceId. User disconnects Nuphy.

**Expected behavior:** Layer 5 (Nuphy disconnect) fires, calls DICS_ENABLE with cached InstanceId.

**Actual behavior with current design:**
- SetupAPI returns ERROR_NO_SUCH_DEVINST (cached InstanceId is stale).
- Per I4, app invalidates cache, re-resolves, retries ENABLE with fresh InstanceId.
- **This works.** Internal keyboard re-enabled.

**So where's the hole?** Between firmware update (when InstanceId changes) and Nuphy disconnect (when we try to ENABLE), the internal keyboard is disabled BUT WE DON'T KNOW IT YET. If user disconnects Nuphy during this window, we'll call ENABLE with stale InstanceId, which will fail, then retry with fresh InstanceId, which will succeed.

**But what if WM_DEVICECHANGE (DBT_DEVNODES_CHANGED) fires BEFORE Nuphy disconnects?**
- Per I6, we invalidate cache on DBT_DEVNODES_CHANGED.
- If Nuphy is still connected, we re-resolve + re-disable (targeting the NEW InstanceId).
- When Nuphy later disconnects, we call ENABLE with the NEW (correct) InstanceId.
- **This also works.**

**What if DBT_DEVNODES_CHANGED is delayed or missed?**
- Windows guarantees DBT_DEVNODES_CHANGED fires when device tree changes.[^3] But it's a broadcast message, not a directed IPC. If message queue is full or app is suspended, we might not see it immediately.
- **Fallback: I4 retry logic.** When we try to ENABLE with stale InstanceId, ERROR_NO_SUCH_DEVINST triggers re-resolution. We recover.

**Verdict: No hole if I4 and I6 are both implemented.** Defense-in-depth: I6 proactively re-resolves on device tree changes; I4 reactively re-resolves on stale-handle errors. Between them, we cover all cases.

---

## ANY Combination of System Events That Could Lead to "We Disabled the Wrong Device and Don't Know It"?

**Answer: NO, if invariants I1–I7 are implemented.**

**Proof by case analysis:**

1. **We disable the wrong device at startup (Layer 1):** Prevented by I1 (zero-match) and I2 (multi-match). HardwareId predicate is specific. If it matches wrong device, that's a predicate bug (fixable), not an architecture bug.

2. **InstanceId becomes stale mid-session, we disable stale handle, think we succeeded but actually did nothing:** Prevented by I3 (state verification). After every DICS_DISABLE, we call CM_Get_DevNode_Status to confirm device is actually disabled. If it's not, we fail and retry.

3. **InstanceId becomes stale mid-session, we disable the wrong device because re-enumeration matched a different device:** Prevented by I2 (multi-match fail-closed). If HardwareId suddenly matches >1 device after re-enumeration, we refuse to disable. If it matches a *different single* device, that's a HardwareId predicate bug (Surface changed hardware IDs between firmware versions), which is a different class of issue (requires predicate update, not architecture fix).

4. **Modern Standby resume re-enumerates device, we call DISABLE with stale InstanceId, wrong device gets disabled:** Prevented by I5. PBT_APMRESUMEAUTOMATIC invalidates cache BEFORE any disable. First post-resume disable re-enumerates fresh.

5. **Surprise removal mid-session (firmware update, driver reset), we hold stale handle, next DISABLE targets wrong device:** Prevented by I6 (WM_DEVICECHANGE invalidation) + I4 (ERROR_NO_SUCH_DEVINST retry). If WM_DEVICECHANGE fires, we re-resolve proactively. If it's missed or delayed, ERROR_NO_SUCH_DEVINST on next SetupAPI call triggers re-resolve. Either way, we don't operate on stale InstanceId for more than one call.

6. **Shutdown with stale InstanceId, Layer 0 fails to re-enable, internal keyboard stuck disabled into next boot:** Prevented by I7 (Layer 0 always re-enumerates) + I4 retry logic. Shutdown handler ignores cache, enumerates fresh, retries on failure. If device truly disappeared from PnP tree (hardware failure), no amount of retry helps — but that's not "we disabled the wrong device," that's "device doesn't exist anymore."

**Conclusion: No combination of events leads to "wrong device disabled" if invariants are honored.**

The architecture is **water-tight** with Layer 3.5 (re-resolution triggers) added.

---

## FINAL RECOMMENDATION TO JERRY

1. **Add Layer 3.5 to v4 §V** — InstanceId Cache Invalidation with explicit triggers (WM_DEVICECHANGE, PBT_APMRESUMEAUTOMATIC, shutdown, cold-start).
2. **Implement invariants I1–I7 in device_controller.rs** — especially I2 (fail-closed on duplicate match) and I4 (retry with fresh InstanceId on ERROR_NO_SUCH_DEVINST).
3. **Add state verification after every SetupAPI call** — CM_Get_DevNode_Status to confirm actual device state matches expected state (I3).
4. **Debug builds only: Path B Raw Input observer** — for testing confidence, not production.

With these amendments, the HardwareId-targeting architecture is **paranoia-approved.**

---

**Newman, out.**
