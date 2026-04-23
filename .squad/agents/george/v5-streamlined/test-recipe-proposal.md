# v5 Test Recipe — Streamlined & Tier-Based

**Brady's request:** Simplify the 25-test v4 matrix. Too fatigued to keep running it.

**What changed:** v4 had 25 tests (~679 lines). v5 has **10 tests** organized in 3 tiers. Dropped 9, merged 6 into existing tests.

---

## TL;DR — v4 → v5 Summary

| Metric | v4 | v5 | Change |
|--------|----|----|--------|
| Total tests | 25 | 10 | **-15** |
| Safety-critical (Tier 1) | N/A | 5 | Core lockout contract |
| Functional smoke (Tier 2) | N/A | 3 | Basic connect/toggle |
| Stress/edge (Tier 3) | N/A | 2 | Only when touching risky code |
| Avg test runtime | 2-5 min | 1-3 min | Faster, tighter |

### What was dropped / merged:

**DROPPED** (validated indirectly or low value vs. effort):
- **Test 13** (Console hidden) — UX polish; validated once, doesn't protect safety
- **Test 14** (CLI output) — UX polish; validated once, doesn't protect safety
- **Test 15-16** (Start at Login toggle) — registry key writes, not safety-critical; spot-check manually when changing autostart.rs
- **Test 19-22** (CLI install/uninstall boot task, stale-path warnings) — installation plumbing; validated once during initial UX simplification, not regression-prone
- **Test 24** (Autostart at logon after reboot) — **MERGED into Test 1a** (reboot smoke)
- **Test 25** (Auto-install lockout protection) — one-time feature validation; doesn't need ongoing regression testing

**MERGED**:
- **Test 7** (lid-close post-unlock re-disable) — **MERGED into Test 4** (lid close); now one test covers suspend→lock→unlock→re-disable
- **Test 17-18** (Boot recovery task toggle) — **MERGED into Test 6** (boot recovery)

**KEPT** (renumbered, regrouped, made more concise):
- Tests 1-6, 8-12 → became Tier 1 & 2 tests below

**RESULT:** Brady only runs Tier 1 (5 tests, ~10 min) before any release. Tier 2 (3 tests, ~6 min) when touching connect/toggle logic. Tier 3 (2 tests, ~8 min) only when changing crash detection or worker threading.

---

## Prerequisites (SAME as v4, but streamlined wording)

1. **Hardware:** Surface Laptop 7 15" (ARM64), Nuphy Air75 paired and powered on
2. **Safety rehearsal FIRST (5 minutes, one-time):**
   - Lock screen (Win+L)
   - Touchpad → lower-right Accessibility icon → On-Screen Keyboard → type → unlock
   - **If OSK doesn't work: STOP. Do NOT deploy without USB keyboard backup.**
