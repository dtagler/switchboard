# Safety Invariants — Load-Bearing Contracts for v0.1

**Purpose:** Enumeration of the invariants the code MUST never violate. Violations lead to user lockout or silent failure. Newman/Kramer/Elaine must maintain these during implementation; George verifies in review.

**Status:** Pre-implementation design constraints derived from PLAN.md v5.9 §4.

---

## 1. Cold-Start Invariant (§4.3 Row 1)

**Contract:**
Every `main()` entry (normal launch or after crash) calls **unconditional ENABLE via worker + `verify_state`** BEFORE:
- Acquiring BLE device handle
- Subscribing to `ConnectionStatusChanged`
- Starting sanity timer
- Calling `apply_policy()` for the first time

**Rationale:**
If app crashed mid-disable, the `CONFIGFLAG_DISABLED` state persists in registry. Cold start MUST recover this state before doing anything else.

**Violation consequence:**
User boots to login screen with internal keyboard disabled (lockout). OSK or USB keyboard required to recover.

**Implementation checkpoints:**
- `main()` function: first non-log action after mutex is worker ENABLE + verify
- If `verify_state` returns `Disabled` or `Error`, refuse to continue (log loudly, balloon, keep tray alive for manual Quit → retry)

**Test coverage:**
Test 11 (forced crash mid-disable + reboot) validates this invariant.

---

## 2. ENABLE-Path Verify Contract (§4.4, added in v5.2)

**Contract:**
Every recovery ENABLE path must call `verify_state` after the ENABLE operation and treat **only `Enabled` as success**. Specifically:

| Recovery Path | Must Verify |
|---|---|
| Launch (row 1) | ✅ After worker ENABLE |
| `--recover` CLI arg | ✅ After inline ENABLE (retry once on fail) |
| Suspend/shutdown (row 3a) | ✅ After worker ENABLE (log on fail; cannot block shutdown) |
| Quit (row 4b) | ✅ After inline ENABLE (retry once on fail) |

**Rationale:**
SetupAPI can report success but fail to apply the configuration change. If verify shows `Disabled` after ENABLE, the user is locked out.

**Violation consequence:**
Recovery path thinks it succeeded but internal keyboard stays disabled → user locked out.

**Implementation checkpoints:**
- All ENABLE calls (worker or inline) must be followed by `current_state(&target)` call
- `State::Enabled` is the only success; `Disabled` or `Error` logged as "ENABLE verify failed"
- Launch: refuse to continue (balloon, log)
- `--recover`: retry once, then exit 1 (failure code)
- Quit: retry once, then exit 0 anyway (cold-start ENABLE is final recovery)

**Test coverage:**
All tests implicitly check this (internal keyboard must work after recovery). Test 11 (crash recovery) is most critical.

---

## 3. Fail-Closed Predicate (§4.2, §4.4)

**Contract:**
`resolve_target_fresh()` returns `ResolveResult { target: Option<Target>, match_count: usize }`. The 3-clause predicate (VID/PID + kbdhid + BusReportedDeviceDesc) must match **exactly 1** device. If `match_count == 0` or `match_count > 1`:
- Do NOT send `Cmd::Disable`
- Send `Cmd::Enable` instead (fail-safe)
- Log full device enumeration dump for diagnostics

**Rationale:**
- Zero matches: internal keyboard not found (driver issue, hardware failure) → disabling random keyboard is catastrophic
- Multiple matches: ambiguity (driver duplication, USB dock with identical VID/PID) → disabling wrong device is catastrophic

**Violation consequence:**
Wrong device disabled (external USB keyboard, unrelated HID) OR state machine poison (retrying disable on unfindable device).

**Implementation checkpoints:**
- `resolve_target_fresh()` enumerates all `GUID_DEVCLASS_KEYBOARD` devices every call (no cache)
- Increments `match_count` for each device satisfying all three clauses
- Returns `None` if `match_count != 1`
- `apply_policy()` checks `match_count` before sending `Cmd::Disable`

**Test coverage:**
Implicit in all tests (predicate must resolve correctly on target Surface). Negative testing (unplug internal keyboard, dock with duplicate VID/PID) is future work.

---

## 4. No-Cache Invariant (§4.4, §4.6)

**Contract:**
Every `apply_policy()` call reads **fresh state** from:
1. `nuphy_connected()` — fresh `BluetoothLEDevice.ConnectionStatus` read, never cached
2. `resolve_target_fresh()` — fresh SetupAPI enumeration, never cached

