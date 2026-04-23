# Diagnosis — Login-screen keyboard dead + tray starts inactive

**Author:** Newman (Input Hooks Engineer)
**Date:** 2026-04-22
**Mode:** READ-ONLY (no source edits — Jerry is renaming in parallel)
**Reporter:** Brady

> "When rebooting, the internal keyboard did NOT work at the login screen.
>  Then after logging in, the app was running in the tray correctly but it was NOT in the 'active' state, and the internal keyboard WAS working."

---

## Background — what this app actually does to the keyboard

Despite my charter mentioning `WH_KEYBOARD_LL`, the codebase does **not** install a low-level keyboard hook. It uses **SetupAPI to disable the internal keyboard device at the Device Manager level** (`device::enable` / `device::disable` in `src/device.rs`). That's a critical fact for this bug, because:

- A device-disable **persists across reboot.** The driver state is written to the registry; rebooting does not undo it. If kbblock dies with the keyboard disabled, the keyboard is dead until something explicitly re-enables it.
- A WH_KEYBOARD_LL hook, by contrast, dies the moment the process dies. With this app there is no such "automatic safety net" — the only safety nets are (a) `shutdown_cleanup()` on process exit and (b) the boot-recovery scheduled task.

Two autostart mechanisms exist:

1. **`kbblock-logon`** — Task Scheduler logon trigger, runs as the interactive user with `RunLevel=HighestAvailable` (silently elevated). Defined in `src/autostart.rs`. This is what brings the tray up after login.
2. **`kbblock-boot-recover`** — Task Scheduler **boot trigger**, runs as `NT AUTHORITY\SYSTEM` (S-1-5-18) with `--recover` argument **before any user logs in.** Defined in `src/boot_task.rs:147-194`. This is the only thing that can rescue the keyboard at the login screen.

There is **no persisted "was active when shut down" state** anywhere in the codebase. `desired_active` is computed fresh at every launch from two inputs: (a) is `running.lock` present? and (b) are we elevated? See `src/main.rs:538-559`.

---

## Symptom 1: Internal keyboard dead at the login screen

### Root-cause hypothesis

Last session ended with the internal keyboard in the **Disabled** state at the Device Manager level, and the boot-recovery task that's supposed to rescue it before the login screen either (a) wasn't installed, (b) ran but failed to find the device, or (c) was installed pointing at a stale path.

Specifically:

- **kbblock disables the device when `desired_active=true && BT keyboard connected`** (`src/main.rs:1551-1597`, `apply_policy`).
- **The only thing that re-enables the device is** `shutdown_cleanup()` (`src/main.rs:156-199`), which runs on `WM_ENDSESSION`, `WM_CLOSE`, `WM_DESTROY`, panic, or Ctrl-C. If the process dies without any of those firing — hard power-off, hung Windows shutdown, OS-killed-during-suspend — the device stays disabled across the reboot.
- **The boot-recovery task is the only reliable rescue.** It's installed automatically on the first elevated launch (`src/main.rs:449-451, 768-801`) — but **only if the user has ever launched kbblock as admin and the install succeeded**. The marker file `%LOCALAPPDATA%\kbblock\lockout-protection-offered` is written *only on success* (line 787), so a transient COM/RPC failure leaves it unmarked and we retry next launch — that's good. But a user who installed kbblock and rebooted before ever launching it elevated would have nothing rescuing them.
- **Even when the boot-recovery task fires, `recover_mode()` (`src/main.rs:634-697`) has a critical gap:** it calls `device::resolve()` exactly once with no retry. If the HID class is not yet enumerated when the boot trigger fires (very plausible — BootTrigger fires extremely early, before user-mode services are fully up), `resolve()` returns `NoMatch` and the helper exits 1. The internal `device::enable()` call has a 500 ms retry but `resolve()` does not — see `recover_mode()` lines 639-656 vs lines 658-666.
- **Stale-path drift.** The boot-task XML stores an absolute path (`<Command>{path}</Command>`, `boot_task.rs:186`). If the user moved the EXE or reinstalled at a different location, the registered path becomes invalid and the SYSTEM-context task launches nothing. The tray UI shows a ⚠ warning when this happens (`src/main.rs:601-606, 2001-2070`), but the warning only appears *after* login — at the login screen there's no surface for it.

Most likely combination given Brady's environment (active developer, EXE has moved at least once during the project): **stale path** or **resolve race during early boot**.

### Confidence