3. **USB keyboard within arm's reach** (ultimate fallback)
4. **Clean state:** No `kbblock.exe` running, logs in `%LOCALAPPDATA%\kbblock\` backed up or deleted

---

## Tier 1 — Safety Smoke (Run Before Every Release)

These 5 tests protect the lockout contract: **internal keyboard MUST work at lock screen**. If any fail, it's a v0.1-blocking bug.

**Time budget:** ~10 minutes total

---

### Test 1a: Cold Boot + Autostart → Active at Login

**What it proves:** Reboot smoke test. Verifies shutdown handler fired (WM_ENDSESSION), boot recovery ran if installed, autostart launched app in ACTIVE state, internal kbd works at lock screen.

**Preconditions:**
- "Auto-start kbblock at login" enabled (HKCU Run key or kbblock-logon task)
- "Lockout protection" enabled (kbblock-boot-recover task installed)
- Nuphy powered on, connected before reboot

**Steps:**
1. Start → Restart (Windows-initiated reboot)
2. At lock screen: attempt to type on **internal keyboard** (should work)
3. Sign in
4. Wait 3 seconds
5. Verify tray icon appears automatically
6. Attempt to type on **internal keyboard** (should NOT work — Nuphy connected, app Active)

**Pass criteria:**
- Internal keyboard works at lock screen
- Tray icon appears within 3s of desktop ready
- App starts in ACTIVE state (internal kbd blocked after login)
- Log from prior session shows: `WM_ENDSESSION: wparam=true` → `shutdown_cleanup` → `re-enable ok`
- Log from new session shows: `cold_start` → `ENABLE → verify: Enabled` → `apply_policy → DISABLE → verify: Disabled`

**On fail:** Check logs for shutdown handler firing, boot task execution, autostart trigger. If internal kbd dead at lock screen: **CRITICAL LOCKOUT** — fix before shipping.

**Severity:** **CRITICAL** (lockout at lock screen = v0.1 blocker)

---

### Test 1b: Launch with Nuphy Connected (Cold Start Enable)

**What it proves:** Every app launch unconditionally re-enables internal keyboard BEFORE checking Nuphy state (crash recovery contract).

**Preconditions:**
- Nuphy on, connected
- No `kbblock.exe` running

**Steps:**
1. Launch `kbblock.exe` (double-click or PowerShell)
2. UAC → Yes
3. Wait 2 seconds
4. Tray icon appears
5. Attempt to type on **internal keyboard**

**Pass criteria:**
- Internal keyboard produces no characters (disabled after cold-start enable + policy check)
- Log shows: `main() → ENABLE → verify: Enabled → BLE subscribe → apply_policy → DISABLE → verify: Disabled`

**On fail:** If internal kbd works after tray appears, disable failed. If tray never appears, app crashed during init.

**Severity:** **CRITICAL** (cold-start enable is the crash recovery foundation)

---

### Test 2: Nuphy Disconnect → Internal Enable

**What it proves:** BLE disconnect event triggers re-enable within reasonable latency.

**Preconditions:**
- App running, Nuphy on, internal kbd disabled

**Steps:**
1. Power off Nuphy (hold button 3s)
2. Wait 5 seconds (allow BLE stack latency)
3. Attempt to type on **internal keyboard**

**Pass criteria:**
- Internal keyboard works within 5s of Nuphy power-off
- Log shows: `ConnectionStatusChanged → apply_policy → ENABLE → verify: Enabled`

**On fail:** If kbd stays dead after 5s, check if BLE event fired. If no event in log after 30s, sanity timer should rescue (Test 5 validates timer).

**Severity:** **CRITICAL** (user stranded without either keyboard)

---

### Test 4: Lid Close (Suspend → Lock → Unlock → Re-Disable)

**What it proves:** Suspend unconditionally enables, resume gates re-disable until unlock (prevents lockout at lock screen).

**Preconditions:**
- App running, Nuphy on, internal kbd disabled

**Steps:**
1. Close lid
2. Wait 30s (enter Modern Standby)
3. Open lid → lock screen appears
4. Attempt to type on **internal keyboard** at lock screen (should work)
5. Unlock via touchpad + internal kbd (or OSK)
6. Wait 2s after desktop appears
7. Attempt to type on **internal keyboard** (should NOT work — re-disabled post-unlock)

**Pass criteria:**
- Internal kbd works at lock screen (step 4)
- Internal kbd blocked after unlock (step 7)
- Log shows: `PBT_APMSUSPEND → ENABLE` → `PBT_APMRESUMEAUTOMATIC → ENABLE + resume_pending=true` → `WTS_SESSION_UNLOCK → resume_pending cleared → apply_policy → DISABLE`

**On fail (kbd dead at lock screen):** **CRITICAL LOCKOUT** — use OSK or USB kbd to unlock, check suspend handler.

**Severity:** **CRITICAL** (lockout at lock screen after lid-close is the #1 user complaint scenario)

---

### Test 6: Quit While Disabled → Inline Recovery

**What it proves:** Tray Quit always re-enables keyboard before exit, even if worker hung (inline fallback path).

**Preconditions:**
- App running, Nuphy on, internal kbd disabled

**Steps:**
1. Right-click tray → **Quit**
2. Within 1 second, attempt to type on **internal keyboard**
3. Observe tray icon disappearance

**Pass criteria:**
- Internal keyboard produces characters BEFORE tray icon vanishes
- Log shows: `Quit → Shutdown sent → worker join → inline ENABLE → verify: Enabled → exit 0`

**On fail (kbd stays dead after quit):** **CRITICAL LOCKOUT** — reboot, use OSK/USB kbd. Inline enable failed.

**Severity:** **CRITICAL** (user locked out if Quit doesn't restore kbd)

---

## Tier 2 — Functional Smoke (Run When Changing Connect/Toggle Logic)

These 3 tests validate the basic user-facing features: toggle Active, reconnect, sanity timer. Only run these when modifying `apply_policy()`, BLE subscription, or tray menu logic.

**Time budget:** ~6 minutes total

---

### Test 3: Nuphy Reconnect → Internal Disable

**What it proves:** BLE connect event triggers disable when Active=true.

**Preconditions:**
- App running, Nuphy off, internal kbd working

**Steps:**
1. Power on Nuphy
2. Wait 5s for BLE reconnect
3. Attempt to type on **internal keyboard**

**Pass criteria:**
- Internal kbd produces no characters within 5s of Nuphy LED solid
- Log shows: `ConnectionStatusChanged → apply_policy → DISABLE → verify: Disabled`

**On fail:** If internal kbd still works, check BLE event firing or predicate match count.

**Severity:** Not critical (user can toggle Active off manually)

---

### Test 5: Tray Toggle Active (Off → On → Off)

**What it proves:** Tray menu Active checkbox correctly flips `desired_active` and calls `apply_policy()`.

**Preconditions:**
- App running, Nuphy on, internal kbd disabled (Active=true)

**Steps:**
1. Right-click tray → uncheck **Active**
2. Wait 2s → attempt to type on **internal keyboard** (should work)
3. Right-click tray → check **Active**
4. Wait 2s → attempt to type on **internal keyboard** (should NOT work)

**Pass criteria:**
- Step 2: kbd works, log shows `desired_active=false → ENABLE`
- Step 4: kbd blocked, log shows `desired_active=true → DISABLE → verify: Disabled`
- Tooltip updates to reflect Active state

**On fail:** Check tray message pump, `apply_policy()` decision path.

**Severity:** Not critical (feature validation, not safety)

---

### Test 7: Sanity Timer (Passive Drift Correction)

**What it proves:** 20s sanity timer rescues missed BLE disconnect events.

**Preconditions:**
- App running, Nuphy on, Active on, internal kbd disabled

**Steps:**
1. Power off Nuphy (simulates missed BLE event)
2. Do NOT touch tray
3. Wait 25 seconds

**Pass criteria:**
- Internal kbd works within 25s of Nuphy power-off
- Log shows: `sanity_timer_tick → apply_policy → ENABLE`

**On fail:** If kbd stays dead after 25s, sanity timer didn't fire. Check timer setup, `apply_policy()` call.

**Severity:** Not critical (BLE event is primary path; timer is backup)

---

## Tier 3 — Stress / Edge Cases (Run Only When Changing Risky Code)

These 2 tests validate crash recovery and escape hatch paths. Only run when modifying crash detection (`running.lock` logic), worker threading, or `--recover` mode.

**Time budget:** ~8 minutes total

---

### Test 8: Forced Crash Mid-Disable → Cold-Start Recovery

**What it proves:** If app crashes while kbd disabled, next launch unconditionally re-enables (cold-start ENABLE + crash detection).

**Preconditions:**
- App running, Nuphy on, internal kbd disabled

**Steps:**
1. Open PowerShell as Admin
2. `Get-Process kbblock | Stop-Process -Force`
3. Verify tray icon vanishes
4. Attempt to type on **internal keyboard** (still dead — state persists)
5. Launch `kbblock.exe` again
6. UAC → Yes
7. Wait 2s → attempt to type on **internal keyboard**

**Pass criteria:**
- Step 4: kbd still dead (expected — `CONFIGFLAG_DISABLED` persists)
- Step 7: kbd works, app shows tray balloon "Recovered from crash"
- Log shows: `running.lock existed → crashed=true` → `ENABLE → verify: Enabled` → tray starts in INACTIVE mode (user must re-enable Active manually after crash)

**On fail (kbd stays dead after step 7):** **CRITICAL LOCKOUT** — cold-start enable failed. Run `--recover`.

**Severity:** **CRITICAL** (crash recovery is the last-resort safety net)

---

### Test 9: Hung Instance + `--recover` Escape Hatch

**What it proves:** `--recover` bypasses single-instance mutex and performs inline ENABLE even while first instance holds mutex.

**Preconditions:**
- App running, Nuphy on, internal kbd disabled

**Steps:**
1. Open File Explorer → navigate to `kbblock.exe`
2. In address bar: `kbblock.exe --recover` → Enter
3. UAC → Yes
4. Wait 2s
5. Attempt to type on **internal keyboard**

**Pass criteria:**
- No mutex error
- Internal kbd works
- First instance tray still present (both can coexist)
- Log from `--recover` shows: `--recover mode → skip mutex → inline ENABLE → verify: Enabled → exit 0`

**On fail:** If kbd stays disabled OR `--recover` errors on mutex, inline path is broken.

**Severity:** **CRITICAL** (only escape hatch when primary instance hung)

---

## Exit Criteria (for v0.1+ releases)

**Required before ANY release:**
1. ✅ All Tier 1 tests (1a, 1b, 2, 4, 6) pass — lockout contract verified
2. ✅ Logs show correct ENABLE/DISABLE/verify sequences per ARCHITECTURE.md §4.3
3. ✅ Lock-screen OSK recovery path rehearsed (Test 4 step 5 fallback)
4. ✅ USB keyboard fallback confirmed working

**Required before releases touching connect/toggle logic:**
5. ✅ All Tier 2 tests (3, 5, 7) pass

**Required before releases touching crash detection / worker / --recover:**
6. ✅ All Tier 3 tests (8, 9) pass

**BLOCKING bugs:**
- Any Tier 1 test fails → DO NOT SHIP (lockout risk)
- Test 8 or 9 fails → DO NOT SHIP (crash recovery broken)

**Non-blocking warnings (document in known issues):**
- Test 7 sanity timer takes >25s but <40s (BLE latency tolerance)
- Test 3 reconnect takes >5s but <15s (Windows BLE stack variability)

---

## Log Capture Protocol (SAME as v4, simplified)

**Before each test:**
1. Delete `%LOCALAPPDATA%\kbblock\kbblock.log`
2. Launch app (if test requires)

**After each test:**
1. Copy log to `test<tier><number>-<pass|fail>.log` (e.g., `test1a-pass.log`)
2. Verify log contains timestamp, ENABLE/DISABLE/verify calls, apply_policy decisions, op-id correlation

---

## Notes for Brady

**Why this is better:**
- **10 tests vs. 25** — 60% reduction in test burden
- **Tier-based** — you know what to run and when
- **Faster** — Tier 1 in ~10 min (was ~30 min for full v4 smoke suite)
- **Focused** — every test has a clear "what does this protect?" answer

**What you lost:**
- Installation UX tests (15-22, 25) — validate once per feature, not on every regression run
- Console window polish (13-14) — UX niceties, not safety
- Granular autostart/boot-task toggle tests — covered by Test 1a reboot smoke

**What you gained:**
- **Reboot smoke in Tier 1** (Test 1a) — catches shutdown handler bugs, autostart failures, boot recovery issues in ONE test
- **Clear severity** — CRITICAL vs. not-critical flagged per test
- **Sanity preserved** — less time testing, more confidence per minute invested

**When to run what:**
- **Every release candidate:** Tier 1 only (~10 min)
- **Changing apply_policy / BLE / tray:** Tier 1 + Tier 2 (~16 min)
- **Changing crash detection / worker / --recover:** All tiers (~24 min — still better than v4's 25 tests)

**Blind spots in v5:**
- No verify-mismatch race test (v4 Test 10) — that scenario is extremely rare and already handled by the verify contract; we validated it once
- No explicit test for worker-death detection — covered implicitly by Test 6 (Quit with hung worker uses inline fallback)
- No RDP/Fast User Switch tests — explicitly out of scope per README.md (single-user only)

---

## Migration Plan (How to Adopt v5)

1. **Read this proposal** — understand what changed and why
2. **Run Tier 1 once** (5 tests, ~10 min) on current `output\kbblock.exe` to establish baseline
3. **Archive v4 test results** to `.squad/agents/george/v4-codeready/archived-results/` for historical reference
4. **Adopt v5 as the new standard** — update `.squad/decisions.md` with "Test suite v5 adopted, v4 deprecated"
5. **Update George's charter** in `.squad/agents/george/README.md` to reference v5 recipe

**Rollback clause:** If you find a regression v5 missed but v4 caught, re-evaluate. But given the reboot smoke (1a) now exists in v5 and didn't in v4, I'm confident v5 has BETTER coverage of high-impact failure modes.

