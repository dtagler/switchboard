<h1 align="center">
  <img src="assets/icons/switchboard-dark.png" alt="SwitchBoard" width="128" height="128">
</h1>

<p align="center">
  <img src="assets/switchboard-logo.svg" alt="Switchboard" width="880"><br>
  ⌨️ Auto-disable laptop keyboard · 🎯 Connect Bluetooth keyboard
</p>

<p align="center">
  <em>Tray app that disables your Surface Laptop 7's built-in keyboard when your Nuphy Air75 (Bluetooth) connects, and restores it on disconnect, suspend, or manual toggle.</em>
</p>

<p align="center">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2011%20ARM64-0078D6?logo=windows11&logoColor=white">
  <img alt="Hardware" src="https://img.shields.io/badge/hardware-Surface%20Laptop%207-000000?logo=microsoft&logoColor=white">
  <img alt="Language" src="https://img.shields.io/badge/built%20with-Rust-CE422B?logo=rust&logoColor=white">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-brightgreen">
  <img alt="Status" src="https://img.shields.io/badge/status-v0.1%20unsigned-orange">
</p>

---
---

⚠️ **BEFORE FIRST USE: Rehearse lock-screen On-Screen Keyboard recovery (5 minutes).**

1. Lock your screen (Win+L).
2. At lock screen, move touchpad to **lower-right corner**.
3. Click **Accessibility icon** (wheelchair symbol).
4. Click **"On-Screen Keyboard"** in menu.
5. Type your PIN/password and sign back in.

**Why:** If the app crashes while the keyboard is disabled and you lack a USB keyboard, OSK is your only recovery. Better to know it works before you need it. **If this rehearsal fails, do NOT deploy the app without a USB keyboard backup.**

---

## What it does

Tray app that disables the Surface Laptop 7 internal keyboard when the Nuphy Air75 (Bluetooth LE) connects, and re-enables it on disconnect, suspend, lid-close, shutdown, logoff, and when you toggle **Active** off via tray menu.

## Scope (what it is NOT)

