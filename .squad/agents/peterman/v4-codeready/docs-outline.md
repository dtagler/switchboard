# Documentation Outlines — v5.9 IMPLEMENTATION-READY
> Peterman, Docs Writer | April 2026 (v5.9 update 2026-04-21)

---

## § Part 1: README.md Outline

**Placement:** Repo root (`README.md`)  
**Audience:** End users, new adopters, anyone running the app  
**Tone:** Confident, plainspoken, safety-first. Lead with lock-screen OSK rehearsal warning. Honest scope: best-effort fail-safe, single-user, ARM64 Surface, paired Nuphy. Explicit non-promises from §1 (PLAN.md).

### README.md Structure

```
# Bluetooth Keyboard Blocker

[One-liner: "Disable your Surface's built-in keyboard whenever your Nuphy Air75 connects."]

⚠️ BEFORE FIRST USE: Rehearse lock-screen On-Screen Keyboard recovery.
[5-minute walk-through: lock screen → touchpad → Accessibility icon → OSK → type PIN.]
If rehearsal fails, do NOT deploy the app without USB keyboard backup.

## What it does
Tray app that disables the Surface Laptop 7 internal keyboard when the Nuphy Air75 (Bluetooth LE) connects, and re-enables it on disconnect, suspend, shutdown, and when you toggle "Active" off via tray menu.

## Scope (what it is NOT)
- **Single-user only.** RDP, Fast User Switch not supported.
- **ARM64 Surface Laptop 7 only.** Not tested on other hardware.
- **Pre-OS scenarios unaddressable.** BitLocker recovery, UEFI, WinRE — keep a USB keyboard.
- **BLE disconnect latency.** Windows takes 10–30 seconds to notice if Nuphy dies. Toggle "Active" off or run `--recover` to speed recovery.
- **No "Escape panic."** Disabling kbdhid removes the device from input stack — no Escape from internal keyboard possible. Recover via tray, OSK, or USB keyboard.
- **Crash persistence.** If app crashes while keyboard disabled, state persists in Windows (`CONFIGFLAG_DISABLED` registry flag) until app launches again and unconditionally re-enables. See [Recovery](#recovery) for procedure.
- **No auto-launch in v0.1.** Manual launch only. See FUTURE.md for v0.2 auto-launch (Scheduled Task).

## Requirements
- **Hardware:** Surface Laptop 7 (15", Snapdragon X Elite, ARM64 Windows).
- **OS:** Windows 11 ARM64.
- **Keyboard:** Nuphy Air75 V3 paired in Windows Settings → Bluetooth.
- **Permissions:** Admin elevation (UAC prompt).

## Install
1. Unzip `kbblock.exe` (single 3 MB portable exe).
2. Run `kbblock.exe`. UAC prompt → **click "Yes"**.
3. Tray icon appears (keyboard symbol, lower-right). **Done.**

No registry, no `Program Files`, delete the exe to uninstall.

## SmartScreen warning
On network download, Windows may show: `"Windows protected your PC. Unknown publisher."` Expected (unsigned in v0.1; signing deferred to v0.2).
- **Click "More info" → "Run anyway".**
- **Or:** Right-click file → Properties → check **"Unblock"** → OK → run again.

## Daily use
- Nuphy connects → tray icon changes → internal keyboard disabled (verified within 2s).
- Nuphy disconnects or battery dies → internal keyboard re-enabled within seconds (or 10–30s if BLE is slow to notice).
- Right-click tray icon → uncheck **"Active"** → internal keyboard works regardless of Nuphy.
- Right-click tray icon → check **"Active"** → resume auto-disable.

## Recovery (all 6 rows from §12, user language)

**Goal:** Internal keyboard not responding? Try these in order of ease:

1. **App running, signed in.** Right-click tray icon → uncheck **"Active"**. Keyboard works immediately.
2. **Tray won't respond (hung instance).** Press **Win+R**, type `<path>\kbblock.exe --recover` → Enter. (Requires touchpad/Nuphy to navigate Run dialog.) App unconditionally re-enables keyboard and exits. Optional: create a desktop shortcut with Target = `<path>\kbblock.exe --recover` for one-click recovery. Then kill hung instance: Task Manager (Ctrl+Shift+Esc) → Processes → `kbblock.exe` → End task.
3. **Signed in, app crashed (no tray).** File Explorer → double-click `kbblock.exe` → UAC → launch-time ENABLE fires.
4. **At lock screen, no keyboard.** Touchpad → **Accessibility icon (lower-right corner)** → **On-Screen Keyboard** → type PIN. (**`Win+Ctrl+O` does NOT work at lock screen.**) If touchpad frozen: plug USB keyboard or use power button + volume-up (built-in firmware recovery keyboard).
5. **Anywhere — universal fallback.** **Plug USB keyboard.** Works on lock screen, UEFI, everywhere. Keep one in your bag.
6. **Last resort.** Hard power-off (hold power 10 seconds). Cold boot → sign in via OSK or USB keyboard → launch app (forces ENABLE).

**Pre-OS (BitLocker, UEFI, WinRE):** USB keyboard is the only universal path.

## Troubleshooting

### "Nuphy paired but keyboard not disabling"
1. Ensure Nuphy is on and within Bluetooth range.
2. In Windows Settings → Bluetooth, toggle Nuphy off, then back on (re-pair).
3. Restart the app.
4. If still not disabled: check `%LOCALAPPDATA%\kbblock\kbblock.log` for "Nuphy not connected" or SetupAPI errors.

### "Verify mismatch: keyboard disable failed, Active toggled off"
Disable succeeded but post-verification reported keyboard still enabled (rare: PnP rebalance, driver stall).
- Right-click tray → check "Active" again. App retries.
- If repeated failures: open Device Manager (Ctrl+X, Device Manager) → Keyboards → right-click internal keyboard → Enable. Restart app.
- File issue with logs from `%LOCALAPPDATA%\kbblock\kbblock.log`.

### "App won't launch: SmartScreen or UAC error"
1. **"Unknown publisher"** UAC on every launch is expected and safe. Click **"Yes"**.
2. **SmartScreen "This file may be unsafe"** after download: Click **"More info" → "Run anyway"**. Or right-click file → Properties → Unblock → OK.
3. Unusual error? Check Windows Event Viewer → Applications and Services Logs for "kbblock" entries.
4. File issue with: Windows version/build, exact error, and logs from `%LOCALAPPDATA%\kbblock\kbblock.log`.

## Rehearsal Guide (before first use)

**Why:** If the app crashes while keyboard disabled and you lack a USB keyboard, OSK is your only recovery. Test it works:

1. Lock your screen (Win+L).
2. At lock screen, move touchpad to **lower-right corner**.
3. Click **Accessibility icon** (wheelchair symbol).
4. Click **"On-Screen Keyboard"** in menu.
5. Type your PIN/password and sign back in.
6. **Success:** You now know how to recover. You're safe.
7. **Failure:** Do NOT use the app without a USB keyboard backup. Contact IT or try on a personal machine first.

## Known limitations
- **Signed state:** Unsigned in v0.1 (SmartScreen warning on download, "Unknown publisher" on launch). Signing deferred to v0.2.
- **Auto-launch:** Manual launch only. Scheduled Task auto-launch deferred to v0.2 (see FUTURE.md).
- **Multi-user / RDP:** Not supported.
- **Logging:** Debug logs to `%LOCALAPPDATA%\kbblock\kbblock.log` (safe for bug reports, no PII).

## FAQ

### Does this work on other Surface models?
Untested. Hardcoded for Laptop 7 (SAM-bus parent, PID 0x006C). File issue with hardware details if you want support for other devices.

### What if Nuphy battery dies during active use?
Windows BLE stack notices within 10–30 seconds. Keyboard re-enables automatically. During window: use tray "Active" toggle or run `--recover`.

### Can I run this via Group Policy?
Yes. Create a logon task that runs `kbblock.exe` with admin privileges. Single portable exe; no dependencies.

### Power consumption?
~2–5 MB RAM, <1% CPU when idle. BLE monitoring is handled by Windows. No battery impact.

## License
[To be determined by owner — recommend MIT or Apache 2.0.]
```

