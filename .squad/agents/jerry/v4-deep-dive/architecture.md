# v4 Architecture Spec — Jerry (Lead / Windows Architect)

**Date:** 2026-04-20
**Status:** PENDING OWNER REVIEW. Supersedes v3 (§K–§R in decisions.md).
**Context:** Two independent cross-model reviews returned REDESIGN verdicts on v3, identifying 6 critical findings. All 6 are valid. v3 is not defended here.

---

## §1. Honest Comparison: Path A vs Path B

### Path A — SetupAPI Device Disable (Non-Persistent)

**Mechanism:** Call `SetupDiSetClassInstallParams` + `SetupDiCallClassInstaller(DIF_PROPERTYCHANGE)` with `DICS_DISABLE` on the internal keyboard's specific device node when Nuphy connects. Reverse with `DICS_ENABLE` when it disconnects. The device is fully removed from the OS input stack — no app sees any events from it.

**What it costs:**
- Admin elevation required (scheduled task, one-time setup).
- `setup-task.ps1` creates two scheduled tasks (user-logon + SYSTEM-at-startup).
- Moderate code complexity: SetupAPI P/Invoke, device-tree enumeration, verify-after-call.
- Must re-enable on every power transition, session lock, and shutdown to avoid persistence traps.

**What can go wrong:**
1. **Persistence trap.** `DICS_DISABLE` writes `CONFIGFLAG_DISABLED` to `HKLM\...\ConfigFlags`. This persists across reboots. If the app dies, crashes, BSODs, or loses power while the keyboard is disabled, the device stays disabled on next boot. Every defensive layer in v3 existed because of this one property.
2. **Pre-OS lockout.** BitLocker recovery screen, UEFI setup, Windows Recovery Environment, boot manager — all happen before Windows loads. SetupAPI disable persists into these environments because the registry hive is already written. If anything goes wrong with re-enable timing, the user cannot type a BitLocker recovery key. This is the reviewer's finding #2, and it's real.
3. **Fast Startup (hybrid shutdown).** Surface defaults to hybrid shutdown. PnP isn't re-enumerated; `ConfigFlags` are restored from the hibernation image. Layer 0B's "At system startup" task may not fire on a hybrid-resume because it's not a true cold boot. Battery Saver may delay SYSTEM scheduled tasks. (Reviewer finding #1.)
4. **Sleep / Modern Standby.** Surface uses S0ix (Modern Standby). Lid-close → standby → Nuphy disconnects → resume → sign-in screen with no working keyboard. `WTS_SESSION_LOCK` (v3's Layer 0D) is not a power-state event. The correct hooks are `WM_POWERBROADCAST` with `PBT_APMSUSPEND` and `PBT_APMRESUMEAUTOMATIC`. (Reviewer finding #3.)
5. **Re-enable timing.** The re-enable call must complete before the OS tears down the process (shutdown) or suspends it (sleep). `WM_QUERYENDSESSION` gives 20s, but `PBT_APMSUSPEND` gives no guaranteed window — the OS can suspend immediately after broadcasting.
6. **SetupAPI targets wrong node.** Surface's internal keyboard is a child TLC of a composite HID-over-I2C device. Disabling the parent composite disables the touchpad too. Must target the keyboard TLC child specifically via `GUID_DEVINTERFACE_KEYBOARD`. (Reviewer finding #5.)

**Conditions where Path A is the right pick:**
- You need system-wide blocking (no app sees the internal keyboard's events).
- You accept the admin-elevation tax and the complexity of defensive re-enable layers.
- The persistence trap is manageable because the app auto-launches on logon and re-enables on start.

**Owner-visible UX:**
- One-time `setup-task.ps1` run (admin).
- Internal keyboard fully dead when Nuphy connected. Toggle via tray icon.
- On sleep/resume, brief (~500ms) window where internal keyboard is live before app re-disables.
- `recovery.exe` as break-glass if everything fails.

---

### Path B — Raw Input with RIDEV_INPUTSINK | RIDEV_NOLEGACY

**Mechanism:** Register for Raw Input on usage page 0x01 (Generic Desktop), usage 0x06 (Keyboard) with `RIDEV_NOLEGACY | RIDEV_INPUTSINK`. The app receives `WM_INPUT` messages with `RAWINPUTHEADER.hDevice` identifying the source device. For events from the internal keyboard, discard them. For events from the Nuphy, re-inject via `SendInput`.

**What it costs:**
- No admin elevation needed.
- No scheduled tasks, no `setup-task.ps1`, no `recovery.exe`.
- Code complexity: Raw Input registration, device enumeration, `SendInput` re-injection loop.
- Must handle the RIDEV_NOLEGACY-is-global-to-all-keyboards gotcha (see below).

**CRITICAL RESEARCH FINDING — Path B has a fatal flaw:**

`RIDEV_NOLEGACY` suppresses legacy messages (WM_KEYDOWN, WM_KEYUP) **only for the application that registered it, not system-wide.**

Microsoft Learn, RAWINPUTDEVICE Remarks section:
> "If RIDEV_NOLEGACY is set for a mouse or a keyboard, the system does not generate any legacy message for that device **for the application**."

Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice

This means: if our app registers `RIDEV_NOLEGACY` for keyboards, *our* app stops receiving `WM_KEYDOWN` from all keyboards. But **every other application on the system still receives `WM_KEYDOWN` from the internal keyboard normally.** Notepad, the browser, the terminal — they all see internal keyboard input. The internal keyboard is not blocked system-wide.

This was exactly the conclusion reached in v2's §A of decisions.md:
> "Raw Input API (WM_INPUT) — RAWINPUTHEADER.hDevice does identify the source device. But Raw Input only delivers events to the registered window; it cannot block input system-wide. Usable for detection, not blocking."

The v2/v3 rejection of Raw Input as a blocking mechanism was **correct**. The reviewers' recommendation of Path B was based on an incorrect premise — that `RIDEV_NOLEGACY` suppresses legacy messages system-wide. It does not.

**What else is wrong with Path B (even if the fatal flaw didn't exist):**

1. **RIDEV_NOLEGACY is per-usage-page, not per-device.** The `RAWINPUTDEVICE` structure has no field for a device handle. Setting `RIDEV_NOLEGACY` on `usUsagePage=0x01, usUsage=0x06` suppresses legacy messages for ALL keyboards. You'd have to re-inject Nuphy events via `SendInput`. Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice

2. **WM_INPUT not delivered while session locked.** Desktop isolation means the Winlogon desktop (lock screen) receives input, not the user desktop. Our app's window doesn't get `WM_INPUT` while locked. Source: confirmed by K6 in v3, and by https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input

3. **WM_INPUT not delivered on secure desktop.** Ctrl-Alt-Del, UAC prompts — input goes to the secure desktop only. Our Raw Input registration is invisible there.

4. **SendInput blocked by UIPI.** If our app runs non-elevated and the foreground app is elevated, `SendInput` events are blocked by User Interface Privilege Isolation. We'd need admin anyway, negating the "no admin" advantage. Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput

5. **SendInput events lose device identity.** Re-injected events appear as generic keyboard input. Any app using Raw Input to differentiate devices would see our Nuphy re-injections as deviceless.

**Conditions where Path B would be the right pick:**
None. The per-application scope of `RIDEV_NOLEGACY` means it cannot achieve system-wide blocking of the internal keyboard. This is fatal to the stated goal.

**What Raw Input IS good for:**
Device identification. `RAWINPUTHEADER.hDevice` is a stable per-session handle that identifies the physical device (confirmed: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputheader). Raw Input is the correct tool for the dead-man's switch (monitoring Nuphy keypress activity) and for device discovery. v2/v3 already used it for Layer D (dead-man's switch). That usage was correct.

---

### Hybrid Path C — WH_KEYBOARD_LL + Raw Input Correlation

**Not viable.** WH_KEYBOARD_LL fires synchronously during input processing. `WM_INPUT` is posted to the message queue asynchronously. The hook callback executes *before* the corresponding `WM_INPUT` is dispatched. There is no reliable way to correlate a hook callback invocation with a specific `WM_INPUT` message to determine which device generated the keystroke. `KBDLLHOOKSTRUCT` contains no `hDevice` field.

This was correctly identified in v2 and is not worth revisiting.

---

## §2. v4 Architecture Decision

**Recommendation: Path A (SetupAPI), with all 6 reviewer findings fixed.**

Path B is dead — `RIDEV_NOLEGACY` is per-application, not system-wide. The v2/v3 decision to use SetupAPI as the blocking mechanism was correct. What was wrong was v3's defensive layer implementation: Fast Startup assumptions, missing power-state hooks, wrong BT disconnect event, imprecise device targeting, and unverified toolchain. v4 fixes those six things. It does not change the core mechanism.

The BitLocker/pre-OS edge case (finding #2) remains an inherent risk of any SetupAPI approach. v4 mitigates it but cannot eliminate it — the `ConfigFlags` registry write is how SetupAPI works. The mitigation is: never let the device stay disabled across a full shutdown. If the re-enable-on-shutdown path fails (BSOD, power loss), the boot-recovery task catches it. If Fast Startup skips the boot task, we handle it via `WM_POWERBROADCAST PBT_APMRESUMEAUTOMATIC`. Defense in depth, not perfection.

---

## §3. Updated Module List for v4

| Module | Responsibility | Changed from v3? |
|--------|---------------|-------------------|
| `core` | App lifecycle, main thread, message pump, state machine, tray event loop. Owns all disable/enable decisions. Handles `WM_POWERBROADCAST`, `WM_QUERYENDSESSION`, `WM_ENDSESSION`, `WM_WTSSESSION_CHANGE`. | Yes — adds power-state handling |
| `device_controller` | SetupAPI calls. **Targets the keyboard TLC child node** via `GUID_DEVINTERFACE_KEYBOARD`, NOT the parent composite. Stateless: re-scans device tree on every call. Verify-after-call mandatory (`CM_Get_DevNode_Status`). Only main thread invokes. | Yes — precise targeting per finding #5 |
| `bluetooth` | WinRT `DeviceWatcher` for Nuphy presence. Watches for `Updated` with `System.Devices.Aep.IsConnected = false` (NOT `Removed`) to detect Nuphy power-off. Debounced 500ms. Works while session locked (K4 confirmed). | Yes — fixes finding #4 (wrong event) |
| `tray` | System tray icon. Left-click = unconditional panic re-enable. Right-click = Pause/Resume/Uninstall/Quit. | No change |
| `failsafe` | Registry-watchdog thread. Polls `HKCU\...\EmergencyDisable` every 2s. Independent of main thread. | No change |
| `deadman` | Raw Input subscriber on Nuphy `hDevice`. If internal keyboard disabled AND no Nuphy keypress for 60s, force re-enable. Raw Input NOT delivered while locked — acceptable because lock-screen re-enable already covers that case. | Minor — clarified scope |
| `config` | Hardware-ID cache (with re-enumeration fallback). Persistent flags. | No change |

**Removed from v3:**
- Layer 0B "Boot Recovery Task" scheduled as SYSTEM → **Retained but fixed** (see §4). Must handle Fast Startup (finding #1).
- Layer 0E "Unlock trigger" → **Replaced** by `WM_POWERBROADCAST PBT_APMRESUMEAUTOMATIC` handling in `core`, which fires on Modern Standby resume.

**Separate binary:**
- `recovery.exe` — unchanged from v3. Break-glass re-enable tool.

---

## §4. Process Lifecycle for v4

### Startup

1. **§0 INVARIANT** (unchanged): First executable code calls `device_controller::enable_internal_keyboard()` unconditionally. Verify with `CM_Get_DevNode_Status`. If fails, retry once, then enter safe mode (never attempt disable this session).
2. Acquire single-instance mutex.
3. Register for session notifications: `WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)`.
4. Spawn registry-watchdog thread.
5. Init tray icon.
6. Start WinRT `DeviceWatcher` for Nuphy.
7. Start dead-man timer (Raw Input on Nuphy device handle).
8. 5-second grace period.
9. Evaluate Nuphy state. If connected → disable internal keyboard. If not → leave enabled.
10. Enter message loop.

### Runtime

- **Nuphy connects** (DeviceWatcher `Updated`, `IsConnected = true`): Disable internal keyboard via `device_controller`. Verify. Update tray.
- **Nuphy disconnects** (DeviceWatcher `Updated`, `IsConnected = false`): Enable internal keyboard via `device_controller`. Verify. Update tray. This is the #1 lockout defense — covers Nuphy auto-power-off (K17).
- **Tray left-click**: Unconditional panic re-enable. Always. No state checks.

### Suspend (Sleep / Modern Standby) — FIXES FINDING #3

Handle `WM_POWERBROADCAST`:

- **`PBT_APMSUSPEND`**: Unconditionally re-enable internal keyboard. This must complete before the OS suspends. SetupAPI calls take ~100–300ms; the OS typically gives a few seconds between broadcast and actual suspend. If the call fails or is interrupted, the boot-recovery task catches it on next resume/reboot.
- **`PBT_APMRESUMEAUTOMATIC`**: Re-evaluate Nuphy state. If connected → re-disable after 2-second grace. If not → leave enabled (keyboard already re-enabled by the suspend handler). This fires on Modern Standby (S0ix) resume, which is the Surface's default power model.

**Why this fixes finding #3:** v3 used `WTS_SESSION_LOCK` which is a session event, not a power event. Lid-close on Surface triggers Modern Standby (S0ix), which is a power transition. `PBT_APMSUSPEND` fires; `WTS_SESSION_LOCK` may or may not fire depending on lock-before-sleep settings. The power hook is the correct one.

### Lock / Unlock — Retained but Secondary

- **`WTS_SESSION_LOCK`**: Re-enable internal keyboard (defense-in-depth; same as v3 Layer 0D). This handles the case where the user locks the screen manually (Win+L) without a power transition.
- **`WTS_SESSION_UNLOCK`**: If Nuphy connected, re-disable after 2-second grace.

### Shutdown / Restart — FIXES FINDING #1

Handle `WM_QUERYENDSESSION`:
- Return `TRUE` (allow shutdown) but set a flag.

Handle `WM_ENDSESSION`:
- If `wParam` is `TRUE` (shutdown proceeding): Unconditionally re-enable internal keyboard. Verify. This ensures `ConfigFlags` is cleared before the system writes the hibernation image (Fast Startup). 20-second `WaitToKillAppTimeout` is more than adequate.

**Why this fixes finding #1:** Fast Startup restores `ConfigFlags` from the hibernation image. By re-enabling before the image is written, we ensure the device is enabled in the hibernation image. On next "boot" (which is really a hibernate-resume), the internal keyboard starts enabled. The tray app's logon task fires, evaluates Nuphy state, and re-disables if needed.

**Layer 0B (Boot Recovery Task) is retained** for true cold boots (BSOD, power loss, forced shutdown bypassing `WM_ENDSESSION`). The boot task should use trigger "At system startup" AND trigger "On an event: Microsoft-Windows-Kernel-Boot event 27" (which fires on both cold boot and hybrid resume) to handle Fast Startup. Elaine/George to verify the exact trigger in their spike.

### Crash / BSOD / Power Loss

No `WM_ENDSESSION` fires. `ConfigFlags` may be left with `CONFIGFLAG_DISABLED`. Defenses:
1. Boot-recovery task (Layer 0B) fires on next true startup.
2. §0 invariant fires when tray app launches on logon.
3. `recovery.exe` as manual break-glass.

### BitLocker / Pre-OS Recovery — ACKNOWLEDGED RISK (Finding #2)

If the device is disabled in the registry AND a BSOD occurs AND the next boot requires BitLocker recovery key entry AND the boot-recovery task hasn't run yet: the user has no keyboard. This is the scenario the reviewers flagged.

**Mitigations (partial, not perfect):**
- The `WM_ENDSESSION` handler clears the disable on every clean shutdown/restart (including Windows Update reboots that go through the normal shutdown path).
- The boot-recovery task fires before the login screen, which is before most scenarios where a recovery key is needed.
- BitLocker recovery prompts typically appear only on firmware changes or TPM resets — events that also involve a true cold boot where the boot-recovery task fires.
- Surface has a touchscreen. On-screen keyboard is accessible even at BitLocker prompts via Ease-of-Access (verified K16).
- **Remaining gap:** power loss during disabled state → cold boot → BitLocker prompt → boot-recovery task fires AFTER BitLocker prompt? Needs verification. If the boot-recovery task trigger fires before the BitLocker unlock screen, we're safe. If not, the touchscreen/OSK is the only fallback.

**Owner must acknowledge this risk.** It is inherent to any SetupAPI-based approach. The alternative (kernel filter driver) is out of scope.

---

## §5. Day-1 Spike List

Spikes must run in this order. Each gates the next.

| # | Spike | Owner | What it proves | Failure invalidates |
|---|-------|-------|---------------|---------------------|
| 1 | **Toolchain spike** | Elaine | `cargo build --target aarch64-pc-windows-msvc` with `windows-rs` (Win32 + WinRT) and `tray-icon` compiles and links inside Docker. Produces a runnable ARM64 .exe. | Everything. If this fails, evaluate .NET 8 NativeAOT as fallback stack. |
| 2 | **Device-tree spike** | Kramer | Owner runs PowerShell commands on Surface. Identifies exact `InstanceId` of the internal keyboard TLC child node (not parent composite). Confirms `GUID_DEVINTERFACE_KEYBOARD` targeting works. Confirms Nuphy's BT device path. | Finding #5 (device targeting). If the keyboard TLC can't be isolated from the touchpad, SetupAPI approach needs a different targeting strategy. |
| 3 | **Raw Input hDevice spike** | Newman | Verify `RAWINPUTHEADER.hDevice` differentiates internal keyboard from Nuphy on the actual hardware. **Also verify the RIDEV_NOLEGACY scope finding**: confirm it is per-application, not system-wide. If per-application (as my research indicates), Path B is definitively dead. If somehow system-wide, we revisit. | Path B viability (likely dead). Also validates dead-man's switch design. |
| 4 | **BT disconnect event spike** | Kramer | Verify that Nuphy auto-power-off generates `DeviceWatcher.Updated` with `IsConnected = false`, NOT `DeviceWatcher.Removed`. Test on actual hardware. | Finding #4 (BT disconnect mechanism). |
| 5 | **Power-state spike** | George | Verify `PBT_APMSUSPEND` fires before Modern Standby suspend on Surface. Verify `PBT_APMRESUMEAUTOMATIC` fires on lid-open resume. Verify `WM_ENDSESSION` fires before Fast Startup hibernation image write. Verify boot-recovery task trigger timing relative to BitLocker prompt. | Findings #1 and #3 (Fast Startup and sleep handling). |

**Spike dependency chain:** 1 → (2, 3, 4 in parallel) → 5.
Spike 1 (toolchain) must pass before any code. Spikes 2, 3, 4 can run in parallel once toolchain is confirmed. Spike 5 requires a running test binary from spike 1.

Newman's Raw Input spike (#3) specifically: if it confirms `RIDEV_NOLEGACY` is per-application (as documented), that's the final nail for Path B. The dead-man's switch portion of the spike (hDevice differentiation) is still valuable for v4's Layer D regardless.

---

## §6. Owner Decisions Required

1. **Acknowledge Path B is dead.** RIDEV_NOLEGACY is per-application. SetupAPI is the only viable user-mode blocking mechanism. This is not a judgment call — it's how the API works.

2. **Acknowledge the BitLocker/pre-OS residual risk** (§4). Touchscreen + OSK is the fallback. No software-only solution eliminates this risk completely.

3. **Approve the power-state handling model** (§4): re-enable on every suspend, shutdown, and lock. Re-evaluate on every resume and unlock. Accept the brief (~500ms) double-keyboard window on resume.

4. **Confirm scope:** This app targets ONE user on ONE machine (the owner's Surface). Multi-user, Fast User Switching, and enterprise GPO scenarios are documented-not-mitigated. (George's scope-narrowing decision.)

5. **Run Kramer's device-tree spike commands** (spike #2) and paste back the results. This is blocking — we need the exact device node path before writing `device_controller`.

---

## Summary

v3's core mechanism (SetupAPI) was correct. v3's defensive layers had real gaps. v4 fixes all six:

| Finding | v3 Bug | v4 Fix |
|---------|--------|--------|
| #1 Fast Startup | Boot task may not fire | `WM_ENDSESSION` clears ConfigFlags before hibernation image; boot task trigger updated |
| #2 Pre-OS lockout | Unaddressed | Acknowledged risk. Touchscreen/OSK fallback. Boot task timing to be verified in spike #5 |
| #3 Sleep/Modern Standby | Used `WTS_SESSION_LOCK` (wrong hook) | `WM_POWERBROADCAST PBT_APMSUSPEND` / `PBT_APMRESUMEAUTOMATIC` |
| #4 BT disconnect event | Watched `DeviceWatcher.Removed` | Watch `DeviceWatcher.Updated` with `IsConnected = false` |
| #5 Device targeting | Targeted parent composite | Target keyboard TLC child via `GUID_DEVINTERFACE_KEYBOARD` |
| #6 Toolchain unproven | Assumed cargo-zigbuild works | Day-1 spike before any code |

Path B (Raw Input + RIDEV_NOLEGACY) was investigated honestly. It has a fatal flaw: `RIDEV_NOLEGACY` suppresses legacy messages only for the registering application, not system-wide. Other apps still see the internal keyboard. The v2/v3 rejection of Raw Input as a blocking mechanism was correct. The reviewers' recommendation was based on an incorrect premise.

Newman verifies the Raw Input spike. Kramer fixes the BT disconnect mechanism and provides device-tree commands. George handles power-state policy and scope narrowing. Elaine builds the toolchain Dockerfile. Owner approves architecture before any code.

— Jerry