- **Single-user only.** RDP, Fast User Switch not supported.
- **ARM64 Surface Laptop 7 only.** Not tested on other hardware.
- **Pre-OS scenarios unaddressable.** BitLocker recovery, UEFI, WinRE — keep a USB keyboard.
- **BLE disconnect latency.** Windows BLE stack may take 10–30 seconds to notice if Nuphy dies. Toggle **Active** off or run `--recover` to speed recovery.
- **No "Escape panic."** Disabling kbdhid removes the device from the input stack — no Escape from internal keyboard possible. Recover via tray, touchpad + OSK, or USB keyboard.
- **Crash persistence.** If the app crashes while keyboard disabled, state persists in Windows (`CONFIGFLAG_DISABLED` registry flag) until the app launches again and unconditionally re-enables. See [Recovery](#recovery).

## Requirements

- **Hardware:** Surface Laptop 7 (15", Snapdragon X Elite, ARM64 Windows).
- **OS:** Windows 11 ARM64.
- **Keyboard:** Nuphy Air75 V3 paired in Windows Settings → Bluetooth.
- **Permissions:** Admin elevation (UAC prompt).

## Install

1. Unzip `switchboard.exe` (single ~390 KB portable exe; size is approximate and profile-dependent).
2. Run `switchboard.exe`. UAC prompt → **click "Yes"**.
3. Tray icon appears (keyboard symbol, lower-right). **Done.**

### Configure your keyboard's Bluetooth address

SwitchBoard targets one specific Bluetooth keyboard by its MAC address, supplied at runtime. Without this, the BLE monitor stays disabled and the internal keyboard is left enabled (fail-open).

1. Find your keyboard's MAC: **Settings → Bluetooth & devices → [your Nuphy] → Properties → Bluetooth address**.
2. Set it via **either** option:
   - **`.env` file** next to `switchboard.exe` (or in the working directory). Copy `.env.example` to `.env` and fill in:
     ```
     SWITCHBOARD_NUPHY_BD_ADDR=AA:BB:CC:DD:EE:FF
     ```
   - **Environment variable** `SWITCHBOARD_NUPHY_BD_ADDR` (takes precedence over the `.env` file).

Accepted formats (case-insensitive): `0xAABBCCDDEEFF`, `AABBCCDDEEFF`, `AA:BB:CC:DD:EE:FF`, `AA-BB-CC-DD-EE-FF`.

> **Why runtime config?** A Bluetooth MAC is owner-specific PII. Keeping it out of the source tree (and out of any shipped binary) means the same build works for every owner and nothing identifying ever ends up in version control. `.env` is gitignored.

To uninstall cleanly: first uncheck **Auto-start SwitchBoard at login** and **Lockout protection (recommended)** in the tray menu (both leave behind a Task Scheduler entry — `switchboard-logon` and `switchboard-boot-recover` respectively), then delete the exe. If you never enabled either, you can just delete the exe.

> **Console window:** Release builds suppress the console window, so launching from Explorer or autostart shows only the tray icon. Debug builds show the console for development. Terminal invocations (e.g., `switchboard.exe --recover` from PowerShell) still display output.

## Build from source

```powershell
# Windows (Docker must be installed)
.\scripts\build.ps1
# Produces .\dist\switchboard.exe (approximately 390 KB)

# Or macOS / Linux
./scripts/build.sh
```

The build script uses Docker with `cargo-xwin` cross-compiler (Rust 1.90, aarch64-pc-windows-msvc target).

## SmartScreen warning

On first network download, Windows may show: `"Windows protected your PC. Unknown publisher."` or `"This file may be unsafe to open."` Expected (the binary is unsigned).

- **Click "More info" → "Run anyway".**
- **Or:** Right-click file → Properties → check **"Unblock"** → OK → run again.

## Daily use

- Nuphy connects → tray icon changes → internal keyboard disabled (verified within 2s).
- Nuphy disconnects or battery dies → internal keyboard re-enabled within seconds (or 10–30s if BLE is slow to notice).
- Right-click tray icon → uncheck **"Active"** → internal keyboard works regardless of Nuphy.
- Right-click tray icon → check **"Active"** → resume auto-disable.
- **Tray icon auto-themes:** Tray icon swaps between dark and light variants to match your Windows theme. Dark on default dark theme (Win 11), light when you switch to light theme — no user action needed.

### Tray menu

| Item                                      | Effect                                                                                                                                                  |
|-------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Active**                                | Master toggle for the auto-disable policy. Unchecked = internal keyboard always on, regardless of Nuphy state.                                          |
| **Auto-start SwitchBoard at login**       | Per-user autostart via Task Scheduler logon task (`switchboard-logon`). Uses `RunLevel=HighestAvailable` to pre-elevate the token at logon, so the tray comes up silently despite the `requireAdministrator` manifest. Toggling on/off triggers a UAC prompt (one-time per toggle). |
| **Lockout protection (recommended)**      | Registers a SYSTEM-level scheduled task that runs `switchboard.exe --recover` at every boot. Toggling on/off triggers a UAC prompt (one-time per toggle). |
| **Quit**                                  | Clean exit. Re-enables the internal keyboard before the process terminates (fail-open invariant).                                                       |

**Stale-path detection:** if either toggle is checked but points at a different `switchboard.exe` than the one currently running (e.g. you moved or replaced the binary), switchboard pops a non-blocking warning dialog at startup. Uncheck and re-check the toggle to repoint it at the current EXE.

The **Auto-start SwitchBoard at login** state is stored as a Task Scheduler logon task named `switchboard-logon` (LogonTrigger scoped to your user, `RunLevel=HighestAvailable`). Inspect manually:

```powershell
schtasks /query /tn switchboard-logon /v /fo list
```

## Lockout protection / boot recovery task (optional, strongly recommended)

**Problem:** If `switchboard.exe` fails to launch at logon for any reason (uninstall, antivirus quarantine, missing dependency), the internal keyboard may remain disabled from the previous session, locking you out at the lock screen.

**Solution:** Register a scheduled task that runs `switchboard.exe --recover` at every system startup (before user logon), as `NT AUTHORITY\SYSTEM`. This guarantees the internal keyboard is always enabled at boot, independent of whether the app itself launches.

### Install the boot recovery task

**Primary (recommended):** right-click the tray icon → check **Lockout protection (recommended)**. Windows shows a single UAC prompt, the task is installed, and the menu reflects the new state.

**Secondary (CLI):** from any shell run

```cmd
switchboard.exe --install-boot-task
```

UAC will prompt — the EXE manifest is `requireAdministrator`, so every launch elevates. The tray-menu **Lockout protection** toggle internally invokes these subcommands and self-elevates via `ShellExecuteExW "runas"` if the parent is not already elevated. Direct CLI invocation behaves the same way: not-elevated invocations relaunch under UAC and wait for the elevated child. To uninstall:

```cmd
switchboard.exe --uninstall-boot-task
```

<details>
<summary>Tertiary fallback: PowerShell scripts</summary>

```powershell
# Run as Administrator
.\scripts\register-recovery-task.ps1
.\scripts\register-recovery-task.ps1 -SwitchboardPath "C:\your\new\path\switchboard.exe"
.\scripts\unregister-recovery-task.ps1
```
</details>

This creates a scheduled task named **"switchboard-boot-recover"** that:
- Runs at system startup (before any user logs in)
- Executes `switchboard.exe --recover` (unconditional keyboard enable + exit)
- Runs as `NT AUTHORITY\SYSTEM` with highest privileges
- Completes in <1 second (no impact on boot time)

Re-running `--install-boot-task` (or re-checking the tray item) is safe — it replaces the existing task in place (handy after moving `switchboard.exe`).

### Why it's strongly recommended

The boot recovery task adds a **belt-and-suspenders** safety layer:

1. **Boot task** (pre-logon) → ensures keyboard enabled at startup
2. **Cold-start ENABLE** (app launch) → ensures keyboard enabled when app starts
3. **Crash-aware safe mode** (C+A) → detects crashes and starts with blocking disabled
4. **Manual `--recover`** → user-invoked emergency recovery

Even if the app never launches, the boot task ensures you can always type at the lock screen.

**When to skip:** If you have a USB keyboard permanently connected, the boot task is redundant (but harmless).

**Stale-path warning:** if you move `switchboard.exe` after registering the boot task, the task's `<Command>` still points at the old path. Re-check the tray item (or re-run `--install-boot-task`) to update it. The app warns at startup if it detects this drift.

## Recovery (try in order of ease)

| # | Scenario | Recovery |
|---|---|---|
| 1 | App running, signed in | Right-click tray icon → uncheck **"Active"**. Keyboard works immediately. |
| 2 | Tray won't respond (hung instance) | **(1)** Press **Win+R**, type the full path to `switchboard.exe --recover` → press Enter. (Requires touchpad or Nuphy to navigate Run dialog.) App unconditionally re-enables keyboard and exits. **(2)** Task Manager (Ctrl+Shift+Esc) → Processes → `switchboard.exe` → **End task**. (Leaving hung instance running allows it to re-disable after `--recover` finishes.) **Optional:** Create a desktop shortcut with Target = `<full-path>\switchboard.exe --recover` for one-click recovery. |
| 3 | Signed in, app crashed (no tray) | File Explorer → double-click `switchboard.exe` → UAC → launch-time ENABLE fires. |
| 4 | At lock screen, no keyboard | Touchpad → **Accessibility icon (lower-right corner)** → **On-Screen Keyboard** → type PIN. (**`Win+Ctrl+O` does NOT work at lock screen.**) If touchpad frozen: plug USB keyboard or use power button + volume-up (built-in firmware recovery keyboard). |
| 5 | Anywhere — universal fallback | **Plug USB keyboard.** Works on lock screen, BIOS, UEFI, BitLocker recovery, and WinRE — the only path that survives every failure mode. **Keep one in your bag.** |
| 6 | Last resort | Hard power-off (hold power 10 seconds). Cold boot → sign in via row 4 or 5 → launch `switchboard.exe` (forces ENABLE). |

**Pre-OS (BitLocker, UEFI, WinRE):** USB keyboard is the only universal path.

## Troubleshooting

### "Nuphy paired but keyboard not disabling"

1. Ensure Nuphy is on and within Bluetooth range.
2. In Windows Settings → Bluetooth, toggle Nuphy off, then back on (re-pair).
3. Restart the app. The app keeps the internal keyboard enabled during the entire session until Nuphy successfully connects; it does not cache pairing state from a prior boot.
4. If still not disabled: check `%LOCALAPPDATA%\switchboard\switchboard.log` for "Nuphy not connected" or SetupAPI errors.

### "Verify mismatch: keyboard disable failed, Active toggled off"

The app tried to disable the keyboard but post-operation verification reported it still enabled. This can happen if:
- Windows PnP rebalance happened during the disable operation (rare).
- SetupAPI call succeeded but device didn't actually disable (driver issue, hardware problem).

**Recovery:** Right-click tray → check **"Active"** again. App will retry. If it fails repeatedly:
1. Check Device Manager (Ctrl+X, Device Manager) → Keyboards → right-click internal keyboard.
2. If it shows "Disabled," click "Enable" to restore it manually.
3. Restart the app and try again.
4. If the problem persists, file an issue with logs from `%LOCALAPPDATA%\switchboard\switchboard.log`.

### "App won't launch: SmartScreen or UAC error"

1. **"Unknown publisher" UAC on every launch** is expected and safe. Click **"Yes"**.
2. **SmartScreen "This file may be unsafe"** after download: Click **"More info" → "Run anyway"**. Or right-click file → Properties → Unblock → OK.
3. Unusual error? Check Windows Event Viewer → Applications and Services Logs for "switchboard" entries.
4. File issue with: Windows version/build, exact error, and logs from `%LOCALAPPDATA%\switchboard\switchboard.log`.

## FAQ

### Does this work on other Surface models?

Untested. Hardcoded for Laptop 7 (SAM-bus parent, VID_045E&PID_006C). File an issue with hardware details if you want support for other devices.

### What if Nuphy battery dies during active use?

Windows BLE stack notices within 10–30 seconds. Keyboard re-enables automatically. During that window: use tray **Active** toggle or run `--recover`.

### Can I run this via Group Policy?

Yes. Create a logon task that runs `switchboard.exe` with admin privileges. Single portable exe; no dependencies.

### Power consumption?

~2–5 MB RAM, <1% CPU when idle. BLE monitoring is handled by Windows. No measurable battery impact.

### What's "Active" in the tray menu?

Checked (✓) = app monitors BLE and disables keyboard on connect. Unchecked = internal keyboard always enabled, regardless of Nuphy state. Toggle to quickly enable or disable the feature.

### Where are logs?

`%LOCALAPPDATA%\switchboard\switchboard.log` (safe for bug reports; no PII collected).

## Known limitations

- **Unsigned binary.** SmartScreen may warn on first download; "Unknown publisher" UAC label appears on every launch.
- **Multi-user / RDP:** Not supported.
- **Logging:** Debug logs to `%LOCALAPPDATA%\switchboard\switchboard.log` (safe for bug reports; no PII).

## Safety model

- **Cold-start unconditional ENABLE.** Every time the app launches, it unconditionally re-enables the keyboard *before* checking Nuphy state. Crash recovery is automatic on next launch.
- **Quit always restores.** Right-click tray → Quit always re-enables the keyboard before exiting, even if the app's background thread is stuck.
- **Best-effort within Windows session.** Suspend, logoff, shutdown, and lid-close all unconditionally re-enable. Recovery for pre-OS scenarios (BitLocker, UEFI) requires USB keyboard.

## License

MIT — see [LICENSE](LICENSE).