**Medium.** I can identify the failure surfaces from the code with high confidence, but without log evidence from the actual reboot (`%LOCALAPPDATA%\kbblock\kbblock.log`, `phase.log`, and especially Event Viewer → Task Scheduler → kbblock-boot-recover history) I can't say *which* of the failure surfaces fired. Brady or George should pull those logs from the affected machine before we ship a fix.

### Proposed fix (high level — no code this round)

In priority order:

1. **Make `recover_mode()` resilient to early-boot race.** Wrap `device::resolve()` in a bounded retry loop (e.g., poll every 500 ms for up to 30 s) so the SYSTEM-context boot helper waits for HID enumeration. This is the cheapest, safest fix and addresses the most likely cause.
2. **Add a "boot-recover ran" telemetry trail.** Append a one-line timestamp to a separate `boot-recover.log` at the top and bottom of `recover_mode()` so we can prove from the log whether the task fired at all and what its outcome was. Right now if the task is silently broken, we have no evidence.
3. **Verify-and-self-heal stale paths.** When the user-context kbblock starts and detects a stale boot-task path, offer (or just silently do, if elevated) a re-register. Today we only warn — we don't fix.
4. **Add a fallback logon-trigger recovery.** If the boot trigger missed, have the logon-trigger autostart also do an unconditional ENABLE before tray init — which it already does at `src/main.rs:425-439` (cold-start unconditional enable). That part is working: it's exactly what re-enabled the keyboard for Brady after login.

### Could this be a leftover from a prior crash (Checkpoint 001 territory)?

**Yes, almost certainly the trigger.** Checkpoint 001 was about a shutdown-handler crash. The history file shows a long arc of fixing related issues: panic hook installed first, `ShutdownGuard` Drop, `SetProcessShutdownParameters(0x3FF, 0x0000_0001)`, top-level (not message-only) HWND so `WM_ENDSESSION` actually delivers, `WTS_SESSION_LOCK` + lid + display-state notifications. Each fix reduced the surface but none can cover **hard power-off** or **OS killing the process during a hung shutdown**, which are the two scenarios where `running.lock` survives. The boot-recovery task was added precisely for those scenarios — and it's the boot-recovery task that appears to have failed Brady this time.

---

## Symptom 2: App starts in inactive state when BT keyboard already connected

### Root-cause hypothesis

`src/main.rs:530-559` computes the initial tray state. The relevant logic, paraphrased:

```
crashed = running.lock_existed_at_startup
initial_desired_active =
    if crashed && is_elevated  → false  (and show "recovered from unclean shutdown" balloon)
    else if crashed && !is_elevated → false  (silent)
    else → true
```

