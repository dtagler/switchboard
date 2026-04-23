# v0.1 Owner Execution Checklist — Surface Laptop 7 Physical Test

**Hardware:** Surface Laptop 7 15", Snapdragon X Elite, Windows 11 ARM64  
**External Keyboard:** Nuphy Air75 V3 (BLE, BD_ADDR `CC:00:62:19:C5:FD`)  
**Status:** Ready for physical smoke test  
**Date:** 2026-04-21

---

## ⚠️ EMERGENCY RECOVERY (Read This First)

**If the internal keyboard is dead and NOTHING ELSE WORKS:**

1. **Primary:** From File Explorer or Win+R, run:
   ```
   .\output\kbblock.exe --recover
   ```
   - UAC prompt → click "Yes" (touchpad + mouse only)
   - Internal keyboard should work within 2 seconds
   - Exit code 0 = success, exit code 1 = failed (see fallback #2)

2. **Fallback:** Reboot (Start → Power → Restart)
   - On next launch of `kbblock.exe`, cold-start ENABLE will recover
   - At lock screen, use touchpad + Accessibility icon → On-Screen Keyboard if needed

3. **Hard fallback:** Device Manager (Win+X → Device Manager)
   - Find: `Human Interface Devices` → internal keyboard device
   - Right-click → **Enable device**

4. **Ultimate:** Boot to lock screen → plug USB keyboard → type password → sign in → run step 1

---

## 🔍 Log Capture Protocol

### Log File Location

- **Path:** `%LOCALAPPDATA%\kbblock\kbblock.log`
- **Full Windows path:** `C:\Users\davidtagler\AppData\Local\kbblock\kbblock.log`

### How to Monitor Logs

Open a PowerShell window and keep it running through all tests:

```powershell
Get-Content "$env:LOCALAPPDATA\kbblock\kbblock.log" -Wait -Tail 50
```

This will show the last 50 lines and auto-update as new log entries appear.

### Per-Test Log Capture

**Before each test:**
```powershell
Remove-Item "$env:LOCALAPPDATA\kbblock\kbblock.log" -Force -ErrorAction SilentlyContinue
```

**After each test (PASS or FAIL):**
```powershell
Copy-Item "$env:LOCALAPPDATA\kbblock\kbblock.log" "C:\temp\test<N>-<pass|fail>.log"
```

Replace `<N>` with test number (1–12) and `<pass|fail>` with actual result.

### What to Look For in Logs

Each test defines expected log patterns. Key sequences:

- **Cold-start ENABLE:** `main() → ENABLE → verify: Enabled`
- **Block path:** `apply_policy → DISABLE → verify: Disabled`
- **Unblock path:** `apply_policy → ENABLE → verify: Enabled`
- **Op-id correlation:** Each command shows `op_id=X`, results show matching `op_id=X`
- **Verify mismatch (Test 10):** `DISABLE → verify: Enabled (mismatch) → ENABLE → desired_active=false → balloon`

**On FAIL:** Save the log excerpt covering the test window + 10 seconds before/after. Note timestamps.

---

## 🔨 Build & First-Run Preflight

### Step 1: Build the Executable

From PowerShell (run from project root):

```powershell
cd C:\Users\davidtagler\Code\bluetooth-keyboard-app
.\scripts\build.ps1
```

**Expected output:**
- Docker build completes without errors
- Final line: `Built: C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\kbblock.exe`

**Verify:**
```powershell
Get-Item .\output\kbblock.exe | Select-Object Name, Length
```

- **Name:** `kbblock.exe`
- **Length:** Approximately 100–150 KB (ARM64 Windows PE)

### Step 2: Pre-Test Cold State

**Setup:**
1. Nuphy Air75 powered **OFF**
2. Open Notepad, click in text area
3. Type three characters on **internal keyboard**
4. **Expected:** Characters appear (baseline: internal keyboard works)

**If internal keyboard doesn't work NOW:**
- STOP. The internal keyboard is already broken before testing begins.
- Reboot, verify, or troubleshoot hardware before proceeding.

### Step 3: First Launch (Smoke Test)

1. Open File Explorer → `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\`
2. Double-click `kbblock.exe`
3. **UAC prompt:** "Do you want to allow this app to make changes?" → Click **Yes** (touchpad/mouse)
4. Within 2 seconds: **Tray icon should appear** in system tray (lower-right corner)
5. Hover tray icon → **Tooltip should show:** `kbblock | Active: On | Nuphy: Disconnected`
6. Open Notepad, type on internal keyboard → **Expected:** Characters appear (Nuphy off = internal works)

**PASS criteria:**
- UAC prompt appeared and was accepted
- Tray icon appeared within 2 seconds
- No immediate crash (tray icon stays)
- Internal keyboard works (Nuphy off)

**FAIL observations:**
- No UAC prompt → manifest not embedded (build issue)
- UAC accepted but no tray icon → app crashed during init (check log)
- Tray icon appears but vanishes → app crashed after tray setup (check log)

**On FAIL:** Save log to `preflight-fail.log`, review, fix build before continuing.

**On PASS:** Right-click tray icon → **Quit**. App closes cleanly. Proceed to Test 1.

---

## ✅ Test 1: Launch with Nuphy Connected

**Purpose:** Verify cold-start ENABLE + BLE subscribe + apply_policy DISABLE sequence.

### Setup

1. **Nuphy:** Powered ON and connected (verify in Windows Settings → Bluetooth & devices → Nuphy shows "Connected")
2. **Internal keyboard baseline:** Open Notepad, type three characters on internal keyboard → characters appear
3. **Log:** Delete `%LOCALAPPDATA%\kbblock\kbblock.log` (if exists)
4. **Safety net:** Plug/unplug USB keyboard, verify it types (hard fallback confirmed)

### Action

1. Double-click `kbblock.exe` from File Explorer
2. UAC prompt → Click **Yes** (touchpad/mouse)
3. Wait 2 seconds for tray icon to appear
4. Open Notepad (or keep existing window), click in text area
5. Attempt to type on **internal keyboard** (e.g., type "test" or "abc")

### Expected Observable Outcome

**Within 2 seconds of tray icon appearing:**
- **Internal keyboard produces NO characters** (keystrokes are blocked)
- **Nuphy keyboard works** (type on Nuphy, characters appear)

**Tray tooltip (hover icon):**
- Shows `Active: On | Nuphy: Connected`

**Log shows (in order):**
1. `main() → ENABLE` (cold-start unconditional ENABLE)
2. `verify: Enabled` (internal keyboard confirmed working)
3. `BLE subscribe → ConnectionStatusChanged` (Nuphy watcher active)
4. `apply_policy → nuphy_connected=true, predicate match_count=1 → DISABLE`
5. `verify: Disabled` (internal keyboard confirmed blocked)

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Characters appear from internal keyboard after 2s | DISABLE failed OR verify mismatch | **CRITICAL** |
| Tray icon never appears | App crashed during init | **CRITICAL** |
| UAC prompt never appears | Manifest not embedded | **BUILD ISSUE** |
| Log shows `verify: Enabled (mismatch)` after DISABLE | SetupAPI succeeded but device still enabled (Test 10 scenario) | **SEE TEST 10** |

### Recovery on FAIL

1. If tray icon visible: Right-click → **Quit**
2. Run emergency recovery: `.\output\kbblock.exe --recover` (Win+R or Explorer address bar)
3. Verify internal keyboard works after recovery
4. Save log to `C:\temp\test1-fail.log`
5. Review log for error messages, op_id sequence, verify result

### On PASS

1. Leave app running (do NOT quit)
2. Save log to `C:\temp\test1-pass.log`
3. Internal keyboard should remain disabled for Test 2

---

## ✅ Test 2: Nuphy Disconnect

**Purpose:** Verify BLE `ConnectionStatusChanged` → apply_policy ENABLE when Nuphy disconnects.

### Setup

- Test 1 passed
- App running, tray icon visible
- Internal keyboard disabled (Nuphy connected)

### Action

1. Power off Nuphy (hold power button ~3 seconds until LED turns off)
2. Wait 3 seconds (BLE disconnect latency)
3. Open Notepad (or keep existing), click in text area
4. Attempt to type on **internal keyboard**

### Expected Observable Outcome

**Within 2–3 seconds of Nuphy power-off:**
- **Internal keyboard produces characters** (unblocked)

**Tray tooltip:**
- Updates to `Active: On | Nuphy: Disconnected`

**Log shows:**
1. `ConnectionStatusChanged → apply_policy`
2. `nuphy_connected=false → ENABLE`
3. (Internal keyboard now works)

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard still dead after 5 seconds | BLE event missed OR ENABLE failed | **CRITICAL** |
| After 25 seconds, still dead | Sanity timer didn't rescue (see Test 8) | **CRITICAL** |
| App crashed (tray icon vanished) | BLE event handler fault | **CRITICAL** |

### Recovery on FAIL

1. Right-click tray → **Quit** (if tray still visible)
2. Run `.\output\kbblock.exe --recover`
3. Verify internal keyboard works
4. Save log to `C:\temp\test2-fail.log`

### On PASS

1. Leave app running, Nuphy powered **OFF**
2. Save log to `C:\temp\test2-pass.log`
3. Internal keyboard should remain enabled for Test 3

---

## ✅ Test 3: Nuphy Reconnect

**Purpose:** Verify BLE reconnect → apply_policy DISABLE.

### Setup

- Test 2 passed
- App running
- Nuphy powered **OFF**
- Internal keyboard working

### Action

1. Power on Nuphy (press power button → LED blinks → LED goes solid)
2. Wait 3 seconds for BLE reconnect (watch for LED solid = connected)
3. Attempt to type on **internal keyboard**

### Expected Observable Outcome

**Within 2–3 seconds of Nuphy LED solid:**
- **Internal keyboard produces NO characters** (blocked again)

**Tray tooltip:**
- Updates to `Active: On | Nuphy: Connected`

**Log shows:**
1. `ConnectionStatusChanged → apply_policy`
2. `nuphy_connected=true, predicate match_count=1 → DISABLE`
3. `verify: Disabled`

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard still works after 5s | BLE event missed OR disable failed | **HIGH** |
| After 25 seconds, still works | Sanity timer didn't correct | **HIGH** |

### Recovery on FAIL

1. Quit app, run `--recover`
2. Save log to `C:\temp\test3-fail.log`

### On PASS

1. Leave app running, Nuphy **ON**
2. Save log to `C:\temp\test3-pass.log`
3. Internal keyboard should remain disabled for Test 4

---

## ✅ Test 4: Tray Toggle — Disable via Active Uncheck

**Purpose:** Verify manual user control: unchecking Active → ENABLE.

### Setup

- Test 3 passed
- App running
- Nuphy **ON**, connected
- Internal keyboard disabled

### Action

1. Right-click tray icon
2. Click **"Active"** to **uncheck** it (menu item should have checkmark before click, no checkmark after)
3. Wait 2 seconds
4. Attempt to type on **internal keyboard**

### Expected Observable Outcome

**Within 2 seconds:**
- **Internal keyboard produces characters** (unblocked)

**Tray tooltip:**
- Shows `Active: Off | Nuphy: Connected`

**Log shows:**
1. `desired_active=false`
2. `apply_policy → ENABLE`
3. `verify: Enabled`

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard stays dead | ENABLE didn't fire OR verify mismatch | **CRITICAL** |
| Menu item doesn't toggle (checkmark stuck) | Tray message pump broken | **HIGH** |

### Recovery on FAIL

Run `--recover`, save log to `C:\temp\test4-fail.log`.

### On PASS

1. Leave app running, Active **OFF**
2. Save log to `C:\temp\test4-pass.log`
3. Internal keyboard should remain enabled for Test 5

---

## ✅ Test 5: Tray Toggle — Enable via Active Check

**Purpose:** Verify re-checking Active → DISABLE.

### Setup

- Test 4 passed
- App running
- Nuphy **ON**, connected
- Active **OFF**
- Internal keyboard working

### Action

1. Right-click tray icon
2. Click **"Active"** to **check** it (menu item should have NO checkmark before click, checkmark after)
3. Wait 2 seconds
4. Attempt to type on **internal keyboard**

### Expected Observable Outcome

**Within 2 seconds:**
- **Internal keyboard produces NO characters** (blocked)

**Tray tooltip:**
- Shows `Active: On | Nuphy: Connected`

**Log shows:**
1. `desired_active=true`
2. `apply_policy → DISABLE`
3. `verify: Disabled`

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard still works | DISABLE failed OR predicate resolved to 0/multiple | **CRITICAL** |

### Recovery on FAIL

Quit app, run `--recover`, save log to `C:\temp\test5-fail.log`.

### On PASS

1. Leave app running, Active **ON**, Nuphy **ON**
2. Save log to `C:\temp\test5-pass.log`
3. Internal keyboard should remain disabled for Test 6

---

## ✅ Test 6: Lid Close (Modern Standby Suspend) — Lock Screen

**Purpose:** Verify `PBT_APMSUSPEND` → ENABLE at lock screen (prevents lockout).

### Setup

- Test 5 passed
- App running, Active **ON**, Nuphy **ON**
- Internal keyboard disabled

### Action

1. **Close Surface lid**
2. Wait 30 seconds (device enters Modern Standby / S0ix sleep)
3. **Open lid**
4. Lock screen appears
5. **DO NOT UNLOCK YET**
6. Use touchpad to verify **Accessibility icon** (lower-right corner) is clickable (you should see cursor change on hover)
7. Attempt to type on **internal keyboard** at lock screen (e.g., as if entering password)

### Expected Observable Outcome (Lock Screen)

**At lock screen, BEFORE unlocking:**
- **Internal keyboard produces characters** (unblocked at lock screen)
- Accessibility icon is visible and clickable (recovery path confirmed)

**This is CRITICAL:** If internal keyboard is dead at lock screen, user is locked out.

### FAIL Criteria (Lock Screen)

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard DEAD at lock screen | Suspend ENABLE failed → **USER LOCKED OUT** | **CRITICAL LOCKOUT** |
| Accessibility icon not present or not clickable | Windows config issue (not app bug) | **ENVIRONMENT** |

### Recovery on FAIL (at Lock Screen)

**If internal keyboard is dead at lock screen:**

1. Touchpad → click **Accessibility icon** (lower-right) → **On-Screen Keyboard**
2. OSK appears → use touchpad to click keys → type password → sign in
3. After sign-in: File Explorer → `.\output\kbblock.exe --recover`
4. Verify internal keyboard works
5. Save log to `C:\temp\test6-fail-lockscreen.log`

**This is the #1 catastrophic failure mode. If this fails, Test 6 BLOCKS v0.1 release.**

### On PASS (Lock Screen)

**Internal keyboard works at lock screen → proceed to unlock:**

1. Type password on internal keyboard (should work)
2. Sign in to desktop
3. Wait 2 seconds
4. Attempt to type on internal keyboard (should be **DISABLED** again)
5. Verify tray tooltip: `Active: On | Nuphy: Connected` (if Nuphy auto-reconnected)

**Log shows (after unlock):**
1. `PBT_APMSUSPEND → ENABLE → verify: Enabled` (happened during suspend)
2. `PBT_APMRESUMEAUTOMATIC → resume_pending=true` (lid opened, before lock screen)
3. `WTS_SESSION_UNLOCK → resume_pending cleared → apply_policy → DISABLE → verify: Disabled` (after sign-in)

**Expected on desktop:**
- Internal keyboard **DISABLED** again (Nuphy connected, Active on)

**If internal keyboard WORKS on desktop (should be disabled):**
- **FAIL:** Resume gating didn't work OR `apply_policy` wasn't called on unlock
- Save log to `C:\temp\test6-fail-desktop.log`
- This is Test 7 failure (see below)

### On Full PASS (Lock Screen + Desktop)

1. Save log to `C:\temp\test6-pass.log`
2. Right-click tray → **Quit** (clean up for adversarial tests)

---

## ✅ Test 7: Lid Close (Post-Unlock Re-Disable)

**Purpose:** Verify `WTS_SESSION_UNLOCK` clears `resume_pending` and re-applies policy.

### Setup

- Test 6 passed **at lock screen** (internal keyboard worked)
- User unlocked and is now on desktop
- Nuphy still connected (verify in Bluetooth settings if unsure)

### Action

1. Verify Nuphy connection status: Windows Settings → Bluetooth & devices → Nuphy should show "Connected"
2. Open Notepad, click in text area
3. Attempt to type on **internal keyboard**

### Expected Observable Outcome

- **Internal keyboard produces NO characters** (re-disabled after unlock)

**Tray tooltip:**
- `Active: On | Nuphy: Connected`

**Log shows:**
1. `WTS_SESSION_UNLOCK → resume_pending cleared`
2. `apply_policy → DISABLE → verify: Disabled`

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard works on desktop | Resume gating failed OR `apply_policy` not called on unlock | **HIGH** |

### Recovery on FAIL

Quit app, run `--recover`, save log to `C:\temp\test7-fail.log`.

### On PASS

1. Save log to `C:\temp\test7-pass.log`
2. Right-click tray → **Quit**
3. Verify internal keyboard works after Quit (Quit ENABLE)
4. Tests 6 and 7 are now complete as a pair (suspend → lock → unlock → desktop)

---

## ✅ Test 8: Sanity Timer (Passive Drift Correction)

**Purpose:** Verify 20-second sanity timer calls `apply_policy` even when no events fire.

### Setup

- Fresh launch
- Nuphy **ON**, connected
- Active **ON**
- Internal keyboard disabled

### Action

1. Launch `kbblock.exe` normally (fresh launch, not from Test 7)
2. Wait for internal keyboard to disable (within 2s)
3. **Power off Nuphy** (simulate missed BLE event)
4. **Do NOT interact with tray or keyboard**
5. **Wait 25 seconds** (sanity timer fires at 20s intervals + 5s margin)

### Expected Observable Outcome

**Within 25 seconds:**
- **Internal keyboard starts working** (sanity timer rescued)

**Log shows:**
1. `sanity_timer_tick → apply_policy`
2. `nuphy_connected=false → ENABLE`
3. `verify: Enabled`

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| After 25 seconds, internal still dead | Sanity timer didn't fire OR `apply_policy` didn't ENABLE | **HIGH** |

### Recovery on FAIL

Quit, `--recover`, save log to `C:\temp\test8-fail.log`.

### On PASS

1. Save log to `C:\temp\test8-pass.log`
2. Quit app

---

## ✅ Test 9: Quit While Disabled

**Purpose:** Verify Quit handler calls ENABLE before exiting (critical lockout prevention).

### Setup

- Fresh launch
- Nuphy **ON**, connected
- Active **ON**
- Internal keyboard disabled

### Action

1. Launch `kbblock.exe` normally
2. Verify internal keyboard is disabled (Nuphy on, Active on)
3. Right-click tray icon → **Quit**
4. **Immediately** (within 1 second) attempt to type on internal keyboard
5. Observe tray icon disappearance timing

### Expected Observable Outcome

**Critical timing:**
- **Internal keyboard produces characters BEFORE tray icon disappears**
- Tray icon disappears within 1–2 seconds of clicking Quit

**Log shows:**
1. `Quit → Shutdown sent`
2. `worker join (success or timeout 500ms)`
3. `inline ENABLE` (fallback after join)
4. `verify: Enabled`
5. `exit 0`

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Internal keyboard stays dead after tray icon vanishes | Quit ENABLE failed → **USER LOCKED OUT** | **CRITICAL LOCKOUT** |
| Process hangs (tray icon stays forever) | Message loop blocked | **CRITICAL** |

### Recovery on FAIL

1. Reboot
2. At lock screen: Use touchpad + Accessibility → OSK OR plug USB keyboard
3. Sign in, launch `kbblock.exe` (cold-start ENABLE will recover)
4. Save log to `C:\temp\test9-fail.log`

**This is the second catastrophic failure mode. If this fails, Test 9 BLOCKS v0.1 release.**

### On PASS

1. Save log to `C:\temp\test9-pass.log`
2. App cleanly exited, internal keyboard works

---

# 🔥 ADVERSARIAL TESTS (§9.1)

These tests exercise worst-case scenarios with explicit recovery validation.

---

## ✅ Test 10: Verify Mismatch (Top Priority per PLAN.md v5.4)

**Purpose:** Exercise the verify-failure recovery path where `SetupAPI` reports success but `verify_state` finds keyboard still enabled. This is the "disable failed silently" scenario. v5.4 UX: on mismatch, ENABLE + flip Active off + show balloon.

### Setup

- App running
- Nuphy **ON**, connected
- Active **ON** (or fresh launch)

### Action (Attempt to Force Race Condition)

1. Right-click tray → ensure **Active is checked**
2. **Immediately** toggle Active off and on rapidly **3 times within 2 seconds**
   - Right-click → Active (uncheck)
   - Right-click → Active (check)
   - Right-click → Active (uncheck)
   - Right-click → Active (check)
   - Right-click → Active (uncheck)
   - Right-click → Active (check)
3. Watch for **tray balloon notification** (bottom-right, near system tray)
4. Hover tray icon, read **tooltip**

### Expected Behavior (If Verify Mismatch Triggers)

**Tray balloon appears:**
- Text: `"Keyboard disable failed — toggled Active off. Restart app to retry."`

**Tooltip shows:**
- `Active: Off | Nuphy: Connected` (Active was flipped off automatically)

**Internal keyboard behavior:**
- **Produces characters** (ENABLE was called as recovery)

**Log shows:**
1. `apply_policy → DISABLE`
2. `verify: Enabled (mismatch)` (device still enabled after DISABLE)
3. `ENABLE` (recovery)
4. `desired_active=false` (flip Active off to prevent retry loop)
5. `balloon: Keyboard disable failed...` (user notification)

**Recovery validation:**
- No silent state poison (app doesn't keep retrying disable)
- User can toggle Active back on when ready (manual retry)

### Alternate Outcome (If Mismatch Doesn't Trigger)

**If rapid toggling doesn't trigger verify mismatch in 3 attempts:**

- Log still shows correct `DISABLE → verify: Disabled` sequences
- Internal keyboard behavior matches Active state (on = blocked, off = works)
- **This means the race wasn't fast enough; predicate + verify contract held**
- Mark test as **PASS** (contract is robust)

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Verify mismatch occurs but NO balloon | UX contract broken (silent failure) | **HIGH** |
| Verify mismatch, Active stays ON, app keeps retrying | State poison (retry loop) | **HIGH** |
| Verify mismatch, internal keyboard stays disabled | ENABLE didn't fire → **lockout risk** | **CRITICAL** |

### Recovery on FAIL

1. Quit app (if tray responsive)
2. Run `.\output\kbblock.exe --recover`
3. Verify internal keyboard works
4. Save log to `C:\temp\test10-fail.log`

### On PASS

1. Save log to `C:\temp\test10-pass.log`
2. Quit app

---

## ✅ Test 11: Forced Crash Mid-Disable (Critical Lockout Test)

**Purpose:** Simulate app crash while internal keyboard is disabled. Verify cold-boot ENABLE recovery via §12 recovery procedure row 3.

### Setup

- Nuphy **ON**, connected
- Fresh app launch
- Internal keyboard disabled

### Action — Part A: Forced Crash

1. Launch `kbblock.exe` normally
2. Verify internal keyboard disabled (Nuphy on, Active on)
3. Open **PowerShell as Administrator** (Win+X → Windows PowerShell (Admin))
4. Capture PID:
   ```powershell
   $pid = Get-Process kbblock | Select-Object -ExpandProperty Id
   Write-Host "kbblock PID: $pid"
   ```
5. Kill process:
   ```powershell
   Stop-Process -Id $pid -Force
   ```
6. Verify tray icon vanishes **immediately** (within 1 second)
7. Attempt to type on internal keyboard → **Expected: STILL DEAD** (crash left keyboard disabled)

### Action — Part B: Reboot + Lock Screen Recovery

8. **Reboot Surface** (Start → Power → Restart)
9. Wait for lock screen to appear
10. **DO NOT UNLOCK YET**
11. Attempt to type on **internal keyboard** at lock screen

### Expected Observable Outcome (Part B: Lock Screen)

**At lock screen, BEFORE unlocking:**
- **Internal keyboard is DEAD** (disabled state persisted via `CONFIGFLAG_DISABLED`)
- This confirms the crash scenario is realistic

**Recovery procedure validation (§12 row 4):**

1. **Touchpad → lower-right corner → Accessibility icon → On-Screen Keyboard**
2. OSK appears → click keys with touchpad
3. Type password using OSK → sign in

### Action — Part C: Post-Unlock Launch

4. After sign-in to desktop, open File Explorer
5. Navigate to `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\`
6. Double-click `kbblock.exe`
7. UAC prompt → Click **Yes** (touchpad/mouse)
8. Wait 2 seconds for tray icon to appear
9. Attempt to type on **internal keyboard**

### Expected Observable Outcome (Part C: Post-Launch)

**Within 2 seconds of tray icon appearing:**
- **Internal keyboard produces characters** (cold-start ENABLE recovered)

**Log shows:**
1. `main() → ENABLE` (cold-start unconditional ENABLE)
2. `verify: Enabled` (internal keyboard restored)
3. (App continues normal init)

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| At lock screen, internal keyboard WORKS (unexpected) | SetupAPI disable didn't persist (surprising but safe) | **NOTE** |
| After launching app post-unlock, internal still dead | Cold-start ENABLE failed → **USER LOCKED OUT** | **CRITICAL LOCKOUT** |
| OSK via Accessibility icon didn't appear | Windows config OR user error (not app bug) | **ENVIRONMENT** |

### Recovery on FAIL (Post-Unlock, Internal Still Dead)

1. Run `.\output\kbblock.exe --recover` from Win+R (using OSK if keyboard dead)
2. Verify internal keyboard works after `--recover`
3. Save log to `C:\temp\test11-fail.log`

**This is the third catastrophic failure mode (crash state persistence). If this fails, Test 11 BLOCKS v0.1 release.**

### On PASS

1. Save log to `C:\temp\test11-pass.log`
2. App running normally, internal keyboard works
3. This validates: crash state persistence + OSK recovery + cold-start ENABLE

---

## ✅ Test 12: Hung Instance + `--recover` Escape Hatch

**Purpose:** Verify `--recover` bypasses mutex and ENABLEs inline even while first instance holds mutex.

### Setup

- No `kbblock.exe` running
- Nuphy **ON**, connected

### Action

1. Launch `kbblock.exe` normally (File Explorer double-click)
2. UAC → Yes → tray icon appears
3. Verify internal keyboard disabled (Nuphy on)
4. **Open another File Explorer window** (Win+E)
5. Navigate to `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\`
6. In Explorer address bar, type:
   ```
   kbblock.exe --recover
   ```
7. Press **Enter**
8. UAC prompt → Click **Yes**
9. Wait 2 seconds

### Expected Observable Outcome

**Within 2 seconds:**
- **No error message** about mutex or "another instance is running"
- **Internal keyboard produces characters** (ENABLE succeeded)
- **First instance tray icon still present** (first instance keeps running, not disrupted)

**Log from `--recover` invocation shows:**
1. `--recover mode → skip mutex`
2. `inline ENABLE` (no worker thread)
3. `verify: Enabled`
4. `exit 0` (success)

**First instance log shows:**
- No disruption (continues running normally)
- No unexpected events or errors

### FAIL Criteria

| Observation | Meaning | Severity |
|---|---|---|
| Error about mutex or single instance conflict | `--recover` tried to acquire mutex | **HIGH** |
| Internal keyboard stays disabled | Inline ENABLE failed | **CRITICAL** |
| First instance crashed | Mutex not properly skipped | **HIGH** |

### Recovery on FAIL

1. Kill both processes (Task Manager or PowerShell `Stop-Process`)
2. Reboot if internal keyboard still dead
3. Save logs from both instances to `C:\temp\test12-fail-instance1.log` and `C:\temp\test12-fail-recover.log`

### On PASS

1. Save logs to `C:\temp\test12-pass-instance1.log` and `C:\temp\test12-pass-recover.log`
2. Right-click first instance tray → **Quit**
3. Verify internal keyboard works after Quit (Quit ENABLE)

---

# 📋 Recovery Drill Matrix (§12 Validation)

These drills confirm each row of §12 recovery procedure was tested end-to-end.

| Drill | §12 Row | Scenario | Test Coverage | Status |
|---|---|---|---|---|
| **1** | Row 1 | Tray uncheck Active | Test 4 | ✅ After Test 4 |
| **2** | Row 2 | `--recover` while hung | Test 12 | ✅ After Test 12 |
| **3** | Row 3 | Double-click launch after crash | Test 11 (Part C) | ✅ After Test 11 |
| **4** | Row 4 | Lock-screen OSK | Test 6 + Test 11 (Part B) | ✅ After Tests 6 & 11 |
| **5** | Row 5 | USB keyboard hard fallback | Safety net (all tests) | ✅ Plug test before each test |
| **6** | Row 6 | Hard power-off | Manual rehearsal | ⚠️ Not covered by tests |

**Drill 6 (Hard Power-Off) — Manual Rehearsal:**

1. With internal keyboard disabled, hold **power button for 10 seconds** (hard power-off)
2. Cold boot → lock screen appears
3. Proceed with row 4 (OSK) or row 5 (USB keyboard)
4. Sign in, launch `kbblock.exe`
5. Verify internal keyboard works (cold-start ENABLE)

This drill is NOT part of the automated test suite but should be manually rehearsed once.

---

# 🎯 Exit Criteria — v0.1 Acceptance Gate

**All of the following must be TRUE before v0.1 release:**

## Core Tests

- [ ] **Test 1:** Cold-start + BLE + disable ✅
- [ ] **Test 2:** Nuphy disconnect → internal works ✅
- [ ] **Test 3:** Nuphy reconnect → internal disabled ✅
- [ ] **Test 4:** Tray toggle Active off → internal works ✅
- [ ] **Test 5:** Tray toggle Active on → internal disabled ✅
- [ ] **Test 6:** Lid close → lock screen → internal works ✅ **(CRITICAL)**
- [ ] **Test 7:** Post-unlock → internal disabled ✅
- [ ] **Test 8:** Sanity timer → internal works ✅
- [ ] **Test 9:** Quit → internal works before exit ✅ **(CRITICAL)**

## Adversarial Tests

- [ ] **Test 10:** Verify mismatch → balloon + Active off + ENABLE ✅
- [ ] **Test 11:** Crash + reboot → OSK → launch → internal works ✅ **(CRITICAL)**
- [ ] **Test 12:** `--recover` while hung → internal works ✅

## Log Verification

- [ ] All logs show expected ENABLE/DISABLE/verify sequences
- [ ] Op-id correlation works (no stale results accepted)
- [ ] No unexpected tray balloon warnings (except Test 10 if verify mismatch triggered)
- [ ] Predicate never resolved to 0 or >1 matches (all tests show `match_count=1`)

## Recovery Validation

- [ ] Lock-screen OSK path verified in Tests 6 and 11 ✅
- [ ] USB keyboard fallback verified in every safety net check ✅
- [ ] Recovery drills 1–5 documented and confirmed ✅

## Blocking Failures

**These MUST pass or v0.1 cannot ship:**

1. **Test 6 lock screen FAIL** — User locked out after suspend
2. **Test 9 Quit FAIL** — User locked out after quit
3. **Test 11 post-unlock FAIL** — Cold-start ENABLE doesn't recover after crash

**Non-blocking warnings (document in known issues):**

- Test 8 sanity timer takes >25s but <60s (BLE latency tolerance)
- Test 10 verify mismatch never triggers (predicate is robust)

---

# 📦 Test Results Summary Template

After completing all 12 tests, fill out this summary:

```
v0.1 Test Results — Surface Laptop 7 (Snapdragon X Elite)
Date: YYYY-MM-DD
Tester: [Your Name]

SMOKE TESTS (1–9):
✅ Test 1: Launch with Nuphy Connected
✅ Test 2: Nuphy Disconnect
✅ Test 3: Nuphy Reconnect
✅ Test 4: Tray Toggle Off
✅ Test 5: Tray Toggle On
✅ Test 6: Lid Close (Lock Screen)
✅ Test 7: Lid Close (Post-Unlock)
✅ Test 8: Sanity Timer
✅ Test 9: Quit While Disabled

ADVERSARIAL TESTS (10–12):
✅ Test 10: Verify Mismatch
✅ Test 11: Forced Crash + Reboot
✅ Test 12: Hung Instance + --recover

CRITICAL LOCKOUT TESTS:
✅ Test 6: Lock screen internal keyboard works
✅ Test 9: Quit enables internal keyboard
✅ Test 11: Cold-start ENABLE recovers after crash

BLOCKING FAILURES: [None / List failures]

KNOWN ISSUES: [None / List non-blocking issues]

LOGS ATTACHED:
- test1-pass.log through test12-pass.log
- (test<N>-fail.log for any failures)

RECOMMENDATION: [PASS — ship v0.1 / FAIL — address blocking issues before ship]
```

---

**End of Owner Execution Checklist. Print this document for offline execution. Good luck testing! 🚀**
