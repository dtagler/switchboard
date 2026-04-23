# Skill: System Tray App Safety Test Template

**Pattern:** Structured test protocol for Windows tray applications with safety-critical failure modes (lockout, data loss, system corruption).

**When to use:** Any tray app where failure leaves user in degraded state requiring recovery (not just "restart app and it works").

---

## Template Structure

### 1. Prerequisites Section
- **Hardware specification** (exact model, peripherals)
- **Pre-test rehearsal of recovery paths** (e.g., lock-screen OSK, safe mode)
- **Safety backup validation** (e.g., USB keyboard plug test, external display)
- **Clean-state baseline** (no hung processes, fresh logs)

**Why:** Establishes known-good starting point; rehearses recovery before failure occurs.

---

### 2. Safety Net Protocol (Run Before EVERY Test)
- **Hard-fallback validation** (e.g., USB keyboard still works)
- **Log truncation** (isolate per-test logs)
- **Baseline functionality check** (verify system state before test)

**Why:** Detects hardware/environmental failure early; prevents false-negative test results.

---

### 3. Smoke Tests (Happy Path)
- Launch → feature activates
- Primary toggle/control → feature deactivates
- Primary toggle/control → feature reactivates
- Power state transitions (suspend/resume, lid close/open)
- Graceful shutdown (tray Quit)

**Why:** Validates normal user workflows; confirms core feature loop works.

---

### 4. Adversarial Tests (Failure Modes)
- **Forced crash mid-operation** (e.g., `Stop-Process -Force`) + cold boot
- **Hung instance + recovery CLI** (e.g., `--recover` bypass)
- **State verification mismatch** (operation reports success but state unchanged)
- **Lock screen / secure desktop** (Ctrl+Alt+Del, UAC prompt)
- **Multi-user / RDP** (if scoped)

**Why:** Exercises catastrophic failure modes; validates recovery paths work when primary paths fail.

---

### 5. Recovery Drill Matrix
Map each documented recovery procedure to the test(s) that validate it. Format:

| Recovery Procedure | Test Coverage | Status |
|---|---|---|
| Tray menu → disable feature | Smoke Test N | ✅ after Test N |
| CLI `--recover` | Adversarial Test M | ✅ after Test M |
| Lock-screen OSK | Adversarial Test K | ✅ after Test K |
| Hard reboot | Adversarial Test K | ✅ after Test K |

**Why:** Confirms every recovery path has test coverage; identifies gaps in documentation or tests.

---

### 6. Exit Criteria
**Blocking failures** (tests that MUST pass):
- Any test where failure leaves user in unrecoverable state (lockout, data corruption)
- Any test validating a documented recovery path

**Non-blocking failures** (document as known issues):
- Timing tolerance exceeded (e.g., 2.5s instead of 2s) but feature still works
- Edge case that doesn't block primary workflow

**Log/telemetry criteria:**
- Expected event sequences appear in logs
- No unexpected errors or warnings (except those explicitly tested)

**Why:** Clear definition of "done"; separates critical failures from polish issues.

---

## Application to bluetooth-keyboard-app

**Prerequisites:**
- Surface Laptop 7 hardware (BLE stack, Modern Standby, SAM-bus keyboard)
- Lock-screen OSK rehearsal before Test 1 (muscle-memory for recovery)
- USB keyboard within reach

**Safety net:**
- USB keyboard plug test (validates hard-fallback before each test)
- Log truncation (isolates per-test diagnostics)
- Internal keyboard baseline (confirms not already broken)

**Smoke tests 1–9:**
- Launch, power-cycle Nuphy, toggle Active, lid close/open, sanity timer, Quit

**Adversarial tests 10–12:**
- Verify mismatch (SetupAPI lies), forced crash + reboot, hung instance + `--recover`

**Recovery drills:**
- Row 1 (tray) → Test 4
- Row 2 (`--recover`) → Test 12
- Row 3 (cold launch) → Test 11
- Row 4 (lock-screen OSK) → Tests 6, 11
- Row 5 (USB keyboard) → every safety net
- Row 6 (hard power-off) → manual rehearsal

**Exit criteria:**
- Blocking: Tests 6, 9, 11, 12 (lockout scenarios)
- Non-blocking: Test 8 sanity timer >25s (latency tolerance)
- Logs: ENABLE/DISABLE/verify sequences per PLAN.md §4.3

---

## Generalization for Other Projects

**Adapt this template when:**
- Tray app modifies system state (input devices, network, audio, display)
- Failure mode is "user can't interact with system" or "data loss"
- Recovery requires out-of-band paths (OSK, safe mode, USB device)

**Key elements to preserve:**
1. Pre-test recovery rehearsal (don't discover recovery path under duress)
2. Per-test safety net (detect environmental failure early)
3. Adversarial tests for every documented recovery path (confirm docs match reality)
4. Blocking vs. non-blocking failure classification (clear release gate)

**Key elements to adapt:**
- Hardware prereqs (match your target device)
- Recovery paths (OSK for input lockout, safe mode for driver issues, etc.)
- State persistence mechanism (registry, config file, driver settings)

---

## Anti-patterns to Avoid

❌ **No pre-test recovery rehearsal** → user discovers OSK doesn't work during lockout test (now locked out for real)
❌ **No per-test safety net** → USB keyboard breaks during Test 6, fail attributed to software (false negative)
❌ **Adversarial tests without reboot** → doesn't validate state persistence (missed critical bug)
❌ **Exit criteria are vague** → "mostly works" ships with lockout bugs
❌ **Recovery drills are documentation-only** → recovery path documented but never tested (may not work)

---

**Status:** Extracted from bluetooth-keyboard-app v0.1 test recipe (2026-04-21). Template refined for reuse on future safety-critical tray apps.