When the previous shutdown was unclean (which is exactly what produced Symptom 1), `running.lock` survives in `%LOCALAPPDATA%\kbblock\` and the next launch enters this branch with `initial_desired_active = false`. The app comes up *intentionally* inactive as a safety stance — the design philosophy is "if I don't know what happened last time, don't immediately re-disable the keyboard the user just regained access to."

So Symptom 2 is **not a bug per se** — it's the documented safe-mode behavior firing, which Brady experienced as wrong because the precondition (BT keyboard already reconnected) made him expect active mode.

There are **no other suppressors** of `desired_active` on the cold-start path that I can find. The only other places `desired_active` is forced to false are inside the running message loop: worker death (`src/main.rs:1593`), Quit (`src/main.rs:1497`), suspend / lid / display-off paths (lines 1327, 1444). None of those fire before tray creation.

A second contributing factor worth flagging: **the "Active" toggle doesn't read BLE state.** Even if we kept `desired_active=true`, the initial `apply_policy()` runs after a 3-second grace timer (`INITIAL_POLICY_DELAY_MS`, line 72) — that grace was added (per history, Test 11) precisely because BLE often shows `Disconnected` in the first second after launch and the policy would re-disable the keyboard before the BT stack reconnected. So the existing 3 s grace already handles the timing race, but it does not handle the *crash-recovery* override.

### Confidence

**High.** The control flow is straightforward and the comment at `src/main.rs:546-548` (`"Click the tray icon → Active to re-arm blocking"`) explicitly describes the experienced behavior.

### Proposed fix (high level — no code this round)

Pick **one of (c) or (d)**, lean toward (c):

- **(a) Retry BT detection for N seconds** — already done, this is the 3 s `TIMER_INITIAL_POLICY` grace at `src/main.rs:608-612, 975-982`. Fine, but doesn't help here because the issue isn't BT timing, it's the crash override.
- **(b) Listen for BT connect events** — already done, `src/ble.rs` subscribes `ConnectionStatusChanged` and the message loop calls `apply_policy()` on each event. Also fine, but not the issue.
- **(c) ✅ Persist the user's last desired_active across clean shutdowns AND treat crash-recovery as a softer override.** Write `desired_active` to `%LOCALAPPDATA%\kbblock\state.json` whenever it changes (toggle, Quit, etc.). On startup:
  - Clean shutdown (no `running.lock`) → restore last persisted state. If `true`, app comes up active and user sees no change. If `false`, ditto. This fixes the *clean reboot* case completely.
  - Unclean shutdown (`running.lock` present) → still default to `false` for the safety-critical reason currently documented, BUT add a one-balloon prompt "BT keyboard is connected — Activate now?" with a 1-click yes button. Or auto-promote to `true` if BLE shows Connected within the 3 s grace AND the cold-start ENABLE verified successfully (i.e., we have full confidence the keyboard works). The latter is more invasive but more "it just works."
- **(d) Re-derive state from BLE.** Skip persistence entirely; if BT keyboard is connected within the grace window, set `desired_active=true`. Risk: any time the user wants to *temporarily* disable kbblock (e.g., loaning the laptop), the next launch overrides their intent. So (c) is safer.

My recommendation: **(c) — persist clean-shutdown state, and on crash-recovery offer a 1-click "BT keyboard detected, re-arm?" balloon.** This respects the existing safety stance for genuine crashes while eliminating the false positive on every reboot.

---

## Linked or independent?

**One bug with two visible faces. Single root cause: the previous shutdown was unclean.**

- The unclean shutdown left the device disabled at the driver level → boot-recovery task should have rescued it but didn't (Symptom 1).
- The unclean shutdown left `running.lock` on disk → next launch entered crash-safe mode → tray came up inactive (Symptom 2).

They are linked at the *trigger* level (one shutdown fault produces both) but the *fixes* are independent — fixing the boot-recovery race won't fix the tray-state UX, and persisting state won't bring the keyboard back at the login screen. We need to fix both.

There is also a third silent face of the same bug that Brady didn't mention but is worth naming: **even when the boot-recovery task succeeds, the tray will still come up inactive on the next login** (because `running.lock` is created/deleted inside the user's `%LOCALAPPDATA%`, which the SYSTEM-context boot task neither reads nor writes). So fixing only Symptom 1 leaves Symptom 2 reproducible on every dirty reboot.

---

## Recommended next steps

### Tests George could write

1. **Reproduce-the-bug integration test (manual, on real hardware):**
   - Launch kbblock elevated. Connect Nuphy. Verify tray shows Active and internal keyboard is disabled in Device Manager.
   - `Stop-Process -Id <kbblock pid> -Force` (simulates hard crash; no `WM_ENDSESSION`).
   - Verify `%LOCALAPPDATA%\kbblock\running.lock` exists and Device Manager still shows internal keyboard disabled.
   - Reboot.
   - **Test A (Symptom 1):** at the login screen, attempt to type with the internal keyboard. Should work. Then check Event Viewer → Task Scheduler → `kbblock-boot-recover` History to confirm task ran and exited 0.
   - **Test B (Symptom 2):** log in, wait for tray. Verify state.

2. **Unit/headless tests (no hardware needed):**
   - `recover_mode()` resolve-retry loop: mock `device::resolve()` to return `NoMatch` 3 times then `Ok` — assert helper exits 0 after retrying, not 1 immediately.
   - State persistence roundtrip: write `desired_active=true` to state file, simulate launch, assert restored.
   - Crash-recovery promotion logic: with `running.lock` present + BLE Connected within grace window + cold-start ENABLE verified, assert `desired_active` ends up `true` (if we adopt the auto-promote path) or that the prompt-balloon code is invoked (if we adopt the explicit-prompt path).

3. **Boot-task health check:** new test (or just CLI subcommand) that compares `boot_task::registered_path()` against `current_exe()` and exits 1 if stale. Could be wired into the tray's existing stale-path detector.

### Order of fixes

1. **First: Symptom 1, fix #1 (resolve retry in `recover_mode`).** This is safety-critical — without it, the user can't log in. Smallest, safest change. Plus fix #2 (boot-recover.log) so we can verify it worked next time it matters.
2. **Second: Symptom 2 fix (c).** Pure UX, but high frequency once anyone has had one bad shutdown.
3. **Third: stale-path self-heal.** Lower priority; the warning UI Brady already has is acceptable in the meantime.

Defer until after Jerry's rename completes. None of these need to land in this round.
