# kbblock Test Architecture

**One-line summary:** Automate ~60% of George's validation matrix via `cargo test`; ~40% stays manual because it requires real BLE hardware pairing, real SetupAPI on a live kernel device, real shutdown/reboot behavior, and lock-screen input verification.

---

## 1. Pure-Logic Units (The Easy Wins)

These functions have zero Win32 dependency or can be tested with trivial inputs. All live behind `fn` (private), so tests go in `#[cfg(test)] mod tests` blocks inside each file — standard Rust pattern.

### autostart.rs (4 functions)

| Function | Test Name | Assertions |
|----------|-----------|------------|
| `xml_escape()` | `test_xml_escape_basic` | `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`, `"` → `&quot;` |
| `xml_escape()` | `test_xml_escape_passthrough` | Plain ASCII string unchanged |
| `xml_escape()` | `test_xml_escape_combined` | `<"foo" & bar>` produces correct output |
| `strip_verbatim()` | `test_strip_verbatim_present` | `\\?\C:\foo` → `C:\foo` |
| `strip_verbatim()` | `test_strip_verbatim_absent` | `C:\foo` → `C:\foo` (no-op) |
| `extract_command()` | `test_extract_command_valid` | Extracts `C:\path\kbblock.exe` from well-formed XML |
| `extract_command()` | `test_extract_command_escaped` | Handles `&amp;`, `&lt;`, `&quot;` in command path |
| `extract_command()` | `test_extract_command_missing` | Returns `None` when `<Command>` tag absent |
| `extract_command()` | `test_extract_command_empty` | Returns `Some("")` for `<Command></Command>` |
| `build_logon_task_xml()` | `test_logon_task_xml_structure` | Contains `<LogonTrigger>`, `<UserId>`, `<RunLevel>HighestAvailable`, `<Command>` with path |
| `build_logon_task_xml()` | `test_logon_task_xml_escaping` | Username with `&` is escaped; path with `<` is escaped |
| `build_logon_task_xml()` | `test_logon_task_xml_roundtrip` | `extract_command(build_logon_task_xml(path, user)) == Some(path)` |

### boot_task.rs (4 functions — same signatures, different XML shape)

| Function | Test Name | Assertions |
|----------|-----------|------------|
| `xml_escape()` | `test_xml_escape_*` | Same battery as autostart (code is duplicated; tests confirm both copies) |
| `strip_verbatim()` | `test_strip_verbatim_*` | Same as autostart |
| `extract_command()` | `test_extract_command_*` | Same battery |
| `build_task_xml()` | `test_boot_task_xml_structure` | Contains `<BootTrigger>`, `<UserId>S-1-5-18`, `<Arguments>--recover</Arguments>`, `<ExecutionTimeLimit>PT2M` |
| `build_task_xml()` | `test_boot_task_xml_roundtrip` | `extract_command(build_task_xml(path)) == Some(path)` |
| `build_task_xml()` | `test_boot_task_xml_escaping` | Path with special chars is XML-safe |

### device.rs (predicate logic)

| Function | Test Name | Assertions |
|----------|-----------|------------|
| `matches()` | `test_matches_exact_target` | Returns `true` for `service="kbdhid"`, hwid containing `VID_045E&PID_006C`, parent starting with SAM GUID |
| `matches()` | `test_matches_wrong_service` | `service="i8042prt"` → `false` |
| `matches()` | `test_matches_wrong_vid_pid` | hwid `VID_045E&PID_0000` → `false` |
| `matches()` | `test_matches_wrong_parent` | parent `USB\ROOT_HUB` → `false` |
| `matches()` | `test_matches_empty_hardware_ids` | Empty hwid vec → `false` |
| `matches()` | `test_matches_case_insensitive_parent` | SAM GUID in lowercase → `true` (code does `.to_uppercase()`) |
| `matches()` | `test_matches_multiple_hwids` | Multiple hwids, one matching → `true` |
| `matches()` | `test_matches_partial_vid_pid` | `VID_045E` without `PID_006C` → `false` |

