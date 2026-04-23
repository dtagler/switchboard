# Architecture

Lightweight Windows tray app managing keyboard device state via SetupAPI. Single-user, ARM64 Surface Laptop 7 only. Two threads (message-loop + worker); fail-safe primitives.

---

## Why SetupAPI, not hooks or filters?

Earlier design considered `WH_KEYBOARD_LL` hooks, which hook the keyboard input stack. The fatal limitation: hooks receive keycode and scancode, not the source *device*. Impossible to distinguish "this keystroke came from the internal keyboard" vs. "external keyboard." 

SetupAPI device-state changes (Device Manager → right-click → Disable) act on the exact PnP node, with no ambiguity. They fully remove the device from the input stack until re-enabled.

---

## Mechanism (registry `CONFIGFLAG_DISABLED` + PnP re-evaluation)

The disable primitive on Surface Laptop 7 is **not** `SetupDiCallClassInstaller(DICS_DISABLE)` — that call returns `ERROR_NOT_DISABLEABLE (0xE0000231)` for the SAM-bus internal keyboard. Instead, `device::write_config_flag` toggles the `CONFIGFLAG_DISABLED` bit (0x00000001) directly in the device's `HKLM\SYSTEM\CurrentControlSet\Enum\...` registry node, then triggers PnP re-evaluation via `SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, DICS_PROPCHANGE)` (with `CM_Reenumerate_DevNode` as a fallback). Verification reads the flag back. ENABLE is the same flow with the bit cleared.

