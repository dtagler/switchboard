> **ARCHIVED — superseded by v5 (10 tests, tier-based). See ../test-recipe.md.**

# v0.1 Owner Test Recipe — Acceptance Gate

**Purpose:** Executable test protocol for Newman/Kramer/Elaine code verification and owner acceptance. All 12 tests must pass before v0.1 release.

**Status:** Pre-implementation. This document defines the target; code must satisfy these criteria.

---

## Prerequisites (MUST verify before starting)

1. **Hardware:** Surface Laptop 7 15", Snapdragon X Elite, Windows 11 ARM64
2. **Nuphy Air75:** Paired (Windows Settings → Bluetooth → Devices), powered on, connected (BD_ADDR `CC:00:62:19:C5:FD`)
3. **External USB keyboard:** Available as safety backup, within arm's reach
4. **Lock-screen OSK rehearsal FIRST:** Before running any tests:
   - Lock screen (Win+L)
   - Touchpad → lower-right corner → Accessibility icon → On-Screen Keyboard
   - Verify OSK appears and allows typing
   - Unlock, return to desktop
   - **This is the recovery path if tests fail catastrophically.**
5. **Log directory:** Create `%LOCALAPPDATA%\kbblock\` if missing
6. **Clean state:** No `kbblock.exe` processes running (`Get-Process kbblock` should error)

---

## Safety Net Protocol (Before EVERY Test)

1. **USB keyboard plug test:**
   - Unplug USB keyboard
   - Wait 2 seconds
   - Plug USB keyboard back in
   - Type three characters in Notepad
   - **PASS:** Characters appear
   - **FAIL:** STOP. Reboot. Do not proceed with testing.

2. **Log truncation:**
   - Delete `%LOCALAPPDATA%\kbblock\kbblock.log` (if exists)
   - This ensures each test captures only its own log events

3. **Internal keyboard baseline:**
   - Type three characters on internal keyboard in Notepad
   - **PASS:** Characters appear
   - **FAIL:** STOP. Reboot before continuing.

---

## Smoke Tests (§9 from PLAN.md)

### Test 1: Launch with Nuphy Connected

**Setup:**
- Nuphy powered on and connected (verify in Windows Bluetooth settings)
- Internal keyboard confirmed working (baseline check)

**Steps:**
1. Launch `kbblock.exe` via File Explorer double-click
2. UAC prompt appears → Click "Yes" (touchpad or mouse)
3. Wait 2 seconds
4. Tray icon appears in system tray
5. Open Notepad, click in text area
6. Attempt to type on **internal keyboard**

**Pass Criteria:**
- Internal keyboard produces no characters within 2 seconds of tray icon appearing
- Log shows: `ENABLE → verify: Enabled → BLE subscribe → DISABLE → verify: Disabled`

**Fail Observations:**
- Characters appear from internal keyboard → DISABLE failed
- Tray icon never appears → app crashed during init
- UAC prompt never appears → manifest not embedded
- Log shows `verify mismatch` → SetupAPI succeeded but device still enabled (adversarial test #10 scenario)

**On Fail:** Right-click tray → Quit (if visible). Run `kbblock.exe --recover` from Win+R. Verify internal keyboard works. Save log to `test1-fail.log`.

**On Pass:** Save log to `test1-pass.log`. Leave app running.

---

### Test 2: Nuphy Disconnect

**Setup:**
- Test 1 passed; app running; internal keyboard disabled

**Steps:**
1. Power off Nuphy (hold power button 3s → LED off)
2. Wait 3 seconds (BLE stack disconnect latency)
3. Attempt to type on **internal keyboard**

**Pass Criteria:**
- Internal keyboard produces characters within 2 seconds of Nuphy power-off
- Log shows: `ConnectionStatusChanged → apply_policy → ENABLE`

**Fail Observations:**
- Internal keyboard stays dead after 3 seconds → BLE event missed OR ENABLE failed
- After 20 seconds, still dead → sanity timer didn't rescue

**On Fail:** Right-click tray → Quit. Run `--recover`. Save log to `test2-fail.log`.

**On Pass:** Save log to `test2-pass.log`. Leave app running, Nuphy powered off.

---

### Test 3: Nuphy Reconnect

**Setup:**
- Test 2 passed; app running; Nuphy off; internal keyboard working

**Steps:**
1. Power on Nuphy (press power button → LED blinks → solid)
2. Wait 3 seconds for BLE reconnect
3. Attempt to type on **internal keyboard**

**Pass Criteria:**
- Internal keyboard produces no characters within 2 seconds of Nuphy LED going solid
- Log shows: `ConnectionStatusChanged → apply_policy → DISABLE → verify: Disabled`

**Fail Observations:**
- Internal keyboard still works → BLE event missed OR disable didn't fire
- After 20 seconds, still works → sanity timer didn't correct

**On Fail:** Quit app, run `--recover`, save log to `test3-fail.log`.

**On Pass:** Save log to `test3-pass.log`. Leave app running, Nuphy on.

---

### Test 4: Tray Toggle — Disable via Active Uncheck

**Setup:**
- Test 3 passed; app running; Nuphy on; internal keyboard disabled

**Steps:**
1. Right-click tray icon
2. Click **"Active"** to uncheck it
3. Wait 2 seconds
4. Attempt to type on **internal keyboard**

**Pass Criteria:**
- Internal keyboard produces characters
- Log shows: `desired_active=false → apply_policy → ENABLE`
- Tooltip changes (hover tray icon) to reflect Active=false state

**Fail Observations:**
- Internal keyboard stays dead → ENABLE didn't fire OR verify mismatch
- Menu item doesn't toggle → tray message pump broken

**On Fail:** Run `--recover`, save log to `test4-fail.log`.

**On Pass:** Save log to `test4-pass.log`. Leave app running, Active off.

---

### Test 5: Tray Toggle — Enable via Active Check

**Setup:**
- Test 4 passed; app running; Nuphy on; Active off; internal keyboard working

**Steps:**
1. Right-click tray icon
2. Click **"Active"** to check it
3. Wait 2 seconds
4. Attempt to type on **internal keyboard**

**Pass Criteria:**
- Internal keyboard produces no characters
- Log shows: `desired_active=true → apply_policy → DISABLE → verify: Disabled`

**Fail Observations:**
- Internal keyboard still works → DISABLE failed OR predicate resolved to zero/multiple

**On Fail:** Quit, `--recover`, save log to `test5-fail.log`.

**On Pass:** Save log to `test5-pass.log`. Leave app running, Active on, Nuphy on.

---

### Test 6: Lid Close (Modern Standby Suspend)

**Setup:**
- Test 5 passed; app running; Nuphy on; Active on; internal keyboard disabled

**Steps:**
1. Close Surface lid
2. Wait 30 seconds (enter Modern Standby)
3. Open lid
4. Lock screen appears → use touchpad to verify **Accessibility icon is clickable** (do NOT unlock yet)
5. Attempt to type on **internal keyboard** at lock screen

**Pass Criteria:**
- Internal keyboard produces characters at lock screen
- Log shows (after unlock): `PBT_APMSUSPEND → ENABLE → verify: Enabled`

**Fail Observations:**
- Internal keyboard dead at lock screen → suspend ENABLE failed (user locked out)
- After unlock, internal keyboard works but never re-disables → `PBT_APMRESUMEAUTOMATIC` didn't set `resume_pending` OR `WTS_SESSION_UNLOCK` didn't fire

**On Fail (at lock screen):** OSK via Accessibility icon, unlock, quit app, `--recover`, save log to `test6-fail-lockscreen.log`. **This is a critical lockout failure.**

**On Pass (at lock screen, internal works):** Unlock. Wait 2 seconds. Attempt to type on internal keyboard. Should be disabled again. Save log to `test6-pass.log`. Leave app running.

---

### Test 7: Lid Close (Post-Unlock Re-Disable)

**Setup:**
- Test 6 passed at lock screen; user unlocked; waiting on desktop

**Steps:**
1. Verify Nuphy still connected (Windows Bluetooth → Devices)
2. Attempt to type on **internal keyboard**

**Pass Criteria:**
- Internal keyboard produces no characters
- Log shows: `WTS_SESSION_UNLOCK → resume_pending cleared → apply_policy → DISABLE → verify: Disabled`

**Fail Observations:**
- Internal keyboard works → resume gating failed OR `apply_policy` wasn't called on unlock

**On Fail:** Quit, `--recover`, save log to `test7-fail.log`.

**On Pass:** Save log to `test7-pass.log`. Right-click tray → Quit to clean up for adversarial tests.

---

### Test 8: Sanity Timer (Passive Drift Correction)

**Setup:**
- Fresh launch; Nuphy on; Active on; internal disabled

**Steps:**
1. Launch app
2. Wait for internal keyboard to disable
3. Power off Nuphy (simulates missed BLE event)
4. Do NOT interact with tray
5. Wait 25 seconds

**Pass Criteria:**
- Internal keyboard starts working within 25 seconds
- Log shows: `sanity_timer_tick → apply_policy → ENABLE` (sanity timer fires at 20s intervals; allow 5s margin)

**Fail Observations:**
- After 25 seconds, internal keyboard still dead → sanity timer didn't fire OR `apply_policy` didn't ENABLE

**On Fail:** Quit, `--recover`, save log to `test8-fail.log`.

**On Pass:** Save log to `test8-pass.log`. Quit app.

---

### Test 9: Quit While Disabled

**Setup:**
- Fresh launch; Nuphy on; Active on; internal disabled

**Steps:**
1. Launch app
2. Verify internal keyboard disabled
3. Right-click tray → **Quit**
4. Immediately attempt to type on internal keyboard (within 1 second)
5. Observe tray icon disappearance

**Pass Criteria:**
- Internal keyboard produces characters BEFORE tray icon disappears
- Log shows: `Quit → Shutdown sent → worker join (success or timeout) → inline ENABLE → verify: Enabled → exit 0`

**Fail Observations:**
- Internal keyboard stays dead after tray icon vanishes → Quit ENABLE failed (user locked out)
- Process hangs (tray icon stays forever) → message loop blocked

**On Fail:** Reboot, use OSK at login or plug USB keyboard, save log to `test9-fail.log`. **Critical lockout failure.**

**On Pass:** Save log to `test9-pass.log`.

---

## Adversarial Tests (§9.1 from PLAN.md)

### Test 10: Verify Mismatch (Top Priority per v5.4)

**Purpose:** Exercise the verify-failure recovery path where `SetupAPI` reports success but `verify_state` finds keyboard still enabled. This is the "disable failed silently" scenario.

**Setup:**
- App running; Nuphy on; Active on

**Steps:**
1. Right-click tray → check Active (trigger disable)
2. **Immediately** toggle Active off and on rapidly 3 times within 2 seconds (race condition attempt)
3. Watch for tray balloon notification
4. Hover tray icon, read tooltip

**Expected Behavior (if verify mismatch triggered):**
- Tray balloon appears: "Keyboard disable failed — toggled Active off. Restart app to retry."
- Tooltip shows Active=false
- Internal keyboard produces characters (ENABLE was called)
- Log shows: `DISABLE → verify: Enabled (mismatch) → ENABLE → desired_active=false → balloon`

**Pass Criteria:**
- If verify mismatch occurs, recovery is automatic (ENABLE + flip Active off + notify user)
- No silent state poison (app doesn't keep retrying disable)
- User can toggle Active back on when ready

**Alternate (if mismatch doesn't trigger in 3 attempts):**
- Log still shows correct `DISABLE → verify: Disabled` sequences
- Internal keyboard behavior matches Active state
- This means the race wasn't fast enough; predicate+verify contract held

**Fail Observations:**
- Verify mismatch occurs but no balloon → UX contract broken
- Verify mismatch occurs, Active stays on, app keeps retrying → state poison
- Verify mismatch occurs, internal keyboard stays disabled → ENABLE didn't fire (lockout)

**On Fail:** Quit, `--recover`, save log to `test10-fail.log`.

**On Pass:** Save log to `test10-pass.log`. Quit app.

---

### Test 11: Forced Crash Mid-Disable (Critical Lockout Test)

**Purpose:** Simulate app crash while internal keyboard is disabled. Verify cold-boot ENABLE recovery via §12.

**Setup:**
- Nuphy on and connected
- Fresh app launch

**Steps:**
1. Launch `kbblock.exe`
2. Verify internal keyboard disabled
3. Open PowerShell as Administrator
4. Run: `Get-Process kbblock | Select-Object -ExpandProperty Id` (capture PID)
5. Run: `Stop-Process -Id <captured_pid> -Force`
6. Verify tray icon vanishes immediately
7. Attempt to type on internal keyboard (should still be dead)
8. **Reboot Surface** (Start → Power → Restart)
9. At lock screen, attempt to type on internal keyboard

**Pass Criteria (at lock screen after reboot):**
- Internal keyboard is DEAD (disabled state persisted via `CONFIGFLAG_DISABLED`)
- Recovery via §12 row 4: Touchpad → Accessibility icon → OSK → type password → unlock
- After unlock, launch `kbblock.exe` via File Explorer
- UAC prompt → Yes
- Internal keyboard starts working within 2 seconds
- Log shows: `main() → ENABLE → verify: Enabled → ...`

**Fail Observations:**
- At lock screen, internal keyboard works → SetupAPI disable didn't persist (surprising but safe)
- After launching app post-unlock, internal keyboard stays dead → cold-start ENABLE failed (lockout)

**On Fail (post-unlock, internal still dead):** Run `kbblock.exe --recover` from Win+R. Save log to `test11-fail.log`. **Critical lockout failure.**

**On Pass:** Save log to `test11-pass.log`.

---

### Test 12: Hung Instance + `--recover` Escape Hatch

**Purpose:** Verify `--recover` bypasses mutex and ENABLEs inline even while first instance holds mutex.

**Setup:**
- No `kbblock.exe` running

**Steps:**
1. Launch `kbblock.exe` normally
2. Verify internal keyboard disabled (Nuphy on)
3. Open another File Explorer window
4. Navigate to `kbblock.exe` location
5. In Explorer address bar, type: `kbblock.exe --recover` and press Enter
6. UAC prompt → Yes
7. Wait 2 seconds

**Pass Criteria:**
- No error message about mutex
- Internal keyboard produces characters
- First instance tray icon still present (first instance keeps running)
- Log from `--recover` invocation shows: `--recover mode → skip mutex → inline ENABLE → verify: Enabled → exit 0`

**Fail Observations:**
- Error about single instance or mutex conflict → `--recover` tried to acquire mutex
- Internal keyboard stays disabled → inline ENABLE failed
- First instance crashed → mutex not properly skipped

**On Fail:** Kill both processes, reboot if needed, save both logs to `test12-fail-instance1.log` and `test12-fail-recover.log`.

**On Pass:** Save logs to `test12-pass-instance1.log` and `test12-pass-recover.log`. Quit first instance.

---

## Recovery Drill Matrix (§12 Row-by-Row Rehearsal)

**Purpose:** Rehearse each row of §12 PLAN.md under controlled conditions before shipping.

### Drill 1: Row 1 — Tray Uncheck Active
- **Scenario:** App running normally, internal disabled
- **Action:** Right-click tray → uncheck Active
- **Expected:** Internal keyboard works immediately
- **Status after Test 4:** ✅ Verified

### Drill 2: Row 2 — `--recover` While Hung
- **Scenario:** First instance unresponsive (simulated by running normally)
- **Action:** `kbblock.exe --recover` from Win+R or Explorer address bar
- **Expected:** Internal keyboard works; first instance keeps running
- **Status after Test 12:** ✅ Verified

### Drill 3: Row 3 — Double-Click Launch After Crash
- **Scenario:** App crashed mid-disable (simulated by `Stop-Process`)
- **Action:** File Explorer → `kbblock.exe` → double-click → UAC → Yes
- **Expected:** Cold-start ENABLE fires, internal keyboard works
- **Status after Test 11:** ✅ Verified

### Drill 4: Row 4 — Lock-Screen OSK
- **Scenario:** Internal keyboard disabled at lock screen
- **Action:** Touchpad → lower-right Accessibility icon → On-Screen Keyboard
- **Expected:** OSK appears, can type password, unlock
- **Status after Test 6:** ✅ Verified (rehearsal + actual lockout test)

### Drill 5: Row 5 — USB Keyboard Hard Fallback
- **Scenario:** All software paths failed
- **Action:** Plug in USB keyboard
- **Expected:** Can type password at lock screen, sign in, launch app or `--recover`
- **Status:** ✅ Verified in safety net protocol (every test)

### Drill 6: Row 6 — Hard Power-Off
- **Scenario:** Lock screen + no OSK response + no USB keyboard available
- **Action:** Hold power button 10 seconds (hard power-off) → cold boot → row 4 or 5
- **Expected:** Boot to lock screen, OSK or USB keyboard works
- **Status:** Manual rehearsal required (not part of automated tests)

---

## Exit Criteria

**All of the following must be TRUE:**

1. ✅ Tests 1–9 (smoke) pass with correct log sequences
2. ✅ Tests 10–12 (adversarial) pass with correct recovery behavior
3. ✅ No unexpected tray balloon warnings (except Test 10 if verify mismatch triggered)
4. ✅ Logs show expected ENABLE/DISABLE/verify sequences per §4.3 behaviors
5. ✅ Recovery drills 1–6 rehearsed and documented
6. ✅ Lock-screen OSK path verified in Tests 6 and 11
7. ✅ USB keyboard fallback verified in every safety net check
8. ✅ No `worker_dead` transitions unless intentionally forced (not part of current tests)
9. ✅ Op-id correlation works: no stale results accepted (implicit in all toggle tests)
10. ✅ Verify mismatch recovery documented (Test 10 + log analysis)

**Failures that BLOCK v0.1:**

- Test 6 fail at lock screen (user locked out after suspend)
- Test 9 fail (user locked out after Quit)
- Test 11 fail post-unlock (cold-start ENABLE doesn't recover)
- Test 12 fail (no hung-instance escape hatch)

**Failures that are warnings but don't block (document in known issues):**

- Test 8 sanity timer takes >25s (BLE latency tolerance)
- Test 10 verify mismatch never triggers during racing (predicate is robust)

---

## Log Capture Protocol

**Before each test:**
1. Delete `%LOCALAPPDATA%\kbblock\kbblock.log`
2. Launch app (if required by test)

**After each test:**
1. Copy `%LOCALAPPDATA%\kbblock\kbblock.log` to `test<N>-<pass|fail>.log` in test results directory
2. Verify log contains:
   - Timestamp per event
   - `ENABLE → verify: Enabled` on recovery paths
   - `DISABLE → verify: Disabled` on block paths
   - `apply_policy` decision reasoning (nuphy_connected, predicate match count)
   - Op-id per worker command and result

---

## Notes for Newman/Kramer/Elaine

- **Timing tolerance:** All tests specify ≤2s for state transitions. SetupAPI calls can block; if tests fail only on timing (e.g., 2.5s instead of 2s), note in results but don't consider blocking unless >5s.
- **Log verbosity:** Every `apply_policy` call should log its decision path. Predicate refusal (match_count != 1) must include full device enumeration dump.
- **Verify contract enforcement:** Launch, `--recover`, suspend/shutdown, Quit all MUST call `verify_state` after ENABLE. `Enabled` is the only success; `Disabled` or `Error` is logged loudly.
- **Test ordering:** Smoke tests 1–9 build on each other (app stays running). Adversarial tests 10–12 are independent (quit after each). Recovery drills are retrospective (confirm already-tested paths).
- **Surface hardware required:** BLE stack behavior, Modern Standby (PBT_APMRESUMEAUTOMATIC), and SAM-bus keyboard targeting are Surface-specific. Cannot be validated on x64 desktop or VM.

---

## New Tests (13+) — Feature Coverage Added Post-v0.1 Snapshot

### Test 13: Console Window Hidden on Launch (No Black Console)

**What it proves:** The `windows_subsystem = "windows"` manifest setting and AttachConsole logic work together to hide the console window on direct launch while preserving parent console attachment.

**Preconditions:**
- `output\kbblock.exe` exists and is built in release mode
- No `kbblock.exe` processes running
- File Explorer open
- PowerShell open

**Steps:**
1. Open File Explorer, navigate to `output\` directory
2. Double-click `kbblock.exe` (launch from Explorer, no console)
3. Observe desktop for 2 seconds
4. Open PowerShell window (separate console)
5. In PowerShell, run: `.\output\kbblock.exe` (launch from terminal with parent console)
6. Observe immediately if a console window appears
7. After 1 second, check if the existing PowerShell window shows any output
8. Kill both instances: `Get-Process kbblock | Stop-Process -Force`

**Expected:**
- **Double-click launch (step 2–3):** NO black console window appears. Only tray icon shows. Windows Mica/Acrylic background unchanged.
- **PowerShell launch (step 5–7):** NO new console window spawned. Command returns to PowerShell prompt immediately. Parent PowerShell window becomes the console for any AttachConsole output (if any).

**Pass criteria:**
- Double-click: zero console windows visible at any point
- PowerShell launch: process attaches to parent console (command line returns, no new window)
- Both: tray icon appears within 2 seconds

**Cleanup:** Kill any running `kbblock.exe` instances.

---

### Test 14: CLI Output Preserved (AttachConsole)

**What it proves:** When launched from PowerShell, the `--help` or similar CLI flag output appears in the SAME PowerShell window, demonstrating AttachConsole(ATTACH_PARENT_PROCESS) is working.

**Preconditions:**
- PowerShell open
- `output\kbblock.exe` accessible from current working directory or PATH
- No existing `kbblock.exe` running

**Steps:**
1. In PowerShell, run: `.\output\kbblock.exe --help` (or `.\output\kbblock.exe --version` if --help not defined)
2. Observe output for 2 seconds
3. Verify text appears in the SAME PowerShell window (not a new console)
4. Note the exit code: `$LASTEXITCODE` should be 0 or 1 (success or expected error)

**Expected:**
- CLI help/version text (if any) appears in the same PowerShell console
- Command returns to prompt
- No new console window spawns

**Pass criteria:**
- Text output visible in parent PowerShell window
- No black console window appeared
- Exit code is not a crash code (e.g., not 0xC0000374 / exception code)

**Cleanup:** None (process exited cleanly).

---

### Test 15: "Start at Login" Toggle — Install Autostart

**What it proves:** The autostart menu item correctly reads/writes the HKCU Run key and re-launches the app on next logon.

**Preconditions:**
- App running (Test 1 state recommended)
- No existing `kbblock` Run key entry (clean state)

**Steps:**
1. Right-click tray icon
2. Left-click "Start at login" (check it)
3. Wait 1 second
4. Open PowerShell as the same (non-admin) user
5. Run: `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v kbblock`
6. Note the value: should be `"C:\path\to\kbblock.exe"` (with quotes)
7. Verify the path matches the currently running `kbblock.exe` executable location
8. Sign out (Win+L or `logoff` in PowerShell) or reboot
9. Sign back in (or restart)
10. Verify tray icon re-appears automatically within 3 seconds of desktop ready

**Expected:**
- Step 5: Registry key exists with correct quoted path
- Step 8: Logoff/reboot succeeds
- Step 10: Tray icon appears without manual launch

**Pass criteria:**
- Registry entry created with exact exe path (quoted)
- App auto-launches after logoff/reboot
- Tray icon functional after auto-launch

**Cleanup:** Right-click tray → "Start at login" to uncheck. Verify registry key is deleted: `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v kbblock` should return "ERROR: The system was unable to find the specified registry key or value."

---

### Test 16: "Start at Login" Toggle — Uninstall Autostart

**What it proves:** Unchecking "Start at login" removes the HKCU Run key value cleanly.

**Preconditions:**
- App running
- "Start at login" currently checked (from Test 15 cleanup or prior state)

**Steps:**
1. Right-click tray icon
2. Left-click "Start at login" (uncheck it)
3. Wait 1 second
4. Open PowerShell
5. Run: `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v kbblock 2>&1`
6. Verify the value is gone (error message: "ERROR: The system was unable to find the specified registry key or value.")

**Expected:**
- Registry key deleted
- Menu item shows unchecked state

**Pass criteria:**
- Registry value removed
- No errors in subsequent startup attempts

**Cleanup:** Leave unchecked.

---

### Test 17: "Boot Recovery Task" Toggle — Install Task

**What it proves:** Checking "Boot recovery task" creates a Task Scheduler entry that runs `kbblock.exe --recover` at AT_STARTUP with SYSTEM principal and Highest run level.

**Preconditions:**
- App running (non-elevated tray session)
- "Boot recovery task" currently unchecked

**Steps:**
1. Right-click tray icon
2. Left-click "Boot recovery task" (check it)
3. UAC prompt appears → Click "Yes" (touch/mouse)
4. Wait 2 seconds for task registration
5. Open PowerShell as Administrator
6. Run: `schtasks /Query /TN kbblock-boot-recover /V /FO LIST`
7. Inspect the output and verify:
   - **Task Name:** `\kbblock-boot-recover`
   - **Trigger Type:** `At system startup`
   - **Status:** `Ready` or `Enabled`
   - **Principal:** `NT AUTHORITY\SYSTEM`
   - **Run Level:** `Highest`
   - **Task To Run:** Should contain `kbblock.exe` and `--recover` flag

**Expected:**
- UAC prompt appears and disappears after clicking Yes
- Task Scheduler query succeeds
- All metadata matches above spec

**Pass criteria:**
- Task installed with correct name, trigger, principal, and command
- Tray menu item checked

**Cleanup (for next test):** Leave checked for Test 18.

---

### Test 18: "Boot Recovery Task" Toggle — Uninstall Task

**What it proves:** Unchecking "Boot recovery task" removes the Task Scheduler entry cleanly.

**Preconditions:**
- App running
- "Boot recovery task" currently checked (from Test 17)

**Steps:**
1. Right-click tray icon
2. Left-click "Boot recovery task" (uncheck it)
3. UAC prompt appears → Click "Yes"
4. Wait 2 seconds
5. Open PowerShell as Administrator
6. Run: `schtasks /Query /TN kbblock-boot-recover /V /FO LIST 2>&1`
7. Verify the command fails with: "ERROR: The system cannot find the file specified."

**Expected:**
- UAC prompt appears once
- Task Scheduler query fails (task no longer exists)
- Menu item shows unchecked

**Pass criteria:**
- Task deleted
- Query returns error (task not found)

**Cleanup:** Leave unchecked.

---

### Test 19: CLI Install/Uninstall Boot Task (Non-Elevated)

**What it proves:** Running `kbblock.exe --install-boot-task` and `--uninstall-boot-task` from a non-elevated PowerShell self-elevates via UAC and registers/removes the task.

**Preconditions:**
- PowerShell open (non-admin)
- No boot task currently installed

**Steps:**
1. Run: `.\output\kbblock.exe --install-boot-task`
2. UAC prompt appears → Click "Yes"
3. Wait 2 seconds
4. Inspect console output: should show task details (path, trigger, principal, action)
5. Verify exit code: `$LASTEXITCODE` should be 0
6. Verify task: `schtasks /Query /TN kbblock-boot-recover`
7. Now run: `.\output\kbblock.exe --uninstall-boot-task`
8. UAC prompt appears → Click "Yes"
9. Verify exit code: `$LASTEXITCODE` should be 0
10. Verify task deleted: `schtasks /Query /TN kbblock-boot-recover 2>&1` should fail

**Expected:**
- Install output: Task name, trigger type (At startup), principal (SYSTEM), action (exe path + --recover)
- Uninstall output: "[OK] Task removed" or similar success message
- UAC prompts once per command (2 total)
- Exit codes: 0 for both
- Task verification succeeds after install, fails after uninstall

**Pass criteria:**
- Both commands self-elevate without errors
- Task installed and removed correctly
- Output captured and readable in console

**Cleanup:** None (task already removed).

---

### Test 20: CLI Install/Uninstall Boot Task (Already Elevated)

**What it proves:** Running `--install-boot-task` from an ALREADY-elevated PowerShell does NOT re-prompt UAC (reuses token).

**Preconditions:**
- PowerShell open as Administrator (pre-elevated)
- No boot task currently installed

**Steps:**
1. Right-click PowerShell → "Run as Administrator"
2. In elevated PowerShell, run: `.\output\kbblock.exe --install-boot-task`
3. Observe: NO UAC prompt should appear
4. Wait 1 second
5. Verify task: `schtasks /Query /TN kbblock-boot-recover`
6. Cleanup: `.\output\kbblock.exe --uninstall-boot-task` (also no UAC)
7. Verify task deleted

**Expected:**
- No UAC prompts in either command
- Output and task created/removed successfully
- Process runs in elevated token directly

**Pass criteria:**
- Zero UAC dialogs
- Task installed and removed
- Commands execute in <2 seconds (no elevation overhead)

**Cleanup:** Task removed in step 6.

---

### Test 21: Stale-Path Warning — Autostart Registry

**What it proves:** If the autostart Run key points to a different exe path than the currently running binary, a MessageBoxW warning appears on launch.

**Preconditions:**
- Original `kbblock.exe` built and tested (let's say at `C:\Users\Brady\Downloads\kbblock.exe`)
- "Start at login" enabled (Run key points to Downloads version)
- New `kbblock.exe` exists at a different location (e.g., `C:\Tools\kbblock.exe`)

**Steps:**
1. Edit HKCU Run key manually (via `reg add`) to point to `C:\Users\Brady\Downloads\kbblock.exe`, save the value
2. Run the NEW exe from `C:\Tools\kbblock.exe`
3. Observe: A MessageBoxW should appear warning about stale path
4. Read the warning message (should mention registry path mismatch)
5. Click OK to dismiss
6. Tray icon appears
7. Right-click tray → "Start at login" → uncheck and re-check
8. Verify registry now points to `C:\Tools\kbblock.exe` (updated)
9. Relaunch from `C:\Tools\kbblock.exe`
10. Verify NO warning appears (path now matches)

**Expected:**
- Step 3: MessageBoxW alert with stale-path warning
- Step 8: Registry updated to new path
- Step 10: No warning on re-launch

**Pass criteria:**
- Warning appears when exe location differs from registered path
- Warning disappears after re-toggling autostart
- Registry path updated correctly

**Cleanup:** Restore original exe path or delete the test copy.

---

### Test 22: Stale-Path Warning — Boot Recovery Task

**What it proves:** If the boot task's Action command points to a different exe path than the currently running binary, a MessageBoxW warning appears on launch.

**Preconditions:**
- Boot task installed from original exe location (e.g., `C:\Users\Brady\Downloads\kbblock.exe`)
- New exe exists at different location (e.g., `C:\Tools\kbblock.exe`)

**Steps:**
1. Install boot task from original location (Test 17 steps 1–7)
2. Verify task points to `C:\Users\Brady\Downloads\kbblock.exe`
3. Run new exe: `C:\Tools\kbblock.exe`
4. Observe: MessageBoxW should appear warning about stale task path
5. Click OK
6. Tray icon appears
7. Right-click tray → "Boot recovery task" → uncheck and re-check
8. UAC → Yes
9. Verify task XML now points to `C:\Tools\kbblock.exe` (updated)
10. Relaunch from `C:\Tools\kbblock.exe`
11. Verify NO warning appears (path now matches)

**Expected:**
- Step 4: MessageBoxW alert with stale-task warning
- Step 9: Task Action command updated to new path
- Step 11: No warning on re-launch

**Pass criteria:**
- Warning appears when task path differs from current exe
- Warning disappears after re-toggling boot task
- Task command updated correctly

**Cleanup:** Uninstall boot task (uncheck) or delete the test copy.

---

### Test 23: Boot Recovery at Lock Screen (No Autostart)

**What it proves:** With only "Boot recovery task" checked (autostart OFF), the boot-triggered SYSTEM task fires before Winlogon, re-enabling the internal keyboard BEFORE the user logs in.

**Preconditions:**
- "Boot recovery task" checked
- "Start at login" unchecked (autostart OFF)
- Nuphy connected and paired
- Internal keyboard confirmed working before test

**Steps:**
1. Launch app, verify internal keyboard disabled
2. Reboot Surface (Start → Power → Restart)
3. At lock screen (do NOT log in yet), attempt to type on internal keyboard
4. Verify characters appear
5. Log in
6. Verify tray icon is NOT present (autostart is OFF)
7. Open Event Viewer or check log file to confirm boot task ran

**Expected:**
- Step 3–4: Internal keyboard works at lock screen (task ran pre-logon)
- Step 5–6: After login, tray icon absent (autostart disabled)
- Step 7: Task execution logged in Event Viewer (Task Scheduler logs)

**Pass criteria:**
- Boot task fires before Winlogon (keyboard alive at lock screen)
- Autostart remains off (no tray icon on login)
- Internal keyboard enabled for unlock

**Cleanup:** None.

---

### Test 24: Combined Autostart + Boot Recovery (Full Stack)

**What it proves:** With both "Start at login" and "Boot recovery task" checked, the full recovery stack works: at lock screen (boot task), after login (autostart).

**Preconditions:**
- Both "Start at login" and "Boot recovery task" checked
- Nuphy connected
- Internal keyboard confirmed working

**Steps:**
1. Launch app, verify internal keyboard disabled
2. Force-close app (or trigger a crash): `Get-Process kbblock | Stop-Process -Force`
3. Reboot
4. At lock screen, attempt to type on internal keyboard
5. Verify characters appear (boot task did its job)
6. Log in
7. Verify tray icon appears automatically (autostart did its job)
8. Hover tray icon, read tooltip
9. Verify tray shows Active=true (rearm)

**Expected:**
- Step 4–5: Lock screen keyboard works (boot task re-enabled)
- Step 6–7: Tray icon auto-appears (autostart)
- Step 9: Tooltip shows Active state

**Pass criteria:**
- Boot task runs at lock screen (keyboard available for unlock)
- Autostart launches tray on login
- Both mechanisms coordinate (no conflicts)

**Cleanup:** None.

---

### Test 25: Boot Recovery After Forced Crash (Reboot Lockout Verification)

**What it proves:** A forced app crash followed by reboot, with boot recovery task installed, keeps the internal keyboard available at lock screen (verifies the critical Test 11 scenario on the consolidated build).

**Preconditions:**
- "Boot recovery task" checked
- Nuphy connected and active
- App running and internal keyboard disabled

**Steps:**
1. Verify internal keyboard is disabled (Nuphy on, Active on)
2. Force crash: `Get-Process kbblock | Stop-Process -Force`
3. Immediately attempt internal keyboard → should still be disabled (SetupAPI state persists)
4. Reboot Surface (Start → Power → Restart)
5. At lock screen (do NOT log in), attempt to type on internal keyboard
6. Verify characters appear within 3 seconds of desktop appearance
7. Log in (use touchpad + On-Screen Keyboard or USB keyboard if needed)
8. Launch app again (File Explorer double-click or Start menu)
9. Verify tray appears and internal keyboard is disabled again (normal operation resumed)

**Expected:**
- Step 2–3: Crash leaves keyboard disabled (SetupAPI state)
- Step 5–6: Boot task fires pre-logon, re-enables keyboard (characters appear)
- Step 8–9: Fresh launch re-disables (normal operation)

**Pass criteria:**
- Lock screen keyboard works after crash+reboot (boot task recovered)
- App re-launches cleanly and re-establishes normal disable state
- No permanent lockout

**Cleanup:** App now running; leave as-is for manual verification or quit.

---

## Notes for Tests 13–25

- **Windows subsystem:** All tests depend on `windows_subsystem = "windows"` being set in main.rs (line 1). No console window should appear on normal launch.
- **AttachConsole:** Tests 14 validates the AttachConsole(ATTACH_PARENT_PROCESS) call on main() entry (line 186–191 of main.rs).
- **Registry/Task Scheduler:** Tests 15–22 exercise autostart.rs and boot_task.rs modules. Stale-path detection uses refresh_stale_indicators() called at startup (line 422).
- **Crash recovery:** Tests 23–25 exercise the boot-task SYSTEM principal firing before Winlogon (cold-boot recovery layer).
- **Elevated vs. non-elevated:** Test 20 validates is_elevated() and relaunch_elevated() in the admin subcommand path (lines 1558–1623).
- **Single-instance mutex:** All tests should fail gracefully if a second instance tries to launch (Test 15–22 may have background instances; use `Get-Process kbblock -ErrorAction SilentlyContinue | Stop-Process -Force` to clean before tests).

---

**End of Test Recipe. All tests (1–25) define the acceptance gate. Code must satisfy these criteria.**