**Note:** `CandidateInfo` is currently `struct` (private). Tests in `#[cfg(test)] mod tests` inside device.rs can access it directly. No visibility change needed.

### main.rs (pure helpers)

| Function | Test Name | Assertions |
|----------|-----------|------------|
| `get_running_lock_path()` | `test_running_lock_path_format` | Contains `kbblock` and `running.lock`; tests with `LOCALAPPDATA` set via temp env |
| `get_lockout_offered_marker_path()` | `test_lockout_marker_path_format` | Contains `kbblock` and `lockout-protection-offered` |
| `paths_equal()` | `test_paths_equal_same` | `C:\foo\bar` == `C:\foo\bar` |
| `paths_equal()` | `test_paths_equal_case_insensitive` | `C:\Foo\Bar` == `c:\foo\bar` |
| `paths_equal()` | `test_paths_equal_different` | `C:\foo` != `C:\bar` |
| `parse_admin_subcommand()` | Difficult to test (reads `env::args()`); **defer to tier (b)** — extract to `parse_admin_subcommand_from(args: &[String])` |

### ble.rs

| Function | Test Name | Assertions |
|----------|-----------|------------|
| `BleError::Display` | `test_ble_error_display_not_paired` | Contains BD_ADDR hex string |
| `BleError::Display` | `test_ble_error_display_winrt` | Contains API name and HRESULT |

**Note:** `status_str()` uses `BluetoothConnectionStatus` enum from windows-rs. Testable only when `windows` crate compiles (it does on the host), but runs fine without hardware.

---

## 2. Win32-Touching Code That Needs Abstraction

### device.rs — `KeyboardControl` trait

Current problem: `resolve()`, `enable()`, `disable()`, `current_state()` all call SetupAPI inline. Can't test the *caller's* logic (apply_policy) without real hardware.

**Proposed seam (minimal):**

```rust
pub trait KeyboardControl {
    fn resolve(&self) -> ResolveResult;
    fn enable(&self, target: &Target) -> Result<EnableOutcome, DeviceError>;
    fn disable(&self, target: &Target) -> Result<(), DeviceError>;
    fn current_state(&self, target: &Target) -> Result<KeyboardState, DeviceError>;
    fn disable_and_verify(&self, target: &Target) -> (Result<(), DeviceError>, Result<KeyboardState, DeviceError>);
}

pub struct SetupApiKeyboard; // Real impl — delegates to existing functions
pub struct MockKeyboard { ... } // Test impl — tracks calls, returns configured results
```

**Scope:** Only the trait definition + `SetupApiKeyboard` impl wrapper. The existing free functions stay. `MockKeyboard` lives in `#[cfg(test)]`.

**NOT doing:** Injecting the trait into every callsite in main.rs right now. That's tier (b) — when we extract `AppCore`.

### ble.rs — `BleMonitor` trait

```rust
pub trait BleMonitor {
    fn is_connected(&self) -> bool;
}
```

`BleHandle` already implements `is_connected()`. The trait lets `AppState` hold `Box<dyn BleMonitor>` instead of `Option<BleHandle>` — enabling test doubles. Tier (b).

### autostart.rs / boot_task.rs — No trait needed

The Win32 parts (`with_folder`, `get_current_username`, `delete_legacy_run_key`) are COM plumbing. The *testable* surface is the XML generation and parsing, which is already pure. The COM operations themselves (register/delete task) are integration-level and tested manually. No seam needed.

---

## 3. State-Machine and Event-Handling Tests

### Current obstacle

`apply_policy()` and all `handle_*` functions take `&mut AppState`, which contains `HWND`, `TrayIcon`, `CheckMenuItem`, `Sender<Cmd>` — deeply coupled to Win32 runtime.

### Proposed extraction: `PolicyEngine`

Extract the *decision logic* from `apply_policy()` into a pure function:

```rust
pub enum PolicyDecision {
    Enable,
    Disable(Target),
    NoOp, // worker_dead and not desired_active
}

pub fn decide_policy(
    desired_active: bool,
    nuphy_connected: bool,
    worker_dead: bool,
    resolve_result: ResolveResult,
) -> PolicyDecision { ... }
```

