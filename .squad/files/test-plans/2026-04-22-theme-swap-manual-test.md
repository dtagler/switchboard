# Theme-Swap Manual Test Plan — 2026-04-22

**Author:** George (QA / Safety Tester)
**Tester:** Brady
**Hardware:** Surface Laptop 7, Snapdragon X Elite, Windows 11
**Build under test:** Freshly compiled `switchboard.exe` (release, aarch64-pc-windows-msvc) with embedded ICO resources 101 (dark) / 102 (light) and `WM_SETTINGCHANGE` listener in `src/theme.rs` + `src/main.rs` wndproc.
**Goal:** Verify the live tray-icon theme swap actually works on real hardware, that the EXE icon is correct in Explorer, and that the Escape-hold fail-safe still works after the icon code was added.

> ⚠️ **Critical contract:** Test 7 (Escape fail-safe) is non-negotiable. If it fails, **roll the build back immediately** — do not ship, do not autostart, do not "we'll fix it next iteration."

---

## Pre-flight

Do these in order. Don't skip.

1. **Uninstall any legacy `kbblock` install.**
   - If `scripts\uninstall-kbblock-legacy.ps1` exists: run it from an elevated PowerShell.
     ```powershell
     pwsh -ExecutionPolicy Bypass -File .\scripts\uninstall-kbblock-legacy.ps1
     ```
   - If the script is missing or you're unsure whether you ever installed `kbblock`, manually verify all of the following are gone (per the rename decision, decisions.md §kbblock→switchboard):
     - HKCU `…\CurrentVersion\Run\kbblock` value — should NOT exist
       ```powershell
       reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v kbblock
       ```
     - Scheduled tasks `kbblock-logon` and `kbblock-boot-recover` — should NOT exist
       ```powershell
       schtasks /Query /TN kbblock-logon
       schtasks /Query /TN kbblock-boot-recover
       ```
     - `%LOCALAPPDATA%\kbblock\` directory — fine to leave but note its presence.
   - **Pass condition:** all three checks return "not found" or the script reports clean exit.

2. **Verify nothing is running.** Open Task Manager → Details. Confirm there is **no** `kbblock.exe` and **no** `switchboard.exe` process.
   - If either is present, right-click → End task. Re-check.

3. **Note the BT keyboard state.** The Nuphy Air75 may or may not be paired/connected at start. Write it down — connected vs. disconnected, paired vs. unpaired. The theme tests don't require BT, but if anything weird happens later we want to know what the starting state was.

4. **Note the current Win11 theme** (Settings → Personalization → Colors → "Choose your mode"). Record both:
   - "Choose your mode" (overall, controls **SystemUsesLightTheme**)
   - "Choose your default Windows mode" if shown separately on this build (some Win11 versions split these)

5. **Copy the freshly-built `switchboard.exe` to a stable path** that won't move during testing. Suggested: `C:\Tools\switchboard\switchboard.exe`. Do NOT run it from the `target\` build output — that path can be wiped by a `cargo clean` mid-test.

---

## Test 1: EXE icon in File Explorer

**Setup:** `switchboard.exe` is at the stable path from Pre-flight step 5. The app is **not** running.

**Steps:**
1. Open File Explorer. Navigate to the folder containing `switchboard.exe`.
2. Cycle through Explorer view modes: View → **Small icons**, then **Medium icons**, then **Large icons**, then **Extra large icons**.
3. (Optional) Right-click `switchboard.exe` → Properties → check the icon shown in the Properties dialog header.

**Expected:**
- The SwitchBoard "§" keycap icon (the **dark variant** — Surface blue stroke, white § glyph) appears at every size.
- The icon is sharp/crisp at each size — Win11 should be picking the right embedded resolution (16, 20, 24, 32, 48, 256).
- Same icon shows in the Properties dialog.

**Pass:** Icon visible, recognizable as the SwitchBoard keycap, dark variant, crisp at all four Explorer view sizes.

**Fail conditions:**
- Generic Windows app icon shown (means resource 101 didn't embed at all)
- Icon shown but pixelated/blurry at small or large sizes (means a size variant is missing from the .ico)
- Wrong icon — light variant, or wrong glyph (means the build picked up the wrong asset)

---

## Test 2: Initial tray icon — DARK theme

**Setup:** Win11 in Dark mode.

**Steps:**
1. Open Settings → Personalization → Colors. Set **"Choose your mode"** = **Dark**.
2. Confirm taskbar is visibly dark.
3. Verify the registry agrees:
   ```powershell
   reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" /v SystemUsesLightTheme
   ```
   Expected: `SystemUsesLightTheme    REG_DWORD    0x0`
4. Launch `switchboard.exe` (double-click).
5. Find it in the system tray (may be in the overflow chevron — pin it for the rest of the tests).

**Expected:**
- Tray icon = **dark variant** (white § on Surface blue keycap stroke).
- Hover tooltip says "SwitchBoard" (brand-cased), per rename decision.

**Pass:** Dark variant icon visible in tray on first launch when system is in dark mode.

**Fail:** Wrong variant (light) shown, or no icon at all, or icon is the generic Windows fallback.

---

## Test 3: Initial tray icon — LIGHT theme

**Setup:** Switching to light mode and re-launching.

**Steps:**
1. Right-click the SwitchBoard tray icon → **Exit**.
2. Confirm in Task Manager that `switchboard.exe` is gone.
3. Settings → Personalization → Colors → **"Choose your mode"** = **Light**.
   - **Important:** if Win11 shows TWO dropdowns ("Choose your default Windows mode" and "Choose your default app mode"), set the **Windows mode** dropdown to Light. The app reads `SystemUsesLightTheme`, which is the *Windows* mode, not the *app* mode (`AppsUseLightTheme`).
4. Confirm taskbar is visibly light.
5. Verify the registry:
   ```powershell
   reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" /v SystemUsesLightTheme
   ```
   Expected: `SystemUsesLightTheme    REG_DWORD    0x1`
6. Re-launch `switchboard.exe`.

**Expected:**
- Tray icon = **light variant** (near-black § on Surface blue keycap stroke).

**Pass:** Light variant on first launch when system is in light mode.

**Fail:** Dark variant shown (means initial theme read failed or the wrong resource ID was selected). If this fails, Test 4 will also fail — fix this first.

---

## Test 4: Live swap — DARK → LIGHT, no restart

**This is the core test for this build.**

**Setup:** Start switchboard with system in **Dark** mode (re-do Test 2 setup if needed). The tray icon must be the dark variant before you start.

**Steps:**
1. Confirm tray icon is dark variant.
2. Settings → Personalization → Colors → "Choose your mode" → **Light** (Windows mode, see Test 3 caveat).
3. **Watch the tray icon.** Don't blink.

**Expected:**
- Within ~1 second of toggling the setting, the tray icon swaps to the **light variant**.
- No restart of `switchboard.exe` required.
- Process ID in Task Manager is unchanged (i.e., it didn't crash and respawn — there is no respawn logic for this).

**Pass:** Tray icon updates to light variant, same PID, within ~1 second.

**Fail:**
- Icon doesn't change at all (most likely cause: wndproc isn't receiving `WM_SETTINGCHANGE`, or the lParam string check `is_immersive_color_set` is rejecting it, or the resource ID swap is calling `set_icon` with the same icon).
- Icon changes but to the wrong variant.
- App crashes (check Task Manager for the process).

**Diagnostic if it fails — see "Diagnostics" section at the bottom.** Do not skip to Test 5; if Test 4 fails, the swap logic is broken in both directions.

---

## Test 5: Live swap — LIGHT → DARK, no restart

**Setup:** Continuing from Test 4. App still running; system in Light mode; icon = light variant.

**Steps:**
1. Confirm tray icon is light variant.
2. Settings → Personalization → Colors → "Choose your mode" → **Dark**.
3. Watch the tray icon.

**Expected:**
- Within ~1 second, tray icon swaps to **dark variant**.
- Same PID.

**Pass / Fail:** Same criteria as Test 4, mirrored.

---

## Test 6: Multiple rapid toggles — flicker and handle-leak check

**Setup:** App running. System in either mode.

**Steps:**
1. Open Task Manager → Details tab. Right-click any column header → **Select columns** → tick **GDI Objects** and **USER Objects**. Find `switchboard.exe` and **note the starting GDI count and USER count.**
2. Toggle Settings → Personalization → Colors → "Choose your mode" between Dark and Light **rapidly** — at least 5 full toggles within 30 seconds. (Yes, this is annoying with the Settings UI; do your best. If toggling via Settings is too slow, you can toggle via PowerShell — see Diagnostics.)
3. After the last toggle, wait ~5 seconds for things to settle.

**Expected:**
- Tray icon ends in the **correct** variant matching the final theme state.
- No visible flicker/tearing during the swaps.
- GDI Objects count for `switchboard.exe` is **stable** — within ±5 of the starting count. (A constant slow growth across toggles = handle leak; it would only matter over thousands of toggles, but if you see the count grow by 10+ per toggle, that's a regression worth flagging.)
- USER Objects count likewise stable.
- App is still responsive (right-click tray menu opens normally).

**Pass:** Correct final icon, stable handle counts, app responsive.

**Fail:**
- Final icon doesn't match final theme — race condition in the swap logic.
- GDI/USER count climbing monotonically with each toggle — handle leak. Flag for Jerry; the `Icon::from_resource` path is supposed to return shared handles that don't need `DestroyIcon`, so a leak here would invalidate the design assumption in decisions.md.
- App freezes or stops responding to tray right-click — wndproc is stuck in the swap path.

---

## Test 7: Escape fail-safe — NON-NEGOTIABLE

**The whole point of this app is that it never locks you out. This test verifies the icon code didn't break the fail-safe.**

**Setup:** App running. Doesn't matter which theme. **Do this with the built-in keyboard NOT yet blocked** (i.e., either no BT keyboard connected, or block-on-connect hasn't fired yet — confirm by typing on the built-in keyboard and seeing characters appear in Notepad).

Then trigger a block (connect the Nuphy, or use whatever your normal block path is) so the built-in is disabled. Verify the built-in IS blocked (typing produces nothing in Notepad).

**Steps:**
1. With the built-in keyboard blocked, **press and hold the Escape key for a full 10 seconds.** Use a watch — don't guess.
2. Release.

**Expected:**
- Built-in keyboard is re-enabled (open Notepad, type — characters appear).
- `switchboard.exe` exits cleanly (gone from Task Manager within a second or two).
- No tray icon left behind (mouse over the previous tray-icon spot — Win11 should clean it up; if it lingers, hover to dismiss, that's a Win11 cosmetic and not an app failure).

**Pass:** Built-in keyboard works; app process is gone.

**FAIL = ROLLBACK:** If the built-in keyboard does NOT come back after 10 seconds of Escape, the safety contract is broken.
- **Immediately** unplug power and hold the power button to hard-reboot if you can't recover input.
- **Do not** ship this build. Roll back to the previous known-good `switchboard.exe`.
- File a P0 with Newman + Jerry. Describe exactly what you did to trigger the block.

---

## Diagnostics (if a test fails)

### Read the theme registry value

```powershell
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" /v SystemUsesLightTheme
```
- `0x0` = dark, `0x1` = light, **missing** = the app falls back to dark per `src/theme.rs::system_uses_light_theme()`.

You can also force-toggle from PowerShell (faster than the Settings UI for Test 6):

```powershell
# Set DARK
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name SystemUsesLightTheme -Value 0
# Set LIGHT
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name SystemUsesLightTheme -Value 1
```

⚠️ **Important caveat:** Setting the registry value directly does **not** by itself broadcast `WM_SETTINGCHANGE`. If the icon doesn't swap when you set the registry directly, that does NOT mean the app is broken — it means Windows didn't tell it. To force a broadcast after setting:

```powershell
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll", CharSet=CharSet.Auto, SetLastError=true)]
  public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam, uint flags, uint timeout, out IntPtr result);
}
"@
$HWND_BROADCAST = [IntPtr]0xFFFF
$WM_SETTINGCHANGE = 0x001A
$result = [IntPtr]::Zero
[W]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [IntPtr]::Zero, "ImmersiveColorSet", 2, 5000, [ref]$result) | Out-Null
```

If toggling via the **Settings UI** doesn't trigger a swap but the broadcast snippet above does → the app's wndproc is fine, and the issue is upstream (rare). If neither works → the wndproc isn't handling the message correctly.

### Verify embedded ICO resources in the EXE

Per Jerry's release-build verification pattern (Docker, no host installs):

```powershell
docker run --rm -v "${PWD}:/work" -w /work debian:bookworm-slim bash -c "apt-get update -qq && apt-get install -y -qq icoutils && wrestool -l /work/path/to/switchboard.exe | grep -i icon"
```
- Expected: at least two icon entries with IDs `101` and `102`.
- If only `101` appears → the light ICO didn't embed. Check `manifest/switchboard.rc` has both `ICON` lines (101 dark, 102 light) and rebuild.
- If neither appears → `embed-resource` didn't run; check `build.rs` and the build log.

To dump and visually inspect a single embedded icon:

```powershell
docker run --rm -v "${PWD}:/work" -w /work debian:bookworm-slim bash -c "apt-get update -qq && apt-get install -y -qq icoutils && wrestool -x -t 14 -n 102 /work/path/to/switchboard.exe -o /work/extracted-light.ico"
```

### Where to look in source

- **`src/theme.rs`** — Two functions:
  - `system_uses_light_theme() -> bool` — reads HKCU registry DWORD; returns false (dark) on read error or missing key.
  - `is_immersive_color_set(lparam) -> bool` — checks the lParam string passed with `WM_SETTINGCHANGE` equals `"ImmersiveColorSet"`.
- **`src/main.rs`** — Look for:
  - `mod theme;`
  - The `current_theme_light: bool` field on `AppState`.
  - The `WM_SETTINGCHANGE` arm in the wndproc — calls `is_immersive_color_set(lparam)`, then `refresh_tray_theme(state)`.
  - `current_theme_icon()` — used by all three `TrayIconBuilder::new()` sites (admin / needs-admin / error).
- **`manifest/switchboard.rc`** — must contain both `101 ICON` and `102 ICON` lines.
- **`build.rs`** — must have `rerun-if-changed` entries for both .ico files.

### If wndproc isn't receiving WM_SETTINGCHANGE

1. Confirm the message loop is actually pumping — if the app is hung in any other path, no messages get through. Check responsiveness via right-click on the tray icon: if the menu opens, the loop is alive.
2. Add a `tracing::debug!` line at the top of the `WM_SETTINGCHANGE` arm to log every receipt, then rebuild. (Coordinate with Newman / Jerry — don't ship a debug build.)
3. As a sanity check, broadcast the message manually using the PowerShell snippet above. If the manual broadcast triggers the swap but the Settings toggle doesn't, the issue is theme-broadcast-specific (worth a Win11-build-version note).

---

## Reporting back

For each test, record: **PASS** / **FAIL** / **N/A** (with reason if N/A), and any unexpected observations even on PASS.

If anything in Tests 1–6 fails: file an issue, tag Jerry + Elaine.
If Test 7 fails: roll back the build immediately, then file with Newman + Jerry as P0.
