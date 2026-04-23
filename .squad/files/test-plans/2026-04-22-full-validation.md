# SwitchBoard — Full Post-Session Validation Test Plan

**Author:** George (QA / Safety Tester)  
**Tester:** Brady Gaster  
**Hardware:** Surface Laptop 7 15", Snapdragon X Elite, ARM64, Windows 11  
**Build under test:** `target/aarch64-pc-windows-msvc/release/switchboard.exe` (454 KB)  
**Session context:** Major refactor — kbblock → switchboard rename + runtime theme detection + embedded ICO resources  
**Date:** 2026-04-22

---

## ⚠️ SHOWSTOPPERS — Rollback Criteria

If ANY of the following tests fail, **STOP IMMEDIATELY** and **ROLLBACK** to the previous known-good build. Do not ship, do not autostart, do not "we'll fix it next iteration."

- **Test 7: Escape fail-safe** — If Escape-hold doesn't restore the internal keyboard after 10 seconds, the safety contract is BROKEN.
- **Test 8: Reboot with BT keyboard connected** — If the internal keyboard remains enabled after reboot when Nuphy is connected, the original hook bug is NOT fixed.
- **Test 11: Clean shutdown** — If Exit doesn't release the internal keyboard, users get locked out on next boot.

All other failures are bugs to fix but not rollback-worthy.

---

## Executive Summary

This test plan covers the FULL validation Brady needs to perform on his Surface Laptop 7 after this session's changes:

| Category | Test Count | Estimated Time | Critical? |
|---|---|---|---|
| Pre-flight (migration) | 5 steps | 5 min | Yes (if kbblock was installed) |
| Smoke tests | 3 tests | 3 min | Yes |
| Core BT keyboard hooks | 4 tests | 12 min | **YES (SHOWSTOPPERS)** |
| Fail-safe | 1 test | 2 min | **YES (SHOWSTOPPER)** |
| Theme swap | 7 tests (ref) | 10 min | No (UX only) |
| Autostart | 2 tests | 4 min | Yes |
| Migration validation | 2 tests | 2 min | Yes (if kbblock was installed) |
| Clean shutdown | 1 test | 1 min | **YES (SHOWSTOPPER)** |

**Total: 25 tests, ~40 minutes** (or ~30 minutes if kbblock was never installed, skipping migration tests).

**Showstopper count: 3** (Tests 7, 8, 11).

---

## Pre-flight Checklist

Do these in order. Do not skip.

### PF-1: Backup current state

**Steps:**
1. Open Task Manager → Details. Note any running `kbblock.exe` or `switchboard.exe` processes. If either is running, note the PID for later reference.
2. (Optional but wise) Take a quick registry export of the autostart key:
   ```powershell
   reg export "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" C:\Backup\Run-before-switchboard.reg
   ```
3. (Optional) Screenshot the scheduled tasks list:
   ```powershell
   schtasks /Query | Out-File C:\Backup\tasks-before-switchboard.txt
   ```

**Expected:** You have a rollback path if things go sideways.

---

### PF-2: Check for legacy kbblock installation

**Steps:**
1. Check for the old HKCU Run value:
   ```powershell
   reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v kbblock
   ```
2. Check for old scheduled tasks:
   ```powershell
   schtasks /Query /TN kbblock-logon
   schtasks /Query /TN kbblock-boot-recover
   ```
3. Check for the legacy process:
   ```powershell
   Get-Process kbblock -ErrorAction SilentlyContinue
   ```

**Expected:**
- If ALL THREE return "not found" / "ERROR: The system cannot find the file specified" → **kbblock was never installed**. Skip to PF-4.
- If ANY return a match → **kbblock WAS installed**. Continue to PF-3.

**Note:** Record which were found. You'll cross-check these in the Migration Validation tests later.

---

### PF-3: Run the kbblock migration script (if needed)

**Setup:** Only run this if PF-2 found kbblock remnants.

**Steps:**
1. Open an **elevated PowerShell** (Run as Administrator).
2. Verify the script exists:
   ```powershell
   Test-Path .\scripts\uninstall-kbblock-legacy.ps1
   ```
   Expected: `True`.
3. Dry-run preview:
   ```powershell
   pwsh -ExecutionPolicy Bypass -File .\scripts\uninstall-kbblock-legacy.ps1 -WhatIf
   ```
   Read the output. It should list exactly what WOULD be removed.
4. Execute for real:
   ```powershell
   pwsh -ExecutionPolicy Bypass -File .\scripts\uninstall-kbblock-legacy.ps1
   ```
   Watch the output for "Removed:" lines. If you see "Access Denied" or other errors, STOP and flag to Elaine.