This is the §4.4 pseudocode in its purest form. Tests drive it without HWND/tray/channel.

### Event classes and test cases

| Event Class | Cases |
|-------------|-------|
| **BLE connect/disconnect** | (1) Nuphy connects, desired_active=true → Disable. (2) Nuphy disconnects, desired_active=true → Enable. (3) Nuphy connects, desired_active=false → Enable (not NoOp — must actively enable). (4) Nuphy connects, worker_dead=true → refuse Disable, inline Enable. |
| **Tray toggle** | (5) Toggle from active to inactive → Enable. (6) Toggle from inactive to active, Nuphy connected → Disable. (7) Toggle from inactive to active, Nuphy disconnected → Enable. (8) Toggle increments `current_generation`. |
| **Power/Session** | (9) PBT_APMSUSPEND → Enable + resume_pending=true. (10) PBT_APMRESUMEAUTOMATIC → Enable + resume_pending=true, no apply_policy. (11) WTS_SESSION_UNLOCK with resume_pending → clear + apply_policy. (12) WTS_SESSION_UNLOCK without resume_pending → no-op. (13) WTS_SESSION_LOCK → Enable + resume_pending=true. |
| **Lid/Display** | (14) Lid closed → Enable + resume_pending. (15) Display off → Enable + resume_pending. |
| **Shutdown** | (16) WM_QUERYENDSESSION → return TRUE (allow). (17) WM_ENDSESSION wparam=true → shutdown_cleanup. (18) WM_ENDSESSION wparam=false → no-op. |
| **Quit** | (19) Quit → desired_active=false, Shutdown sent, join, inline Enable. (20) Quit with worker timeout → still inline Enable. |
| **Sanity timer** | (21) Worker is_finished=true → worker_dead, desired_active=false, inline Enable. (22) resume_pending > 2 min → clear. (23) Session active, !resume_pending → apply_policy. |
| **Initial policy timer** | (24) 3s timer fires → apply_policy. (25) BLE event before 3s → cancel timer, apply_policy immediately. (26) Tray toggle before 3s → cancel timer. |
| **Worker result (WM_APP+3)** | (27) op_id < current_generation → ignore (stale). (28) verify=Disabled → success, update tooltip. (29) verify=Enabled → mismatch recovery: Enable + desired_active=false. (30) verify=None (error) → Enable. |
| **Predicate failures** | (31) resolve returns NoMatch → Enable (fail-closed). (32) resolve returns MultipleMatches → Enable. (33) resolve returns EnumerationError → Enable. |
| **Crash detection** | (34) running.lock exists at start → desired_active=false. (35) running.lock absent → desired_active=true. |

**Tier (b):** Implement `PolicyEngine` extraction + tests for cases 1-8, 31-33 (pure decide_policy). Cases 9-30 require simulated AppState; defer to tier (b) or (c).

---

## 4. Property-Based Testing (proptest)

### Invariants worth checking