**Important:** `CONFIGFLAG_DISABLED` persists across reboot. Safety is NOT non-persistence; it is "every cold start unconditionally ENABLEs first" (Behaviors §1). If app crashes mid-disable, keyboard stays disabled until app launches again. Recovery procedure in README.md [Recovery](#recovery).

**Permissions implication:** Failures here are about registry-hive write access (HKLM\SYSTEM\CurrentControlSet\Enum), not installer-class permissions. The manifest's `requireAdministrator` covers it.

---

## Target predicate (3 clauses, all must hold, resolved fresh on every action)

Device is a valid disable target if and only if:

1. `Service == "kbdhid"`
2. `HardwareIds` contains substring `VID_045E&PID_006C`
3. `Parent` device path starts with `{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM`

**Match count check:** predicate must select **exactly one** device. Zero matches or multiple matches → refuse disable, fail closed, log full enumeration. 

**Why all three?** VID/PID is manufacturer-standard (many Surfaces use 045E&006C). SAM (Surface Aggregator Module) parent isolates the *internal* keyboard from any externally-connected HID keyboards. Fresh evaluation on every action (never cached) ensures correctness even if user swaps external keyboards or devices enumerate in different order.

---

## Fail-safe primitives

- **Tray stays responsive during SetupAPI calls.** Once the worker thread exists, all blocking SetupAPI operations run on it. The message loop only makes decisions and queues commands. The two documented exceptions where the loop thread itself touches SetupAPI are (1) cold-start ENABLE (before the worker exists) and (2) Quit / `--recover` (where the worker is gone or deliberately bypassed).
- **Stale-result immunity.** Every worker command carries `op_id`; loop increments `current_generation` on state-changing events (tray toggle, resume, quit). Results with `op_id < current_generation` are ignored.
- **Quit must recover even if worker hung.** Quit sends `Cmd::Shutdown`, joins with 500 ms timeout, then performs inline ENABLE + verify on message-loop thread (explicit exception to "no SetupAPI on loop").
- **Worker-death detection.** `SendError` on command queue or sanity-timer probe (`worker_handle.is_finished() == true`) sets `worker_dead=true`, flips `desired_active=false`, routes ENABLE inline. Future ENABLEs also inline; DISABLE permanently refused.
- **`--recover` escape hatch.** Separate argv path skips mutex, calls SetupAPI ENABLE inline (no worker, no message loop), verifies, exits 0 (success) or 1 (verify mismatch). Works even when hung primary instance owns mutex.

---

## Behaviors

### 1. Process launch

Acquire single-instance mutex `Local\switchboard-singleton-v1` (`WAIT_ABANDONED` as success). **Inline ENABLE on the main thread + `verify_state`** (must report Enabled) — this runs *before* the worker thread is spawned, so it is the one explicit exception to "no SetupAPI on the loop thread." If verify fails: log loudly, refuse BLE subscribe, log a "Recovery failed — see log" message, keep tray alive. The SetupAPI worker thread is created immediately after.

**`--recover` argv:** Skip mutex → **inline ENABLE** (no worker, no message loop) + `verify_state`. Retry once on fail. Exit 0 (success) or 1 (verify mismatch). Works even when primary instance owns mutex.

### 2. BLE connection change

`BluetoothLEDevice.ConnectionStatusChanged` event (target Nuphy MAC, runtime-loaded from `SWITCHBOARD_NUPHY_BD_ADDR`) → `PostMessage(WM_APP+1)` → message loop calls `apply_policy()`.

### 3. Power/session transitions

- `WM_QUERYENDSESSION` / `WM_ENDSESSION` / `PBT_APMSUSPEND` (shutdown/logoff/sleep): **unconditional ENABLE** + `verify_state` (logged on fail; cannot block shutdown).
- `PBT_APMRESUMEAUTOMATIC` (lid-open/wake): **ENABLE only** + set `resume_pending=true` + `resume_timestamp`. Do NOT call `apply_policy()` yet (would re-disable at lock screen).
- `WTS_SESSION_UNLOCK`: if `resume_pending`, clear it and call `apply_policy()` (earliest safe moment to re-disable).

### 4. Tray toggle Active

User right-clicks → toggles checkbox. Update `desired_active` → increment `current_generation` → update tooltip → `apply_policy()`.

### 4b. Tray Quit

Set `desired_active=false` → send `Cmd::Shutdown`, join 500 ms. If worker exits cleanly → inline ENABLE + `verify_state`. If timeout → log + inline ENABLE + `verify_state` anyway (stuck worker safer to abandon than leave keyboard disabled). Retry ENABLE once on verify fail. Exit 0 regardless (never hang; cold-start ENABLE on next launch is final recovery).

### 5. 20 s sanity timer

If `!resume_pending` AND session active → `apply_policy()`. Check `resume_timeout`: if 2 min since wake without unlock, clear `resume_pending`. Probe worker liveness: if `worker_handle.is_finished() == true` → set `worker_dead=true`, route future ENABLEs inline. Cost: one `ConnectionStatus` read + `WTSQuerySessionInformation` every 20 s when active; zero when disabled.

---

## Threading model (why two threads)

`SetupDiCallClassInstaller` can block hundreds of ms to seconds (driver unload, PnP rebalance). Running on message loop freezes tray during exactly the window the user is most likely to need recovery.

- **Message-loop thread:** owns tray, hidden top-level message-only window, BLE-callback receipt, sanity timer, `apply_policy()`, state mutations (`desired_active`, `current_generation`, `op_id`). Calls SetupAPI only in the documented exception cases (cold-start ENABLE before worker exists; Quit / `--recover` inline ENABLE).
- **SetupAPI worker thread:** services `mpsc::Receiver<Cmd>` (`Cmd = Enable | Disable | Shutdown`). Posts results via `PostMessage(WM_APP+3)`. Loop ignores stale results. Single worker serializes SetupAPI calls.

**Disable + verify contract:** `Cmd::Disable` performs disable + post-condition verify on the worker thread atomically and posts a single `WM_APP+3` message carrying `{op_id, disable_ok: bool, verify_state}`.

---

## `apply_policy()` contract (pseudocode)

```
fn apply_policy():
    if not desired_active:
        enable_via_worker()
        return

    if not nuphy_connected():
        enable_via_worker()
        return
    
    target = resolve_target_fresh()  // 3-clause predicate, no cache
    if target.match_count != 1:
        enable_via_worker()
        log_dump()
        return
    
    disable_via_worker(target, op_id=next)
    current_generation += 1
    
    // Result handler (WM_APP+3):
    // if result.op_id < current_generation: ignore (stale)
    // else:
    //   match result.verify_state:
    //     Disabled => update_tooltip()
    //     Enabled  => enable_via_worker()
    //                 set desired_active=false
    //                 notify_user("Keyboard disable failed")  // log-only in v0.1
    //                 log("verify mismatch")
    //     Error    => enable_via_worker(); log(e)
```

**All paths fail-safe to ENABLE.** On verify mismatch, ENABLE and flip `desired_active=false` and log loudly (see [`notify_user`] — no balloon in v0.1, log only) — no silent poison flag. User can toggle Active back on when ready.

---

## `--recover` escape hatch

Separate argv path bypasses the single-instance mutex, clears `CONFIGFLAG_DISABLED` and triggers PnP re-evaluation via `SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, DICS_PROPCHANGE)` inline on the calling thread (no worker, no message loop), verifies state, and exits 0 (success) or 1 (verify mismatch). Works even when a hung primary instance owns the mutex.

**Usage:** `switchboard.exe --recover` via Win+R, Explorer address bar, or desktop shortcut. Requires admin elevation (UAC), but runs outside the message-loop event serialization — safe escape hatch when primary instance is hung or crashed.

Shares code path with Quit's inline fallback.

---

## Architecture Diagram

```
┌────────────────────────────────────────────────────────────────────────────────────┐
│ EXTERNAL EVENTS                                                                    │
│  BLE ConnectionStatusChanged │ Power/Session │ Sanity Timer │ Tray │ argv         │
└──────────────────────────┬─────────────────────────────────────────────────────────┘
                           │
                           v PostMessage
┌────────────────────────────────────────────────────────────────────────────────────┐
│ MAIN THREAD (main.rs)                                                              │
│                                                                                     │
│  Mutex → Cold Start: ENABLE via worker + verify → BLE setup → Message Loop        │
│                                                                                     │
│  Message Loop receives events → apply_policy()                                     │
│                                                                                     │
│  ┌────────────────────────────────────────────────────┐                            │
│  │ apply_policy()                                     │                            │
│  │  Inputs: desired_active, nuphy_connected(),        │                            │
│  │          predicate (3-clause, fresh each call)     │                            │
│  │  Output: Send Cmd::Enable OR Cmd::Disable         │────┐                       │
│  └────────────────────────────────────────────────────┘    │                       │
│                                                             │ mpsc channel          │
│  State: desired_active, resume_pending, current_generation,│ (Cmd enum)            │
│         worker_dead (bool)                                 │                       │
│                                                             v                       │
│  Quit: Send Cmd::Shutdown, join 500ms ──> inline ENABLE ───┼──┐                   │
│  Worker-death: SendError / is_finished() ──> inline ENABLE─┼──┤                   │
│                                                             │  │                   │
└─────────────────────────────────────────────────────────────┼──┼───────────────────┘
                                                               │  │
                                                               v  │ inline (bypass)
┌────────────────────────────────────────────────────────────────┼───────────────────┐
│ SETUPAPI WORKER THREAD (device.rs)                            │                   │
│                                                                │                   │
│  mpsc::Receiver<Cmd>                                           │                   │
│   ├─ Cmd::Enable { op_id } ───┐                               │                   │
│   ├─ Cmd::Disable { target, op_id }                           │                   │
│   └─ Cmd::Shutdown            │                               │                   │
│                               v                                │                   │
│  SetupAPI registry+PnP enable/disable (CONFIGFLAG_DISABLED + DICS_PROPCHANGE) <─┘   │
│  disable_and_verify() (atomic disable + post-condition verify, see Disable+verify contract) │
│                            │                                                       │
│                            v                                                       │
│  PostMessage(result + op_id) ──> back to Main Loop                                │
│                                  (stale if op_id < current_generation)             │
│                                                                                    │
└────────────────────────────────────────────────────────────────────────────────────┘
                             │
                             v
         ┌───────────────────────────────────────────────────────┐
         │ TARGET: Surface Internal Keyboard                     │
         │ VID_045E&PID_006C, Parent=SAM-bus, Service=kbdhid     │
         │ (CONFIGFLAG_DISABLED persists across reboot)          │
         └───────────────────────────────────────────────────────┘

argv --recover: Skip mutex → inline ENABLE (no worker) → verify → exit 0/1
```

---

## Safety invariants (12 items)

The implementation guarantees 12 safety invariants. Key ones:

1. **Cold-start ENABLE first** — Before any BLE subscription or policy check, unconditional ENABLE + verify.
2. **All recovery ENABLEs verify** — Every path back to ENABLE reads post-condition state to confirm.
3. **Predicate fail-closed** — Zero or multiple matches → ENABLE, never DISABLE.
4. **No cache** — Fresh reads on every policy decision (Nuphy state, predicate).
5. **Resume gating** — `resume_pending` flag prevents re-disable at lock screen until session unlock.
6. **Worker-dead lockdown** — Once worker exits unexpectedly, all future ENABLEs route inline; DISABLE permanently refused.
7. **Quit-must-recover** — Quit always performs inline ENABLE + verify, even if worker hung.
8. **`--recover` inline path** — Skips mutex and worker; safe escape hatch when primary stuck.
9. **Stale-result immunity** — Results with `op_id < current_generation` ignored.
10. **Suspend-must-ENABLE** — Power transitions (suspend, logoff, shutdown) unconditionally ENABLE.
11. **Tooltip truth** — Tray tooltip always reflects actual keyboard state (Enabled vs. Disabled).
12. **Manifest `requireAdministrator`** — UAC prompt at every launch; SetupAPI device-state changes require admin. Self-elevation via `ShellExecuteExW "runas"` is also used for the boot-task install/uninstall subcommands when launched from a non-elevated parent.

---

## Message constants

| Constant | Value | Source | Handler |
|----------|-------|--------|---------|
| `WM_APP+1` | `0x8401` | BLE `ConnectionStatusChanged` | Calls `apply_policy()` |
| `WM_APP+2` | `0x8402` | Tray menu selection | Toggles "Active", calls `apply_policy()` or triggers Quit |
| `WM_APP+3` | `0x8403` | Worker thread result | Processes `DisableResult`, updates tooltip, handles verify mismatch |

---

## Build stack

- **Container:** Rust 1.90 with cargo-xwin 0.18.4
- **Target:** aarch64-pc-windows-msvc
- **Linker:** lld (via cargo-xwin)
- **Profile:** opt-level=z, lto=true, codegen-units=1, strip=true, panic=abort
- **Output:** Single ~390 KB ARM64 PE exe, zero host dependencies, console-window suppressed in release builds

Build from host via `.\scripts\build.ps1` (Windows) or `./scripts/build.sh` (Linux/macOS).

---

## File layout

| File | Purpose |
|------|---------|
| `src/main.rs` | Bootstrap, mutex, tray, message loop, `apply_policy()`, state management, self-elevation via `ShellExecuteExW "runas"` |
| `src/device.rs` | 3-clause predicate, registry CONFIGFLAG_DISABLED toggle + PnP re-evaluation, ENABLE/DISABLE/verify, diagnostics |
| `src/autostart.rs` | Per-user autostart via Task Scheduler logon task (`switchboard-logon`, `RunLevel=HighestAvailable`). Pre-elevates the token at logon so the tray comes up silently despite `requireAdministrator`. |
| `src/boot_task.rs` | Task Scheduler 2.0 COM module — system-level boot recovery task `switchboard-boot-recover` (admin required) |
| `src/ble.rs` | `BluetoothLEDevice` subscription, `is_connected()` fresh-read helper, `.env` BD_ADDR loader |
| `src/theme.rs` | Light/dark taskbar theme detection (`SystemUsesLightTheme` registry value + `WM_SETTINGCHANGE`) for tray-icon swap |
| `build.rs` | Manifest embedding via `embed-resource` crate |
| `manifest/switchboard.exe.manifest` | `requireAdministrator` declaration |
| `manifest/switchboard.rc` | Manifest resource link |
| `docker/Dockerfile.build` | cargo-xwin container |
| `scripts/build.ps1` | Windows build driver |
| `scripts/build.sh` | Linux/macOS build driver |

---

## Recovery model (cold-start ENABLE / Quit ENABLE / --recover inline)

- **Crash while disabled:** User launches app on next boot → cold-start ENABLE fires unconditionally.
- **Quit while disabled:** Tray Quit → inline ENABLE + verify before process exits.
- **Hung instance:** `--recover` argv path → inline ENABLE (bypasses mutex) → verify → exit.

All three paths share the same ENABLE + verify contract. No cached state can poison recovery.

---

## Resume gating (suspend/wake/unlock flow)

1. Suspend: `PBT_APMSUSPEND` → unconditional ENABLE.
2. Wake: `PBT_APMRESUMEAUTOMATIC` → ENABLE only + set `resume_pending=true` + timestamp.
3. Unlock: `WTS_SESSION_UNLOCK` → if `resume_pending`, clear it + call `apply_policy()` (re-disable if desired).
4. Timeout: If 2 min since wake without unlock, clear `resume_pending` (user left screen locked).

**Why gating?** Lock screen has no input method (except Nuphy, which is cut off before user signs in). Gating prevents re-disable at lock screen.

---

## Why Surface SAM parent is durable (not ContainerId)

Surface Laptop 7 internal devices (keyboard, touchpad, buttons) all report sentinel ContainerId `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}` — meaning "no container info." Surface Aggregator Module (SAM) is a custom embedded controller that enumerates ACPI children; they don't inherit composite-device metadata.

**Implication:** Cannot use ContainerId matching. Must use HardwareId substring + Parent-path topology (3-clause predicate). **Never revert to ContainerId without re-testing on actual hardware.** This is a hardware constant, not a Windows bug.

---

## Decision lineage

v1 (hook-only) → invalidated (hook can't identify source). v2 (SetupAPI + task) → rejected (task defeated by Fast Startup). v3 (5-layer + persistence) → v4 (streamlined, 6 behaviors, 3-clause predicate) → v5 (second review) → v5.4 (10 fixes) → v5.9 (implementation-readiness, 8 fixes including 2-thread threading model clarification, disable+verify contract atomicity, worker-death normalization, manifest embedding, --recover inline path).

---

**Questions?** Start with README.md [Troubleshooting](#troubleshooting) or check `%LOCALAPPDATA%\switchboard\switchboard.log`.