**Rationale:**
BLE state can change between calls (disconnect latency, user toggled Active). Device state can change (driver update, USB dock hotplug). Cached reads lead to stale decisions → disable when Nuphy disconnected OR fail to disable when Nuphy connected.

**Violation consequence:**
App disables internal keyboard after Nuphy disconnects (user locked out) OR fails to disable when Nuphy connects (broken core feature).

**Implementation checkpoints:**
- `ble.rs:is_connected()` calls `device.ConnectionStatus()` on every invocation
- `device.rs:resolve_target_fresh()` calls `SetupDiGetClassDevs` + `SetupDiEnumDeviceInfo` on every invocation
- No module-level `static` cache or `OnceCell` for connection status or device list

**Test coverage:**
Tests 2–3 (Nuphy power-cycle) validate BLE fresh reads. Test 8 (sanity timer) validates periodic re-evaluation.

---

## 5. Resume Gating (§4.3 Row 3)

**Contract:**
`PBT_APMRESUMEAUTOMATIC` (lid open from Modern Standby) sets `resume_pending = true` and does NOT call `apply_policy()`. `WTS_SESSION_UNLOCK` (user signed in) clears `resume_pending` and THEN calls `apply_policy()`. If 2 minutes elapse without unlock, sanity timer clears `resume_pending` (timeout).

**Rationale:**
Resume fires before lock screen → if we call `apply_policy()` immediately, internal keyboard disables at lock screen → user locked out.

**Violation consequence:**
User opens lid, Nuphy auto-reconnects, `apply_policy()` fires before unlock → internal keyboard disabled at lock screen (lockout).

**Implementation checkpoints:**
- `PBT_APMRESUMEAUTOMATIC` handler: set `resume_pending = true`, set `resume_timestamp = Instant::now()`, call `ENABLE` (defensive), do NOT call `apply_policy()`
- `WTS_SESSION_UNLOCK` handler: if `resume_pending`, clear it + call `apply_policy()`
- Sanity timer: if `resume_pending` AND `Instant::now() - resume_timestamp > 2 minutes`, clear `resume_pending` (stuck-at-lock-screen timeout)

**Test coverage:**
Tests 6–7 (lid close + lock screen + unlock) validate resume gating.

---

## 6. Worker-Dead Lockdown (§4.5)

**Contract:**
Once `worker_dead = true` (worker thread exited or queue SendError):
- All future `Cmd::Disable` requests are **permanently refused** (no-op)
- All future `Cmd::Enable` requests route **inline** (bypass queue)
- `desired_active` is set to `false` to prevent retries

**Rationale:**
Worker thread panic or channel disconnect means SetupAPI calls can't reach worker. Queueing more commands is futile. Inline ENABLE is the only recovery path.

**Violation consequence:**
App keeps queueing DISABLE commands to dead worker → state machine hangs → user can't toggle Active → manual kill required.

**Implementation checkpoints:**
- `SendError` on `worker_queue.send()` → set `worker_dead = true`, set `desired_active = false`, call inline ENABLE
- Sanity timer: `worker_handle.is_finished() == true` → set `worker_dead = true`, set `desired_active = false`, call inline ENABLE
- `apply_policy()`: if `worker_dead` is true and decision is DISABLE → refuse, log "worker dead, DISABLE refused"
- `apply_policy()`: if `worker_dead` is true and decision is ENABLE → call inline ENABLE (bypass queue)

**Test coverage:**
Not explicitly tested in v0.1 (requires fault injection: worker panic or channel drop). Future work.

---

## 7. Quit-Must-Recover (§4.3 Row 4b)

**Contract:**
Quit handler must:
1. Set `desired_active = false` (stop retrying disable)
2. Send `Cmd::Shutdown` to worker
3. Join worker with 500ms timeout
4. **Always** call inline ENABLE + `verify_state`, whether worker exited cleanly or timeout
5. Retry ENABLE once if `verify_state` returns `Disabled`
6. Exit 0 (success) regardless — never hang

**Rationale:**
If Quit doesn't ENABLE before exiting, internal keyboard stays disabled until next launch. User may close app via Task Manager or Alt+F4 → Quit is the recovery path.

**Violation consequence:**
User quits app (legitimately or accidentally) → internal keyboard stays disabled → locked out until reboot + cold start.

**Implementation checkpoints:**
- Quit handler: `desired_active = false` (prevent `apply_policy()` from queuing DISABLE during shutdown)
- `worker_queue.send(Cmd::Shutdown)` + `worker_handle.join_timeout(500ms)`
- After join (clean or timeout): inline ENABLE (shared with `--recover` code path)
- If `verify_state()` returns `Disabled`, retry ENABLE once, log result
- `exit(0)` regardless (cold-start ENABLE is ultimate recovery)

