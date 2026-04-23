# Drop-In Copy Blocks for README.md & ARCHITECTURE.md
> Ready for Elaine/Jerry to merge when code lands | v5.9 (2026-04-21)

These prose blocks are exact copy ready to paste into README.md and ARCHITECTURE.md. No editing needed; citations are literal.

---

## Block 1: Lock-Screen OSK Rehearsal Warning (README top banner)

**Where:** README.md, immediately after the one-liner, before any content.

```markdown
⚠️ **BEFORE FIRST USE: Rehearse lock-screen On-Screen Keyboard recovery (5 minutes).**

1. Lock your screen (Win+L).
2. At lock screen, move touchpad to **lower-right corner**.
3. Click **Accessibility icon** (wheelchair symbol).
4. Click **"On-Screen Keyboard"** in menu.
5. Type your PIN/password and sign back in.

**Why:** If the app crashes while keyboard is disabled and you lack a USB keyboard, OSK is your only recovery. Better to know it works before you need it. **If this rehearsal fails, do NOT use the app without a USB keyboard backup.**
```

---

## Block 2: SmartScreen & Unsigned Warning (README, after Install section)

**Where:** README.md, new section "SmartScreen warning" (see outline).

```markdown
## SmartScreen warning

On first run after a network download, Windows SmartScreen may warn: `"Windows protected your PC. Unknown publisher."` or `"This file may be unsafe to open."` This is expected (app is unsigned in v0.1; signing deferred to v0.2).

- **Click "More info" → "Run anyway"** to proceed.
- **Alternative:** Right-click the file → Properties → check **"Unblock"** → OK → run again (no "More info" dialog).

See [FUTURE.md](plan/FUTURE.md) for signing plans (v0.2: Azure Trusted Signing or OV certificate).
```

---

## Block 3: --recover Run Dialog Invocation (README, Recovery section row 2)