| Invariant | Generator | Property |
|-----------|-----------|----------|
| **I3: Fail-closed predicate** | Random sequences of `CandidateInfo` with varying service/hwid/parent | `matches()` returns `true` iff all 3 clauses hold; never `true` on partial match |
| **I9: Stale immunity** | Random `(op_id, current_generation)` pairs | Result ignored iff `op_id < current_generation` |
| **Policy always fails safe** | Random `(desired_active: bool, nuphy_connected: bool, resolve_result: ResolveResult)` | If any input is "unsafe" (not connected, predicate fails, not active), decision is Enable — never Disable |
| **XML roundtrip** | Random ASCII strings for path/username (with special chars &<>") | `extract_command(build_*_xml(path)) == Some(path)` |
| **xml_escape idempotence** | Random strings | `xml_escape` result contains no unescaped `&`, `<`, `>`, `"` |

**Recommendation:** Add `proptest = "1"` as dev-dependency. Start with the predicate invariant and XML roundtrip — highest ROI for property testing. **Tier (b).**

---

## 5. Snapshot Tests (insta)

### Candidates for pinning

| Artifact | Why snapshot |
|----------|-------------|
| `build_logon_task_xml("C:\\kbblock.exe", "testuser")` | Pin XML structure; detect accidental changes to task settings (priority, execution limits, hidden flag). Catches regressions in the ~40-line XML template. |
| `build_task_xml("C:\\kbblock.exe")` | Same rationale for boot-recovery task. Different trigger (Boot vs Logon), different principal (SYSTEM vs user). |
| Tooltip format strings | Pin `"kbblock \| Active: On \| Nuphy: Connected"` format. Catch accidental tooltip regressions. |

**Recommendation:** Add `insta = "1"` as dev-dependency. 3 snapshot tests total. **Tier (a)** — they're trivial to write and protect the XML templates that talk to Task Scheduler.

---

## 6. Doctests

### Candidates

| Function | Doctest value |
|----------|--------------|
| `xml_escape()` | Show the 4 substitutions as a runnable example. Doubles as documentation for the XML generation contract. |
| `extract_command()` | Show extract from a minimal `<Command>path</Command>` snippet. |
| `strip_verbatim()` | Show the `\\?\` prefix removal. |
| `matches()` (if made `pub(crate)`) | Show the 3-clause predicate with a fixture `CandidateInfo`. |

**Tier (b):** Low priority. The `#[cfg(test)]` unit tests cover the same ground. Doctests add value only when functions become `pub` API.

---

## 7. Integration Tests in `tests/`

### Proposed test binaries (no real hardware)

| Test binary | Scenario | Assertions |
|-------------|----------|------------|
| `tests/crash_detection.rs` | Write `running.lock` to a temp dir → simulate "next launch" logic → verify `initial_desired_active == false` | Crash detection round-trip works correctly. Requires extracting `check_running_lock` + `create_running_lock` + `delete_running_lock` to accept a path parameter (currently hardcoded to `%LOCALAPPDATA%`). **Tier (b).** |
| `tests/xml_roundtrip.rs` | Generate both task XMLs with edge-case paths (Unicode, spaces, `&`, long paths) → extract_command → verify round-trip | Cross-module XML generation + parsing integrity. **Tier (a)** — can be written today using pub functions + module tests. |

**Not proposing:** Integration tests that spin up the full `AppState` with mock everything. That's over-engineered for a single-user tray app. The unit tests on `decide_policy` + individual handler tests cover the same ground with less plumbing.

---

## 8. Build-Time Gates

### Proposed Cargo.toml additions

```toml
[lints.rust]
unsafe_op_in_unsafe_fn = "warn"

[lints.clippy]
all = "warn"
pedantic = { level = "warn", priority = -1 }
# Suppress noisy pedantic lints that don't add value here:
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
must_use_candidate = "allow"
cast_possible_truncation = "allow"
cast_sign_loss = "allow"
cast_possible_wrap = "allow"
```

### Tooling commands (run before/during build)

| Tool | Command | Purpose |
|------|---------|---------|
| `cargo fmt` | `cargo fmt --check` | Enforce consistent formatting. Fail on diff. |
| `cargo clippy` | `cargo clippy -- -D warnings` | Catch common bugs, unused code, style issues. |
| `cargo deny` | `cargo deny check licenses advisories` | License audit + security advisory check on deps. Requires `deny.toml` config. **Tier (b).** |
| `cargo udeps` | `cargo +nightly udeps` | Detect unused dependencies. **Tier (c)** — requires nightly, low ROI with only 4 deps. |

### Immediate (tier a)

Add `cargo fmt --check` and `cargo clippy -- -D warnings` to the build script. Defer `cargo deny` and `cargo udeps`.

---

## 9. CI Integration — `scripts/build.ps1` Diff

Current `build.ps1` runs Docker build → copies binary. Tests should run *inside* the same container, *before* the binary copy.

### Problem: cross-compilation

The Docker build uses `cargo-xwin` to cross-compile for `aarch64-pc-windows-msvc` from a Linux container. `cargo test` cannot *run* ARM64 Windows binaries in a Linux container. Tests must be compiled for and run on the **host** (or a Windows container).

### Proposed approach

**Option A (recommended): Host-side test gate.** Run `cargo test`, `cargo fmt --check`, and `cargo clippy` on the Windows host *before* invoking the Docker cross-build. This is the pragmatic path — tests are fast (pure logic, no I/O), and the host already has the Rust toolchain.

```powershell
# scripts/build.ps1 — proposed diff
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot | Split-Path -Parent

# ── Pre-build quality gates (host-side) ──
Write-Host "=== Running cargo fmt --check ===" -ForegroundColor Cyan
cargo fmt --check
if ($LASTEXITCODE -ne 0) { throw "cargo fmt check failed" }

Write-Host "=== Running cargo clippy ===" -ForegroundColor Cyan
cargo clippy -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed" }

Write-Host "=== Running cargo test ===" -ForegroundColor Cyan
cargo test --lib
if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

# ── Cross-compile in Docker (existing) ──
Write-Host "=== Building ARM64 binary in Docker ===" -ForegroundColor Cyan
docker build -t kbblock:build -f $root/docker/Dockerfile.build $root
$cwd = $root -replace '\\','/'
docker run --rm `
  -v "${cwd}:/build" `
  -v "${cwd}/output:/output" `
  -v "${cwd}/.xwin-cache:/xwin-cache" `
  kbblock:build
Write-Host "Built: $root\output\kbblock.exe"
```

**Key detail:** `cargo test --lib` runs only unit tests inside `src/`, not integration tests in `tests/`. This is fast and requires no special setup. Integration tests (`cargo test --test`) can be added later.

**Option B (deferred): In-container testing.** Would require a Windows container image with Rust + windows-rs. Heavy. Not worth it for pure-logic tests that can run on the host.

---

## 10. Things We Genuinely Cannot Automate

These stay in George's manual matrix. No amount of mocking helps.

| Surface | Why manual | George's test # |
|---------|-----------|-----------------|
| **Real BLE pairing + connection** | Requires physical Nuphy Air75 powered on/off within Bluetooth range of the Surface. `FromBluetoothAddressAsync` against a real BLE stack. | T2 (BLE toggle) |
| **Real SetupAPI disable/enable** | `write_config_flag` + `trigger_reeval` on the actual internal keyboard device. The kernel PnP subsystem, driver unload, and CONFIGFLAG_DISABLED persistence are all OS-level behaviors. | T1 (cold start), T3 (disable/enable) |
| **Lock screen keyboard availability** | "Can I still type my password with the Nuphy when the internal keyboard is disabled?" requires sitting at the lock screen with a physical keyboard. | T5 (resume gating) |
| **Shutdown/reboot persistence** | `CONFIGFLAG_DISABLED` survives reboot. Verifying cold-start ENABLE on boot requires actual reboot. | T6 (shutdown), T10 (boot recovery) |
| **Suspend/resume/lid close** | Modern Standby behavior (Connected Standby on Snapdragon) is hardware-specific. Mock PBT_APMSUSPEND doesn't test the actual power transition. | T4 (power transitions) |
| **UAC consent flow** | `ShellExecuteExW("runas")` prompts a real UAC dialog. Can't simulate in tests. | T8 (boot task toggle) |
| **Tray icon visual state** | Tooltip text, menu checkmarks, and balloon notifications render in the real Windows shell. | T12 (tooltip truth) |
| **Multi-instance mutex** | `CreateMutexW("Local\\kbblock-singleton-v1")` behavior across actual Windows sessions. | T11 (single instance) |
| **--recover escape hatch** | Must work when primary instance is *actually hung* (owns mutex). | T9 (--recover) |

**Floor:** 9 manual test areas. These are the irreducible hardware/OS surface. Everything else gets automated.

---

## 11. Quick-Win Sequencing

### Tier (a): Land tomorrow, biggest ROI (1 focused session, ~2-3 hours)

1. **Add `#[cfg(test)] mod tests` to `autostart.rs`** — 12 tests covering `xml_escape`, `strip_verbatim`, `extract_command`, `build_logon_task_xml` (including roundtrip).

2. **Add `#[cfg(test)] mod tests` to `boot_task.rs`** — 10 tests covering same functions for the boot-recovery XML variant.

3. **Add `#[cfg(test)] mod tests` to `device.rs`** — 8 tests covering `matches()` predicate with fixture `CandidateInfo` values (exact target, wrong service, wrong VID/PID, wrong parent, case sensitivity, empty hwids).

4. **Add `insta` dev-dependency + 2 snapshot tests** — Pin the exact XML output of both `build_logon_task_xml` and `build_task_xml`. Catches accidental changes to task scheduler config.

5. **Add `cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test --lib` to `scripts/build.ps1`** — Gate the build on test passage.

6. **Add basic `paths_equal` tests in main.rs** — 3 tests for case-insensitive path comparison.

**What this catches:** XML generation regressions, predicate logic bugs (wrong VID/PID, wrong SAM prefix), formatting errors, path comparison bugs. These are the classes of bugs that would otherwise require a full deploy → reboot → observe cycle to discover.

**Estimated test count:** ~35 tests.

### Tier (b): Nice to have once (a) is in (~1-2 sessions)

7. Extract `decide_policy()` pure function from `apply_policy()` → 8 tests covering all decision branches.

8. Add `KeyboardControl` trait to device.rs (+ `SetupApiKeyboard` wrapper). No callers changed yet.

9. Add `BleMonitor` trait to ble.rs.

10. Add `proptest` dev-dependency + property tests for predicate invariant and XML roundtrip.

11. Extract `parse_admin_subcommand_from(args: &[String])` → unit tests.

12. Extract `check_running_lock` / `create_running_lock` / `delete_running_lock` to accept path parameter → `tests/crash_detection.rs` integration test.

13. Add `cargo deny` with `deny.toml` for license + advisory audit.

14. Tooltip string format tests (may use `insta` snapshot).

### Tier (c): Consider only if a regression bites us

15. Full `AppCore` extraction (AppState without Win32 types) for end-to-end state machine simulation.

16. Property-based testing of event sequences (proptest state machine).

17. `cargo udeps` (requires nightly; only 4 deps).

18. In-Docker test execution (Windows container image).

19. Code coverage tracking (tarpaulin or llvm-cov).

---

## TODO Checklist — Tier (a) Implementation

Assignable to Newman/Kramer/George. Each item is self-contained.

- [ ] **Newman:** Add `[dev-dependencies] insta = "1"` to `Cargo.toml`
- [ ] **Newman:** Add `#[cfg(test)] mod tests` to `src/autostart.rs` with 12 tests (xml_escape ×3, strip_verbatim ×2, extract_command ×4, build_logon_task_xml ×2, roundtrip ×1)
- [ ] **Newman:** Add `#[cfg(test)] mod tests` to `src/boot_task.rs` with 10 tests (same functions, boot-recovery variant)
- [ ] **Newman:** Add `#[cfg(test)] mod tests` to `src/device.rs` with 8 tests for `matches()` predicate
- [ ] **Kramer:** Add 2 `insta` snapshot tests for task XML outputs (in autostart.rs and boot_task.rs test modules)
- [ ] **Kramer:** Add `paths_equal` tests in `src/main.rs` `#[cfg(test)] mod tests` (3 tests)
- [ ] **Kramer:** Add `BleError::Display` tests in `src/ble.rs` `#[cfg(test)] mod tests` (2 tests)
- [ ] **George:** Update `scripts/build.ps1` with pre-build quality gates (`cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --lib`)
- [ ] **George:** Run `cargo test --lib` end-to-end to verify all tests pass in the host environment before declaring tier (a) complete
- [ ] **Newman:** Run `cargo insta review` to accept initial snapshots

**Total tier (a):** ~35 tests, 3 build gates, ~2-3 hours of focused work.