**Test coverage:**
Test 9 (Quit while disabled) validates this invariant.

---

## 8. `--recover` Inline Path (§4.3 Row 1, §4.5)

**Contract:**
`--recover` CLI arg (checked BEFORE mutex acquisition):
1. Skip single-instance mutex entirely
2. Call inline ENABLE (no worker thread, no mpsc channel) sharing code path with Quit fallback
3. Call `verify_state(&target)` after ENABLE
4. Retry ENABLE once if `verify_state` returns `Disabled`
5. Exit 0 if `verify_state` returns `Enabled`, exit 1 if still `Disabled` after retry

**Rationale:**
If first instance is hung (mutex held, message loop deadlocked), `--recover` is the only escape hatch. Must not depend on mutex, worker thread, or message loop.

**Violation consequence:**
User runs `--recover` while hung instance owns mutex → blocked on mutex acquisition → can't recover without Task Manager + reboot.

**Implementation checkpoints:**
- `main()` first action: check `std::env::args()` for `--recover` BEFORE `CreateMutex`
- If `--recover`: skip all init (mutex, log, tray, worker, BLE), call inline ENABLE + verify, exit
- Inline ENABLE code path must not require worker thread or HWND (no `PostMessage`)

**Test coverage:**
Test 12 (`--recover` while first instance running) validates this invariant.

---

## 9. Stale-Result Immunity (§4.5)

**Contract:**
Worker posts results with `{ op_id: u64, ... }`. Message loop maintains `current_generation: u64`, incremented on state-changing events (tray toggle, resume, quit). On `WM_APP+3` (worker result):
- If `result.op_id < current_generation`: ignore (stale result from superseded operation)
- Else: process result (apply state, call ENABLE on verify mismatch, update tooltip, etc.)

**Rationale:**
User toggles Active off → DISABLE queued with op_id=5 → user toggles Active on before DISABLE completes → ENABLE queued with op_id=6, `current_generation=6` → DISABLE result arrives (op_id=5) → stale, should be ignored.

**Violation consequence:**
Stale DISABLE result overwrites current state → internal keyboard disables when Active=false → broken UX, possible lockout if user doesn't notice.

**Implementation checkpoints:**
- `current_generation` initialized to 0
- Incremented on: tray toggle Active, `PBT_APMRESUMEAUTOMATIC` (resume), Quit (shutdown)
- Worker commands carry `op_id = next_op_id++` (monotonic counter)
- `WM_APP+3` handler: `if result.op_id < current_generation { return; }`

**Test coverage:**
Implicit in Test 10 (rapid Active toggling) and Tests 4–5 (toggle sequence). Explicit validation requires logging op_id and generation per event.

---

## 10. Suspend-Must-ENABLE (§4.3 Row 3a)

**Contract:**
`WM_QUERYENDSESSION`, `WM_ENDSESSION`, and `PBT_APMSUSPEND` handlers must:
1. Send `Cmd::Enable` to worker (or inline if worker dead)
2. Call `verify_state` after ENABLE completes
3. Log if verify fails (cannot block shutdown, but must be visible)
4. Return `TRUE` to allow shutdown to proceed

**Rationale:**
Suspend/shutdown → Nuphy disconnects → user cold-boots or wakes to lock screen. If internal keyboard still disabled, user locked out.

**Violation consequence:**
User closes lid, app doesn't ENABLE → wakes to lock screen with dead internal keyboard (lockout).

**Implementation checkpoints:**
- All three message handlers: set `desired_active = false` (prevent concurrent `apply_policy()` from re-disabling), send `Cmd::Enable`, wait for result, verify
- Cannot block shutdown (Windows kills unresponsive apps) → timeout 500ms, log failure, allow shutdown anyway
- Cold-start ENABLE on next launch is final recovery

**Test coverage:**
Test 6 (lid close suspend) validates `PBT_APMSUSPEND` → ENABLE → verify at lock screen.

---

## 11. Predicate Stability (§4.2)

**Contract:**
The 3-clause predicate must deterministically resolve to the same device across:
- Cold boots
- Driver updates (Windows Update)
- USB dock connect/disconnect
- Bluetooth keyboard pair/unpair
- User-initiated device renames via Device Manager

**Predicate clauses:**
1. `HardwareID` contains `VID_045E&PID_006C` (Microsoft vendor, Surface keyboard product)
2. `Service` == `kbdhid` (HID keyboard driver, excludes composite parents)
3. `BusReportedDeviceDesc` contains "keyboard" case-insensitive (hardware-reported identity, survives renames)