---

## § Part 2: ARCHITECTURE.md Outline

**Placement:** Repo root (`ARCHITECTURE.md`)  
**Audience:** Future maintainers, contributors, anyone modifying the codebase  
**Tone:** Informative, slightly literary, never patronizing. Cite PLAN.md §§ liberally. Implementers + contributors both reference the §4.0 diagram.

### ARCHITECTURE.md Structure

```
# Architecture

Lightweight Windows tray app managing keyboard device state via SetupAPI. Single-user, ARM64 Surface Laptop 7 only. Two threads (message-loop + worker); fail-safe primitives per §4.0 (PLAN.md).

## Why SetupAPI, not hooks or filters?

Previous design (WH_KEYBOARD_LL hook) invalidated because hooks cannot identify source device — they see keycode/scancode, not which keyboard sent it. Disabling via SetupAPI (equivalent to Device Manager → right-click keyboard → Disable) is the proven, standard approach. See PLAN.md §4.1 and prior decisions.md.

## Mechanism (SetupAPI device disable)

`SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, DICS_DISABLE | DICS_ENABLE)` on the exact Surface internal keyboard PnP node. OS writes `CONFIGFLAG_DISABLED` to registry and unloads the driver. Device is fully dormant until re-enabled.

**Important:** `CONFIGFLAG_DISABLED` persists across reboot. Safety is NOT non-persistence; it is "every cold start unconditionally ENABLEs first" (§4.3 Behavior 1). If app crashes mid-disable, keyboard stays disabled until app launches again. Recovery procedure in README.md [Recovery](#recovery).

## Target predicate (3 clauses, all must hold, resolved fresh on every action)

Device is a valid disable target if and only if:
1. `Service == "kbdhid"`
2. `HardwareIds` contains substring `VID_045E&PID_006C`
3. `Parent` device path starts with `{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM`

**Match count check:** predicate must select **exactly one** device. Zero or multiple → refuse disable, fail closed, log full enumeration. Reduced from 7 clauses (v4) after dual-model review; VID/PID + SAM parent is unique on this hardware.

## Fail-safe primitives (5 bullets from §4.0)

- **Tray stays responsive during SetupAPI calls.** All blocking SetupAPI operations run on worker thread. Message loop only makes decisions and queues commands.
- **Stale-result immunity.** Every worker command carries `op_id`; loop increments `current_generation` on state-changing events (tray toggle, resume, quit). Results with `op_id < current_generation` are ignored.
- **Quit must recover even if worker hung.** Quit sends `Cmd::Shutdown`, joins with 500 ms timeout, then performs inline ENABLE + verify on message-loop thread (explicit exception to "no SetupAPI on loop").
- **Worker-death detection.** `SendError` on command queue or sanity-timer probe (`worker_handle.is_finished() == true`) sets `worker_dead=true`, flips `desired_active=false`, routes ENABLE inline. Future ENABLEs also inline; DISABLE permanently refused.
- **`--recover` escape hatch.** Separate argv path skips mutex, calls SetupAPI ENABLE inline (no worker, no message loop), verifies, exits 0 (success) or 1 (verify mismatch). Works even when hung primary instance owns mutex.

## Behaviors (6 rows, PLAN.md §4.3, trigger → action narrative form)

**1. Process launch.** Acquire single-instance mutex `Local\kbblock-singleton-v1` (`WAIT_ABANDONED` = success). **Unconditional ENABLE via worker** + `verify_state` (must report Enabled). If verify fails: log, refuse BLE subscribe, balloon "Recovery failed — see README §12", keep tray alive. Resolve `BluetoothLEDevice` for Nuphy MAC, subscribe `ConnectionStatusChanged`, start 20 s sanity timer, call `apply_policy()`. **`--recover` argv:** skip mutex, **inline ENABLE** (no worker) + `verify_state`, retry once on fail, exit 0 (success) or 1 (mismatch). Shares inline-ENABLE code path with Quit.

**2. BLE connection change.** `BluetoothLEDevice.ConnectionStatusChanged` event (Nuphy MAC 0xCC006219C5FD) → `PostMessage(WM_APP+1)` → message loop calls `apply_policy()`.

**3. Power/session transitions.**
- `WM_QUERYENDSESSION` / `WM_ENDSESSION` / `PBT_APMSUSPEND` (shutdown/logoff/sleep): **unconditional ENABLE** + `verify_state` (logged on fail; cannot block shutdown).
- `PBT_APMRESUMEAUTOMATIC` (lid-open/wake): **ENABLE only** + set `resume_pending=true` + `resume_timestamp`. Do NOT call `apply_policy()` yet (would re-disable at lock screen).
- `WTS_SESSION_UNLOCK`: if `resume_pending`, clear it and call `apply_policy()` (earliest safe moment to re-disable).

**4. Tray toggle Active.** User right-clicks → toggles checkbox. Update `desired_active` → increment `current_generation` → update tooltip → `apply_policy()`.

**4b. Tray Quit.** Set `desired_active=false` → send `Cmd::Shutdown`, join 500 ms. If worker exits cleanly → inline ENABLE + `verify_state`. If timeout → log + inline ENABLE + `verify_state` anyway (stuck worker safer to abandon). Retry ENABLE once on verify fail. Exit 0 regardless (never hang; cold-start ENABLE on next launch is final recovery).

**5. 20 s sanity timer.** If `!resume_pending` AND session active → `apply_policy()`. Check `resume_timeout`: if 2 min since wake without unlock, clear `resume_pending`. Probe worker liveness: if `worker_handle.is_finished() == true` → set `worker_dead=true`, route future ENABLEs inline. Cost: one `ConnectionStatus` read + `WTSQuerySessionInformation` every 20 s when active; zero when disabled.

## Threading model (why two threads)

`SetupDiCallClassInstaller` can block hundreds of ms to seconds (driver unload, PnP rebalance). Running on message loop freezes tray during the window the user is most likely to need recovery.

- **Message-loop thread:** owns tray, hidden HWND_MESSAGE window, BLE-callback receipt, sanity timer, `apply_policy()`, state mutations (`desired_active`, `current_generation`, `op_id`). Never calls SetupAPI.
- **SetupAPI worker thread:** services `mpsc::Receiver<Cmd>` (Cmd = Enable | Disable | VerifyState | Shutdown). Posts results via `PostMessage(WM_APP+3)`. Loop ignores stale results. Single worker serializes SetupAPI calls.

## apply_policy contract (§4.4)

```
fn apply_policy():
  if not desired_active: enable_via_worker(); return
  if not nuphy_connected(): enable_via_worker(); return   // fresh read, no cache
  target = resolve_target_fresh()  // 3-clause predicate
  if target.match_count != 1: enable_via_worker(); log_dump(); return
  
  disable_via_worker(target)  // worker does disable + verify atomically
  current_generation += 1
  
  // Result handler:
  // if stale: ignore
  // else match verify_state:
  //   Disabled => update_tooltip()
  //   Enabled => enable_via_worker(); desired_active=false; 
  //             balloon("disable failed"); log()
  //   Error => enable_via_worker(); log()
```

**All paths fail-safe to ENABLE.** On verify mismatch, ENABLE and flip `desired_active=false` with user notification — no silent poison flag.

## --recover inline path

Shares code with Quit fallback: skip mutex → inline `SetupDiCallClassInstaller(DICS_ENABLE)` → `verify_state()` → exit 0 (success) or 1 (verify mismatch). Works even when hung primary instance owns mutex.

## What v0.2 adds

See FUTURE.md:
- **Signing:** Azure Trusted Signing or OV cert. Removes SmartScreen warning + "Unknown publisher" UAC label.
- **Auto-launch:** Scheduled Task (`kbblock-autolaunch`, at-logon, highest privileges). Toggle in-app to enable/disable. Off by default. If crashed while disabled, user must manually launch on next boot OR have auto-launch enabled.

## Include v4.0 ASCII diagram verbatim

[Copy §4.0 diagram from PLAN.md exactly — implementers + contributors both reference it.]

## Module summary (3 files)

| File | Owns |
|------|------|
| `main.rs` | Mutex, log init, tray + tooltip, hidden HWND_MESSAGE, message loop, `apply_policy()`, state (`desired_active`, `current_generation`, `op_id`, `resume_pending`) |
| `device.rs` | 3-clause predicate, fresh enumeration, `enable()`, `disable()`, `verify_state()`, diagnostic dump on refusal |
| `ble.rs` | `BluetoothLEDevice::FromBluetoothAddressAsync` + `ConnectionStatusChanged` handler, `is_connected()` fresh-read helper |

Worker thread + mpsc channel for Cmd routing. BLE → WM_APP+1, tray → WM_APP+2, worker → WM_APP+3.

## Test matrix (what's on Windows, what's in Docker)

**In Docker / Linux (pure Rust logic):**
- 3-clause predicate evaluation (mocked SetupAPI output)
- BLE MAC parsing
- State machine transitions
- Config load/save
- Log rotation

**On ARM64 Windows (integration only):**
- SetupAPI enumeration, disable, enable, verify
- WM_POWERBROADCAST, WM_QUERYENDSESSION handlers
- BluetoothLEDevice subscription + events
- Tray rendering, menu interaction
- Cold-start ENABLE + verify
- Worker health checks

No mocking of Win32/WinRT on Linux — must test natively on Surface.

## Decision lineage

v1 (hook-only) → invalidated (hook can't identify source). v2 (SetupAPI + task) → rejected (task defeated by Fast Startup). v3 (5-layer + persistence) → v4 (streamlined, 6 behaviors, 3-clause predicate) → v5 (second review) → v5.4 (10 fixes) → v5.9 (implementation-readiness, 8 fixes including 2-thread threading model clarification, disable+verify contract atomicity, worker-death normalizations, manifest embedding, --recover inline path). See decisions.md for full rationale on all major decisions.

## Why Surface SAM parent is durable (not ContainerId)

Surface Laptop 7 internal devices (keyboard, touchpad, buttons) all report sentinel ContainerId `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}` — meaning "no container info." Surface Aggregator Module (SAM) is a custom embedded controller that enumerates ACPI children; they don't inherit composite-device metadata.

**Implication:** Cannot use ContainerId matching. Must use HardwareId substring + Parent-path topology (3-clause predicate). **Never revert to ContainerId without re-testing on actual hardware.** See Spike 2 discovery log in .squad/agents/peterman/v4-deep-dive/discovery-containerid.md.
```

---

## Summary

- **README.md:** Lead with OSK rehearsal warning. Install, daily use, SmartScreen note, all 6 recovery rows (user language), troubleshooting, FAQ, license.
- **ARCHITECTURE.md:** Why SetupAPI. Mechanism + 3-clause predicate. Fail-safe primitives + 6 behaviors (narrative form). Threading model. apply_policy contract. --recover. v0.2 roadmap. Include §4.0 diagram. Module summary. Test matrix. Decision lineage. SAM parent note.
- **Status:** v5.9 ready for implementation. Code lands, docs follow in same PR.