**Where:** README.md, [Recovery](#recovery) table, row 2 (Hung instance).

Exact wording for row 2 recovery, including `--recover` invocation via Run dialog:

```
| 2 | Hung instance — tray won't respond | **(1)** Press **Win+R**. Type `<full-path>\kbblock.exe --recover` → press Enter. (Requires touchpad or Nuphy to navigate Run dialog.) App unconditionally re-enables keyboard and exits. **(2)** Task Manager (Ctrl+Shift+Esc or Ctrl+Alt+Delete → Task Manager) → Processes → `kbblock.exe` → **End task**. (Leaving hung instance running allows it to re-disable after `--recover` finishes.) **Optional:** User can manually create a desktop shortcut with Target = `<full-path>\kbblock.exe --recover` for one-click recovery. **Note:** UAC prompt on Secure Desktop requires mouse/touchpad click (standard user without local admin cannot proceed via keyboard alone). |
```

---

## Block 4: USB Keyboard Hard Fallback Billboard (README, Recovery section row 5)

**Where:** README.md, [Recovery](#recovery) table, row 5 (universal fallback).

```
| 5 | Anywhere — universal fallback | **Plug USB keyboard.** Works on lock screen, BIOS, UEFI, BitLocker recovery, and WinRE — the only path that survives every failure mode. **Keep one in your bag.** |
```

**Additional callout (after Recovery table, before Troubleshooting):**

```markdown
**Pre-OS (BitLocker, UEFI, WinRE):** USB keyboard is the only universal path.
```

---

## Block 5: BLE Disconnect Latency Callout (README, Scope section)

**Where:** README.md, "Scope (what it is NOT)" bullet list, as 4th bullet.

```markdown
- **BLE disconnect has latency.** If the Nuphy disconnects uncleanly (battery dead, out of range), Windows BLE stack may take 10–30 seconds to notice (L2CAP supervision timeout). During that window, internal keyboard stays disabled. Tray "Active" toggle and `--recover` are the user-facing mitigations.
```

---

## Block 6: Crash Persistence Explanation (README, Scope section)

**Where:** README.md, "Scope (what it is NOT)" bullet list, as 3rd bullet.

```markdown
- **Crash persistence.** If app crashes while keyboard disabled, that state persists in Windows (`CONFIGFLAG_DISABLED` registry flag) until the app launches again on next boot and unconditionally re-enables. See [Recovery](#recovery) for the procedure.
```

---

## Block 7: SetupAPI Mechanism (ARCHITECTURE.md, core mechanism)

**Where:** ARCHITECTURE.md, "Mechanism (SetupAPI device disable)" section.

```markdown
## Mechanism (SetupAPI device disable)

`SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, DICS_DISABLE | DICS_ENABLE)` on the exact Surface internal keyboard PnP node. OS writes `CONFIGFLAG_DISABLED` to registry and unloads the driver. Device is fully dormant until re-enabled.

**Important:** `CONFIGFLAG_DISABLED` persists across reboot. Safety is NOT non-persistence; it is "every cold start unconditionally ENABLEs first" (§4.3 Behavior 1). If app crashes mid-disable, keyboard stays disabled until app launches again. Recovery procedure in README.md [Recovery](#recovery).
```

---

## Block 8: Three-Clause Predicate (ARCHITECTURE.md)

**Where:** ARCHITECTURE.md, "Target predicate (3 clauses, all must hold, resolved fresh on every action)" section.

```markdown
## Target predicate (3 clauses, all must hold, resolved fresh on every action)

Device is a valid disable target if and only if:
1. `Service == "kbdhid"`
2. `HardwareIds` contains substring `VID_045E&PID_006C`
3. `Parent` device path starts with `{2DEDC554-A829-42AB-90E9-E4E4B4772981}\Target_SAM`

**Match count check:** predicate must select **exactly one** device. Zero or multiple → refuse disable, fail closed, log full enumeration. Reduced from 7 clauses (v4) after dual-model review; VID/PID + SAM parent is unique on this hardware.
```

---

## Block 9: --recover Escape Hatch (ARCHITECTURE.md)

**Where:** ARCHITECTURE.md, under the Behavior 1 description or as a separate subsection.

```markdown
## --recover escape hatch

Separate argv path bypasses the single-instance mutex, calls `SetupDiCallClassInstaller(DICS_ENABLE)` inline on the calling thread (no worker, no message loop), verifies state, and exits 0 (success) or 1 (verify mismatch). Works even when a hung primary instance owns the mutex. Shares code path with Quit's inline fallback.

**Usage:** `kbblock.exe --recover` via Win+R, Explorer address bar, or desktop shortcut. Requires admin elevation (UAC), but runs outside the message-loop event serialization — safe escape hatch when primary instance is hung or crashed.
```

---

## Block 10: SAM Parent Durability Note (ARCHITECTURE.md, at end)

**Where:** ARCHITECTURE.md, "Why Surface SAM parent is durable (not ContainerId)" section.

```markdown
## Why Surface SAM parent is durable (not ContainerId)

Surface Laptop 7 internal devices (keyboard, touchpad, buttons) all report sentinel ContainerId `{00000000-0000-0000-FFFF-FFFFFFFFFFFF}` — meaning "no container info." Surface Aggregator Module (SAM) is a custom embedded controller that enumerates ACPI children; they don't inherit composite-device metadata.

**Implication:** Cannot use ContainerId matching. Must use HardwareId substring + Parent-path topology (3-clause predicate). **Never revert to ContainerId without re-testing on actual hardware.** See Spike 2 discovery log.
```

---

## Block 11: Tray Behavior & Nuphy Persistence (README, Troubleshooting)

**Where:** README.md, Troubleshooting section, new subsection "Nuphy paired but keyboard not disabling".

```markdown
### "Nuphy paired but keyboard not disabling"

1. Ensure Nuphy is on and within Bluetooth range.
2. In Windows Settings → Bluetooth, toggle Nuphy off, then back on (re-pair).
3. Restart the app. The app keeps internal keyboard enabled during the entire session until you successfully pair Nuphy; it does not cache pairing state from a prior boot.
4. If still not disabled: check `%LOCALAPPDATA%\kbblock\kbblock.log` for "Nuphy not connected" or SetupAPI errors.
```

**Key detail:** "must pair in Settings" + "app keeps internal enabled all session if not paired at launch" — this is the fail-safe behavior documented in PLAN.md §4.6.

---

## Block 12: Verify-Mismatch Handling (README, Troubleshooting)

**Where:** README.md, Troubleshooting section, new subsection "Verify mismatch".

```markdown
### "Verify mismatch: keyboard disable failed, Active toggled off"

The app tried to disable the keyboard but post-operation verification reported it still enabled. This can happen if:
- Windows PnP rebalance happened during the disable operation (rare).
- SetupAPI call succeeded but device didn't actually disable (driver issue, hardware problem).

**Recovery:** Right-click tray → check "Active" again. App will retry. If it fails repeatedly:
1. Check Device Manager (Ctrl+X, Device Manager) → Keyboards → right-click internal keyboard.
2. If it shows "Disabled," click "Enable" to restore it manually.
3. Restart the app and try again.
4. If the problem persists, file an issue with logs from `%LOCALAPPDATA%\kbblock\kbblock.log`.
```

---

## Summary of Blocks

| Block | Destination | Content |
|-------|-------------|---------|
| 1 | README, top | OSK rehearsal warning banner |
| 2 | README, Install | SmartScreen & unsigned warning |
| 3 | README, Recovery table | --recover Run dialog invocation (row 2) |
| 4 | README, Recovery table | USB hard fallback (row 5) |
| 5 | README, Scope | BLE disconnect latency callout |
| 6 | README, Scope | Crash persistence explanation |
| 7 | ARCHITECTURE | SetupAPI mechanism section |
| 8 | ARCHITECTURE | Three-clause predicate definition |
| 9 | ARCHITECTURE | --recover escape hatch subsection |
| 10 | ARCHITECTURE | SAM parent durability note |
| 11 | README, Troubleshooting | Nuphy pairing troubleshoot |
| 12 | README, Troubleshooting | Verify-mismatch handling |

---

**Status:** All blocks are v5.9-ready. Elaine/Jerry integrate as code lands.