**Rationale:**
Predicate must survive common Windows configuration changes. If predicate breaks (resolves to 0 or >1), app refuses to disable → core feature broken.

**Violation consequence:**
User updates drivers → predicate resolves to 0 → app refuses to disable → always-on internal keyboard (broken feature, but safe).
USB dock with VID_045E&PID_006C keyboard → predicate resolves to >1 → app refuses to disable → safe but broken.

**Implementation checkpoints:**
- Clause 1: VID/PID from `HardwareID` registry property
- Clause 2: `Service` from `Service` registry property
- Clause 3: `BusReportedDeviceDesc` from device property (not `FriendlyName`, which is user-editable)
- On refusal: log full device enumeration (all keyboards, all properties) for diagnostics

**Test coverage:**
All tests implicitly validate predicate on owner's Surface. Negative testing (driver update, USB dock) is manual future work.

---

## 12. Tooltip Truth (UX Invariant)

**Contract:**
Tray tooltip must reflect ground-truth state at all times:
- "Active: On" → `desired_active == true` AND (internal disabled OR waiting to disable)
- "Active: Off" → `desired_active == false` AND (internal enabled OR waiting to enable)

Updated after:
- Tray toggle
- `apply_policy()` ENABLE/DISABLE completion
- Verify mismatch (flips `desired_active` to `false`)

**Rationale:**
Tooltip is the only persistent UI feedback. Lying tooltip → user can't debug state → manual recovery required.

**Violation consequence:**
Tooltip says "Active: On" but internal keyboard works → user doesn't understand why blocking isn't happening → support burden.

**Implementation checkpoints:**
- Tooltip updated in `apply_policy()` after every state change
- Tooltip updated on tray toggle immediately (before `apply_policy()`)
- Tooltip updated on verify mismatch (after flipping `desired_active`)

**Test coverage:**
Implicit in all tests (hover tooltip after state change). Test 10 (verify mismatch) explicitly checks tooltip after recovery.

---

## Implementation Checklist (for Newman/Kramer/Elaine)

Before declaring a module "done":

- [ ] Invariant 1: Cold-start ENABLE is first action in `main()` after mutex
- [ ] Invariant 2: All recovery ENABLE paths call `verify_state` and check for `Enabled`
- [ ] Invariant 3: Predicate refusal (match_count != 1) triggers ENABLE, not DISABLE
- [ ] Invariant 4: No `static` cache for BLE connection status or device list
- [ ] Invariant 5: `PBT_APMRESUMEAUTOMATIC` sets `resume_pending`, does NOT call `apply_policy()`
- [ ] Invariant 6: `worker_dead` flag blocks future DISABLE, routes ENABLE inline
- [ ] Invariant 7: Quit always calls inline ENABLE before exit, retries once, exits 0
- [ ] Invariant 8: `--recover` skips mutex, calls inline ENABLE, exits 0 or 1
- [ ] Invariant 9: `WM_APP+3` handler ignores `result.op_id < current_generation`
- [ ] Invariant 10: Suspend/shutdown handlers call ENABLE + verify, log failure
- [ ] Invariant 11: Predicate uses `BusReportedDeviceDesc` (not `FriendlyName`)
- [ ] Invariant 12: Tooltip updated after every state mutation

---

## Code Review Focus Areas (for George)

When reviewing PRs:

1. **Grep for `apply_policy()`** — every call site must be after fresh `nuphy_connected()` and `resolve_target_fresh()` reads
2. **Grep for `verify_state`** — must appear after every ENABLE in recovery paths (launch, `--recover`, suspend, Quit)
3. **Grep for `Cmd::Disable`** — must only be sent when `match_count == 1`
4. **Grep for `resume_pending`** — must be set on `PBT_APMRESUMEAUTOMATIC`, cleared on `WTS_SESSION_UNLOCK` or timeout
5. **Grep for `worker_dead`** — must block DISABLE, route ENABLE inline
6. **Trace Quit path** — must call inline ENABLE regardless of worker state
7. **Trace `--recover` path** — must skip mutex, call inline ENABLE, exit with status code
8. **Check `current_generation` increment** — on toggle, resume, quit (any state-changing event)
9. **Check result correlation** — `WM_APP+3` handler ignores stale op_id
10. **Check tooltip updates** — after every `apply_policy()` and verify mismatch

---

**End of Safety Invariants. These are the load-bearing contracts. Violate at user's peril.**