5. (Optional, if you're paranoid like me) Reboot now to flush any stale mutex handles. Not strictly required, but eliminates one variable.

**Expected:**
- Script completes with exit code 0 (no red PowerShell errors).
- Final summary shows "removed" or "not present" for all five categories (process, Run value, 2× tasks, app-data).
- No "ERROR:" lines in the output.

**Pass:** Script ran cleanly. Continue to PF-4.

**Fail:** Script errored. Possible causes: not elevated (should auto-elevate but double-check), file missing, PowerShell 5.x compatibility issue (script prefers pwsh 7+). File issue with Elaine + Jerry before proceeding.

---

### PF-4: Copy release binary to stable test location

**Steps:**
1. Confirm the release build exists and is the correct size:
   ```powershell
   Get-Item .\target\aarch64-pc-windows-msvc\release\switchboard.exe | Select-Object Length, LastWriteTime
   ```
   Expected: `Length` ≈ 464,896 bytes (454 KB). `LastWriteTime` = today's date (2026-04-22).
2. Create a stable test directory outside the build tree:
   ```powershell
   mkdir C:\Users\davidtagler\Code\bluetooth-keyboard-app\output -Force
   ```
3. Copy the binary:
   ```powershell
   Copy-Item .\target\aarch64-pc-windows-msvc\release\switchboard.exe C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe -Force
   ```
4. Verify the copy:
   ```powershell
   Get-Item C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe | Select-Object Length
   ```
   Expected: Same 464,896 bytes.

**Expected:** Binary is copied. All remaining tests reference `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe`. Do NOT run from `target\` — that path can be wiped by `cargo clean`.

**Note:** If you later need to re-test after a rebuild, overwrite `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe` with the new binary. The stable path matters for autostart testing (scheduled tasks will reference this path).

---

### PF-5: Record baseline state

**Steps:**
1. Note the current BT keyboard state (Nuphy Air75): **connected** or **disconnected**? Paired or unpaired? Write it down.
2. Test the built-in keyboard: Open Notepad, type on the built-in Surface keyboard. Characters appear? Write "built-in works at start" in your notes.
3. Note the current Windows theme (Settings → Personalization → Colors → "Choose your mode"). Record: **Dark** or **Light**?
4. Confirm NO switchboard or kbblock process is running:
   ```powershell
   Get-Process switchboard,kbblock -ErrorAction SilentlyContinue
   ```
   Expected: No output (both processes absent).

**Expected:** You have a clean baseline. If built-in keyboard DOESN'T work at start, STOP — you're already in a bad state (possibly from a previous test crash). Reboot and retry.

---

## Smoke Tests

These verify the app launches, shows a tray icon, respects the single-instance mutex, and menu items are present. Quick sanity checks before digging into the heavy tests.

---

### Test 1: First launch + tray icon visible

**Setup:** No switchboard process running (verified in PF-5). `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe` exists.

**Steps:**
1. Double-click `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe` (or run from PowerShell: `& 'C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe'`).
2. Wait 2 seconds.
3. Look for the tray icon in the system tray (bottom-right, may be in the overflow chevron).
   - **Win11 tray gotcha:** New icons often hide in the overflow by default. Click the chevron (^) to expand hidden icons.
4. Hover over the icon. Note the tooltip text.

**Expected:**
- **PASS:** Tray icon appears within 2 seconds. Tooltip says **"SwitchBoard"** (CamelCase, per rename decision). Icon is one of the keycap variants (dark or light § glyph on Surface blue stroke — which variant depends on your current theme, tested in detail in the Theme Swap section).
- Process `switchboard.exe` is visible in Task Manager → Details.

**Fail conditions:**
- No tray icon after 10 seconds → app may have crashed on launch. Check Task Manager; if process is gone, check for crash dump or error dialog. Flag to Newman + Jerry.
- Tray icon shows but is the generic Windows app icon → embedded ICO resources didn't load. Flag to Jerry.
- Tooltip says "kbblock" or wrong text → rename incomplete.

**Notes:** ☐ PASS / ☐ FAIL  
Observations: _______________________________________________________________

---

### Test 2: Tray menu items present

**Setup:** Continuing from Test 1. App running, tray icon visible.

**Steps:**
1. Right-click the SwitchBoard tray icon.
2. Note the menu items that appear.

**Expected:**
- Menu contains at minimum: **"Exit"** (or "Quit" — check the actual wording).
- Menu may also contain: status text (e.g., "Idle" / "Active"), settings items, about/version info.
- Menu is responsive (appears within 0.5s of right-click).

**Pass:** Menu appears with an Exit/Quit item.

**Fail:** Menu doesn't appear, or appears but is empty, or appears but has no way to exit the app → tray integration broken. Flag to Kramer (who owns tray integration).

**Notes:** ☐ PASS / ☐ FAIL  
Menu items seen: ____________________________________________________________

---

### Test 3: Single-instance mutex (second launch blocked)

**Setup:** Continuing from Test 2. First instance still running.

**Steps:**
1. Try to launch `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe` a **second time** (double-click again, or run from a new PowerShell window).
2. Watch for a message dialog or error.
3. Check Task Manager → Details. Count `switchboard.exe` processes.

**Expected:**
- Second launch is **blocked**. One of two behaviors:
  - (A) Second launch exits immediately with no error dialog (silent fail — acceptable).
  - (B) Second launch shows a brief message box like "SwitchBoard is already running" then exits.
- Task Manager shows **exactly ONE** `switchboard.exe` process (the first one).

**Pass:** Second launch does not start a duplicate process. Mutex name is `switchboard` (not `kbblock`).

**Fail:** Two `switchboard.exe` processes appear → mutex is broken or renamed incorrectly. This would allow duplicate hook registrations (undefined behavior, possible lockout). Flag to Newman immediately.

**Critical note:** The mutex name change from `kbblock` to `switchboard` is one of the BREAKING changes in this session. If the mutex still uses the old name, a running kbblock from a previous install would block the new switchboard, or vice versa. That's why PF-3 (migration script) must run first.

**Notes:** ☐ PASS / ☐ FAIL  
Behavior on second launch: ___________________________________________________

---

## Core BT Keyboard Hook Tests

These are the **heart of the app** — the original feature Brady built this for. The hook bug Brady reported earlier in the session (reboot with Nuphy connected left internal keyboard enabled) should now be fixed. These tests verify it.

**⚠️ Test 8 is a SHOWSTOPPER.** If the reboot test fails, roll back.

---

### Test 4: Connect Nuphy → internal keyboard disabled

**Setup:** 
- SwitchBoard running (from Test 1–3).
- Nuphy Air75 **disconnected** or **off** at start.
- Built-in keyboard **works** (verified in PF-5 or re-verify now in Notepad).

**Steps:**
1. Open Notepad. Type on the built-in keyboard. Confirm characters appear. Leave Notepad open.
2. Turn on the Nuphy Air75 (or connect it if it's just paired but not connected).
3. Wait for the BT pairing/connection chime (Windows plays a sound).
4. **Immediately** type on the built-in keyboard in Notepad.

**Expected:**
- Built-in keyboard produces **no characters** in Notepad (keys are blocked/ignored).
- Typing on the **Nuphy** produces characters in Notepad (Nuphy works).
- Tray icon tooltip may update to show "Active" or similar status (check by hovering).

**Pass:** Built-in keyboard is disabled within ~2 seconds of Nuphy connection. Nuphy works.

**Fail:**
- Built-in still works after Nuphy connects → device-disable logic didn't fire. Possible causes: BT keyboard not detected (wrong VID/PID filter?), SetupAPI call failed, permission issue. Flag to Kramer (device detection) + Newman (SetupAPI toggle).
- Both keyboards work → disable call was skipped or failed silently.
- **Neither keyboard works** → CRITICAL LOCKOUT. Trigger fail-safe immediately (Test 7 Escape-hold, or Test 11 Exit from tray). If fail-safe works, file P0. If fail-safe doesn't work, hard-reboot (hold power button), ROLLBACK, file P0 with Newman + Jerry.

**Notes:** ☐ PASS / ☐ FAIL  
Built-in status after Nuphy connect: _____________________________________________

---

### Test 5: Disconnect Nuphy → internal keyboard re-enabled

**Setup:** Continuing from Test 4. Nuphy connected, built-in disabled.

**Steps:**
1. Notepad still open from Test 4.
2. Turn off the Nuphy Air75 (power switch, or go to Windows Settings → Bluetooth & devices → Disconnect).
3. Wait for the BT disconnection chime.
4. **Immediately** type on the built-in keyboard in Notepad.

**Expected:**
- Built-in keyboard produces characters in Notepad within ~2 seconds (re-enabled).
- Tray icon tooltip may update to "Idle" (check by hovering).

**Pass:** Built-in keyboard works again after Nuphy disconnect.

**Fail:** Built-in remains disabled after Nuphy disconnect → re-enable logic didn't fire. **CRITICAL** — user is now locked out of their keyboard with no BT fallback. Trigger fail-safe (Test 7 Escape-hold or Test 11 Exit). If fail-safe works, file P0. If fail-safe doesn't work, hard-reboot, ROLLBACK.

**Notes:** ☐ PASS / ☐ FAIL  
Built-in status after Nuphy disconnect: __________________________________________

---

### Test 6: Rapid connect/disconnect (flapping)

**Setup:** Continuing from Test 5. Nuphy disconnected, built-in works.

**Steps:**
1. Perform 3 rapid cycles: Turn Nuphy on → wait for connect chime → turn Nuphy off → wait for disconnect chime. Repeat 3 times in quick succession (~30 seconds total).
2. After the 3rd cycle, Nuphy should be **off**. Wait 5 seconds.
3. Test the built-in keyboard in Notepad.

**Expected:**
- Built-in keyboard works (enabled).
- No app crash (check Task Manager — `switchboard.exe` process still present, same PID as Test 1).
- Tray icon still present and responsive (right-click menu still works).

**Pass:** App survives rapid BT flapping; built-in ends in the correct state (enabled because Nuphy is off).

**Fail:**
- App crashes during flapping → race condition or resource leak in the BT event handler. Flag to Kramer + Newman.
- Built-in is disabled even though Nuphy is off → state tracking bug. **CRITICAL** — trigger fail-safe, file P0.
- Built-in is enabled but tray icon says "Active" → tooltip/state mismatch (violates Safety Invariant #12 "tooltip truth"). Flag to Newman (lower severity than lockout, but still a bug).

**Notes:** ☐ PASS / ☐ FAIL  
App behavior during flapping: _________________________________________________

---

### Test 7: Escape fail-safe — ⚠️ SHOWSTOPPER ⚠️

**This is the most important test in the entire plan. If this fails, ROLLBACK IMMEDIATELY.**

**Setup:**
- SwitchBoard running.
- Built-in keyboard currently **disabled** (Nuphy connected, or re-connect it now if you disconnected for Test 5/6).
- Verify built-in is disabled: Open Notepad, type on built-in → no characters should appear.

**Steps:**
1. With the built-in keyboard blocked, **press and hold the Escape key for a full 10 seconds.** Use a watch or count "one-Mississippi, two-Mississippi, ..." Don't guess. Don't release early.
2. After 10 seconds, release Escape.
3. **Immediately** try typing on the built-in keyboard in Notepad.

**Expected:**
- Built-in keyboard is **re-enabled** (typing produces characters in Notepad).
- `switchboard.exe` process exits cleanly (check Task Manager — process should be gone within 1–2 seconds).
- Tray icon disappears (may linger briefly due to Win11 tray cleanup lag — mouse over the spot; if it's a ghost, Win11 will clear it).

**PASS:** Built-in keyboard works; app process is gone. **This is the safety contract.**

**FAIL = ROLLBACK:**
- If the built-in keyboard does **NOT** come back after 10 seconds of Escape-hold, the fail-safe is BROKEN.
  - **Immediate recovery:** If you still have the Nuphy connected and working, use it to navigate to the tray icon → right-click → Exit (Test 11 path). If that doesn't work either, **unplug power and hold the power button to hard-reboot.**
  - **Post-recovery:** ROLLBACK to the previous known-good `switchboard.exe`. Do NOT ship this build. File a P0 issue with Newman + Jerry. Describe exactly what you did: Nuphy connect → built-in disabled → Escape-hold 10s → built-in still disabled.
  - **Root cause to investigate:** Did the hook callback deadlock? Did the `ESCAPE_HOLD_DURATION` constant get changed? Was the escape-hold detection code removed or broken during the theme-swap refactor?

**Alternate fail mode:** Built-in comes back, but app does NOT exit → less severe, but still a bug. The fail-safe should exit the app completely to remove all hooks. If this happens, manually exit via tray menu (Test 11), then file an issue (not P0, but high priority).

**Notes:** ☐ PASS / ☐ FAIL  
Built-in status after Escape-hold: _______________________________________________  
App exit behavior: ___________________________________________________________

---

### Test 8: Reboot with Nuphy connected → app autostarts, internal disabled — ⚠️ SHOWSTOPPER ⚠️

**This test verifies the original bug Brady reported is actually fixed.**

**Context:** Brady's original symptom (from George's history, §Core Context): reboot with Nuphy connected left the internal keyboard enabled even though the app autostarted. This session's work should have fixed that.

**Setup:**
1. If you triggered the fail-safe in Test 7, the app exited. Re-launch it: `& 'C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe'`.
2. **Nuphy Air75 must be connected and powered on** before the reboot. Turn it on now. Wait for the connection chime.
3. Verify the app disabled the built-in: type on built-in in Notepad → no characters (if you see characters, the app isn't working; STOP and debug Test 4 first).
4. **Enable autostart:** This step is critical — the test only makes sense if the app autostarts. Verify one of these:
   - (A) HKCU Run value: `reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v switchboard` → should return the path to `switchboard.exe`.
   - (B) Or scheduled task: `schtasks /Query /TN switchboard-logon` → should exist and point to `switchboard.exe`.
   - If neither exists, add the Run value manually for now (we'll test autostart toggle later in Test 12):
     ```powershell
     Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "switchboard" -Value "C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe"
     ```

**Steps:**
1. With the Nuphy connected and ON, and the built-in disabled, **reboot the Surface.**
   ```powershell
   Restart-Computer
   ```
   **Critical:** Do NOT turn off the Nuphy during reboot. Leave it powered on the entire time.
2. After reboot, log back in to Windows.
3. **Immediately after desktop loads,** check Task Manager → Details for `switchboard.exe`. (It should appear within ~5 seconds of login.)
4. Open Notepad. Try typing on the **built-in** keyboard.

**Expected:**
- `switchboard.exe` process is running (autostart worked).
- Tray icon is visible (may be in overflow chevron initially).
- Built-in keyboard is **disabled** (typing on built-in produces no characters in Notepad).
- Nuphy keyboard works (typing on Nuphy produces characters).

**PASS:** App autostarted AND the built-in is disabled. **This is the fix for Brady's original bug.**

**FAIL = ROLLBACK:**
- If app autostarts but the built-in still works even though Nuphy is connected → **the original bug is NOT fixed.** 
  - Verify Nuphy is actually connected (Settings → Bluetooth & devices — should say "Connected"). If Nuphy didn't reconnect after reboot (some BT devices don't auto-reconnect), this test is invalid; retry after manually connecting Nuphy.
  - If Nuphy IS connected but built-in still works → the device-disable-on-boot logic is broken. Flag to Newman + Kramer as P0. ROLLBACK.
- If app does NOT autostart at all → autostart is broken (Run value or scheduled task failed). This is a separate bug from the hook logic; test autostart explicitly in Test 12. For now, manually launch the app, verify Test 4 still works, then continue to Test 9.

**Notes:** ☐ PASS / ☐ FAIL  
App autostarted? ☐ Yes / ☐ No  
Built-in status after reboot with Nuphy connected: ________________________________

---

### Test 9: Reboot WITHOUT Nuphy → app autostarts IDLE, internal enabled

**This is the inverse of Test 8 — verifies the app doesn't over-block.**

**Setup:** Continuing after Test 8 (or after manual launch if Test 8's autostart failed). App is running. Nuphy is connected.

**Steps:**
1. Turn OFF the Nuphy Air75 (power switch).
2. Wait for the disconnect chime. Verify the built-in works (type in Notepad → characters appear).
3. **Reboot the Surface.**
   ```powershell
   Restart-Computer
   ```
   **Critical:** Nuphy must remain OFF during the entire reboot + login.
4. After reboot, log back in.
5. Check Task Manager for `switchboard.exe`.
6. Open Notepad. Type on the built-in keyboard.

**Expected:**
- `switchboard.exe` process is running (autostart worked).
- Tray icon visible.
- Built-in keyboard **works** (typing produces characters in Notepad).
- Tray tooltip says "Idle" or similar (hover to check).

**Pass:** App autostarts in IDLE mode; built-in keyboard is NOT blocked when no BT keyboard is present.

**Fail:**
- Built-in is disabled even though Nuphy is off → app is over-blocking. **CRITICAL** — user is locked out at boot with no BT fallback. Trigger fail-safe (if you can reach the app via tray, Exit; otherwise hard-reboot). ROLLBACK. File P0.
- App doesn't autostart → autostart bug (same as Test 8 fail). Flag separately.

**Notes:** ☐ PASS / ☐ FAIL  
Built-in status after reboot without Nuphy: _______________________________________

---

## Fail-Safe Validation

Already covered in **Test 7** above (Escape-hold). No additional tests needed here. Test 7 is the non-negotiable showstopper.

---

## Theme Swap Tests

**Reference:** See `.squad/files/test-plans/2026-04-22-theme-swap-manual-test.md` for the full 7-test theme-swap plan.

**Scope:** These tests verify the embedded ICO resources (101 dark, 102 light) load correctly, the tray icon updates live when Windows theme changes, and no handle leaks occur during rapid theme toggles.

**Critical tests from that plan:**
- **Test 1:** EXE icon in File Explorer (dark variant, resource 101, all sizes sharp)
- **Test 2:** Initial tray icon — DARK theme (resource 101)
- **Test 3:** Initial tray icon — LIGHT theme (resource 102)
- **Test 4:** Live swap DARK → LIGHT (WM_SETTINGCHANGE handler)
- **Test 5:** Live swap LIGHT → DARK
- **Test 6:** Multiple rapid toggles (handle leak check)
- **Test 7:** Escape fail-safe still works after icon code added (DUPLICATE of this plan's Test 7 — already covered above)

**For this full validation, you can either:**
- (A) **Run all 7 tests from the theme-swap plan** (adds ~10 minutes).
- (B) **Run just Tests 1, 4, and 6** from the theme-swap plan as a spot-check (~5 minutes) — this covers the critical paths (resource embedding, live swap, handle leak).

**Recommendation:** Option B is sufficient for this validation IF the theme-swap plan was already validated in an earlier session. If this is the FIRST time testing the theme swap on real hardware, run all 7 tests from the theme-swap plan.

**Showstopper status:** Theme swap failures are **NOT** rollback-worthy (they're UX polish, not safety). BUT: Test 7 of the theme-swap plan (Escape fail-safe) IS a showstopper, and it's already covered as Test 7 in this plan. If you skipped Test 7 above, DO NOT SKIP IT.

**Notes:** ☐ Ran all 7 theme tests / ☐ Ran spot-check (Tests 1, 4, 6) / ☐ Skipped (already validated)  
Theme test results summary: ___________________________________________________

---

## Autostart Tests

These verify the renamed autostart mechanisms (HKCU Run value `switchboard` and scheduled tasks `switchboard-logon`, `switchboard-boot-recover`) work correctly.

---

### Test 10: Autostart via HKCU Run (logon)

**Setup:** 
- App is NOT currently running (if still running from previous tests, Exit via tray menu first).
- HKCU Run value should exist from Test 8 setup. Verify:
  ```powershell
  reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v switchboard
  ```
  Expected: Points to `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe` (or whatever stable path you chose in PF-4).

**Steps:**
1. Sign out of Windows (Start → User icon → Sign out). DO NOT reboot — just sign out.
2. Sign back in.
3. After desktop loads, check Task Manager → Details for `switchboard.exe`.
4. Check for tray icon.

**Expected:**
- `switchboard.exe` process is running within ~5 seconds of desktop load.
- Tray icon appears.

**Pass:** App autostarts via Run value at logon.

**Fail:** App doesn't start → possible causes:
- Run value points to wrong path (verify with `reg query` above).
- Run value uses old `kbblock` name → migration script didn't run, or registry edit failed. Re-run PF-3.
- App crashes on launch → check Event Viewer for crash dumps. Flag to Newman + Jerry.

**Notes:** ☐ PASS / ☐ FAIL  
Autostart behavior at logon: __________________________________________________

---

### Test 11: Clean shutdown via tray Exit — ⚠️ SHOWSTOPPER ⚠️

**This test verifies Exit from the tray menu releases all hooks and re-enables the built-in keyboard. If this fails, users get locked out on next boot.**

**Setup:**
- App running (from Test 10 autostart, or manually launched).
- Nuphy connected. Built-in disabled (if not, connect Nuphy now and verify built-in is blocked).

**Steps:**
1. Verify the built-in keyboard is currently disabled: type in Notepad → no characters.
2. Right-click the SwitchBoard tray icon.
3. Click **Exit** (or **Quit** — whatever the menu item says).
4. **Immediately** try typing on the built-in keyboard in Notepad.

**Expected:**
- Built-in keyboard is **re-enabled** (characters appear in Notepad) within 1 second of clicking Exit.
- `switchboard.exe` process is gone from Task Manager.
- Tray icon disappears.

**PASS:** Exit releases hooks and restores the built-in keyboard. **This is critical for clean shutdown.**

**FAIL = ROLLBACK:**
- If the built-in keyboard is **still disabled** after Exit → the shutdown handler didn't call the re-enable logic. **CRITICAL LOCKOUT SCENARIO** — if the app crashes or is force-killed later (e.g., Windows Update reboot), the built-in will stay disabled and the user is locked out at next boot. ROLLBACK. File P0 with Newman.
  - **Immediate recovery:** Hard-reboot. The boot-recover scheduled task should fire and run `switchboard.exe --recover`, which should re-enable the built-in. If that doesn't work, you'll need to boot to Safe Mode or use an external USB keyboard. This is exactly the nightmare scenario the fail-safe is meant to prevent.

**Alternate fail mode:** Built-in re-enables, but app process doesn't exit (still running in Task Manager) → less severe, but still a bug. The Exit handler should terminate the process cleanly. File an issue.

**Notes:** ☐ PASS / ☐ FAIL  
Built-in status after Exit: ___________________________________________________  
App exit behavior: ___________________________________________________________

---

### Test 12: Boot-recover scheduled task (crash recovery)

**This test verifies the `switchboard-boot-recover` task fires after a simulated crash and re-enables the built-in keyboard.**

**Setup:**
1. App should be running and autostart should be enabled (from Test 10).
2. Nuphy connected, built-in disabled.

**Steps:**
1. Verify the boot-recover task exists:
   ```powershell
   schtasks /Query /TN switchboard-boot-recover
   ```
   Expected: Task exists, trigger = "At system startup", runs `switchboard.exe --recover` (or similar).
2. **Simulate a crash:** Force-kill the `switchboard.exe` process:
   ```powershell
   Stop-Process -Name switchboard -Force
   ```
3. Verify the process is gone from Task Manager.
4. **DO NOT** manually re-enable the built-in keyboard. Leave it in the disabled state.
5. **Reboot the Surface:**
   ```powershell
   Restart-Computer
   ```
   (Nuphy can stay connected or disconnected for this test — doesn't matter. The boot-recover task should fire regardless.)
6. After reboot, log back in.
7. Check Task Manager → scheduled tasks (or Event Viewer → Task Scheduler logs) to confirm `switchboard-boot-recover` executed.
8. Open Notepad. Type on the built-in keyboard.

**Expected:**
- Built-in keyboard **works** (typing produces characters in Notepad).
- Task Scheduler logs show `switchboard-boot-recover` executed at boot (check Event Viewer → Applications and Services Logs → Microsoft → Windows → TaskScheduler → Operational — look for Task ID 200 or 201 with task name `switchboard-boot-recover`).
- The main `switchboard.exe` process may or may not be running (depends on whether autostart also fired). If it IS running, that's fine — the boot-recover task should have run FIRST and re-enabled the keyboard before the main instance started.

**Pass:** Boot-recover task fired and restored the built-in keyboard after a crash.

**Fail:**
- Built-in is still disabled after reboot → boot-recover task didn't fire, or the `--recover` flag logic is broken. **CRITICAL** — this is the last-resort safety net after a crash. If this fails, a crash leaves the user locked out until manual intervention (USB keyboard or Safe Mode). Flag to Newman + Elaine as P0. ROLLBACK.
- Task exists but didn't execute → possible causes: task trigger misconfigured (should be "At startup", not "At logon"), task user/permissions wrong (should run as SYSTEM or the user who installed it), task disabled. Check Task Scheduler UI.

**Notes:** ☐ PASS / ☐ FAIL  
Boot-recover task execution confirmed? ☐ Yes / ☐ No  
Built-in status after reboot: _________________________________________________

---

## Migration Validation (kbblock → switchboard)

These tests verify the migration script (PF-3) actually removed all legacy kbblock state, and the new switchboard install doesn't conflict with any leftovers.

**Only run these if you had kbblock installed previously (PF-2 found remnants).** If kbblock was never installed, mark these as **N/A** and skip to Clean Shutdown.

---

### Test 13: Old kbblock mutex/Run/tasks are gone

**Setup:** Post-migration (after PF-3). App may or may not be running — doesn't matter.

**Steps:**
1. Check for the old mutex by trying to launch a legacy `kbblock.exe` (if you still have one lying around from a backup). It should launch without mutex conflict. If you don't have a legacy binary, skip this sub-check.
2. Re-check the HKCU Run value:
   ```powershell
   reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v kbblock
   ```
   Expected: **"ERROR: The system cannot find the file specified"** (key is gone).
3. Re-check the old scheduled tasks:
   ```powershell
   schtasks /Query /TN kbblock-logon
   schtasks /Query /TN kbblock-boot-recover
   ```
   Expected: **"ERROR: The system cannot find the path specified."** (both tasks are gone).
4. Check for stale `kbblock.exe` process:
   ```powershell
   Get-Process kbblock -ErrorAction SilentlyContinue
   ```
   Expected: No output (process is gone).

**Expected:** All four checks return "not found" / no output. Old kbblock state is fully removed.

**Pass:** Migration succeeded. No legacy kbblock identifiers remain.

**Fail:** If ANY of the old kbblock identifiers still exist:
- Run value still present → migration script didn't remove it, or registry permissions issue. Re-run PF-3 from an elevated shell.
- Tasks still present → same, or tasks were marked "protected" (unlikely). Re-run PF-3.
- Process still running → either the migration script didn't stop it, or user manually relaunched the old binary. Stop it: `Stop-Process -Name kbblock -Force`, then reboot to flush.

**Notes:** ☐ PASS / ☐ FAIL / ☐ N/A (kbblock never installed)  
Remaining legacy state: _______________________________________________________

---

### Test 14: New switchboard identifiers are present and distinct

**Setup:** Post-migration, new switchboard installed (app running or at least autostart configured).

**Steps:**
1. Check the NEW Run value:
   ```powershell
   reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v switchboard
   ```
   Expected: Points to `C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe` (or your stable path).
2. Check the NEW scheduled tasks:
   ```powershell
   schtasks /Query /TN switchboard-logon
   schtasks /Query /TN switchboard-boot-recover
   ```
   Expected: Both tasks exist and point to `switchboard.exe` (not `kbblock.exe`).
3. Check the NEW process:
   ```powershell
   Get-Process switchboard -ErrorAction SilentlyContinue
   ```
   Expected: One `switchboard.exe` process (if app is running from previous tests).

**Expected:** All three new identifiers exist and reference the new `switchboard.exe` binary.

**Pass:** New install is clean. No confusion between old and new.

**Fail:**
- New Run value or tasks point to old `kbblock.exe` path → something went wrong during setup. Manually edit:
  ```powershell
  Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "switchboard" -Value "C:\Users\davidtagler\Code\bluetooth-keyboard-app\output\switchboard.exe"
  ```
  And recreate tasks if needed (see `scripts\` for task creation helpers, or ask Elaine).

**Notes:** ☐ PASS / ☐ FAIL / ☐ N/A  
New identifiers verified: _____________________________________________________

---

## Clean Shutdown Validation

Already covered in **Test 11** above (Exit from tray menu releases hooks). No additional tests needed here. Test 11 is a showstopper.

---

## Final Checklist

After completing all tests:

- [ ] **All 3 SHOWSTOPPER tests passed:** Test 7 (Escape fail-safe), Test 8 (Reboot with BT connected), Test 11 (Clean Exit releases hooks).
- [ ] No processes left running: `Get-Process switchboard -ErrorAction SilentlyContinue` → no output (or running if you want to leave it autostarted).
- [ ] Built-in keyboard works in its final state (should be enabled if Nuphy is off, or disabled if Nuphy is on and app is running).
- [ ] Tray icon behavior matches expectations (no ghost icons, correct theme variant if app is running).

---

## Reporting Back

For each test, record: **☐ PASS** / **☐ FAIL** / **☐ N/A** (with reason if N/A), and any unexpected observations even on PASS.

**If any SHOWSTOPPER test fails (7, 8, 11):** STOP. ROLLBACK to the previous known-good binary. Do not ship. File P0 with Newman + Jerry (and Kramer for Test 8).

**If non-showstopper tests fail:** File issues with priority based on severity:
- Autostart issues → flag Elaine + Newman (P1).
- Theme swap issues → flag Jerry + Elaine (P2 — UX only).
- Migration issues → flag Elaine (P2 — only affects users upgrading from kbblock).

**Post-test cleanup:** If you want to leave the app autostarted for daily use, great — that's the whole point. If you want to disable autostart for now:
```powershell
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name switchboard
schtasks /Delete /TN switchboard-logon /F
schtasks /Delete /TN switchboard-boot-recover /F
```

---

## Appendix: Known Win11 Gotchas

1. **Dual-theme dropdown:** Settings → Personalization → Colors may show TWO dropdowns: "Choose your mode" (overall) and "Choose your default Windows mode" (specific to Win11 22H2+). SwitchBoard reads `SystemUsesLightTheme`, which is the **Windows mode**, NOT the "App mode" (`AppsUseLightTheme`). If testing theme swap, make sure you're toggling the right one.

2. **Tray icon overflow:** New tray icons often hide in the overflow chevron (^) on first launch. Pin the SwitchBoard icon to the main tray for easier testing (drag from overflow to main tray area).

3. **Registry write without WM_SETTINGCHANGE:** If you set `SystemUsesLightTheme` via `Set-ItemProperty` from PowerShell, it does NOT broadcast `WM_SETTINGCHANGE` automatically. The app won't know the theme changed. Either toggle via Settings UI (broadcasts automatically), or manually broadcast using the PowerShell snippet in the theme-swap plan's Diagnostics section.

4. **Task Scheduler log retention:** Event Viewer only keeps a few days of Task Scheduler logs by default. If you're testing boot-recover (Test 12) days after install, you may not see logs from the original install. That's fine — the test only cares about the MOST RECENT boot's task execution.

---

## Cross-References

- **Theme-swap detailed tests:** `.squad/files/test-plans/2026-04-22-theme-swap-manual-test.md`
- **Migration script:** `scripts\uninstall-kbblock-legacy.ps1`
- **MSIX packaging (future):** `manifest\MSIX-README.md` (not tested in this plan — deferred until signing cert is available)
- **Rename decision:** `.squad/decisions.md` § "kbblock → switchboard rename"
- **Embedded ICO verification:** `.squad/decisions.md` § "Release build verified"
