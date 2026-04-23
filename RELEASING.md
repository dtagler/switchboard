# Releasing SwitchBoard

This runbook guides the owner through cutting a v0.1.0 release after passing George's smoke test.

## Prerequisites

Before starting, confirm:

- Docker Desktop is running.
- You have passed all 12 tests in George's `owner-execution-checklist.md` (or at minimum tests 1–9; tests 10–12 are adversarial pending re-test on consolidated build).
- Your repo is in a clean state (no uncommitted changes).

## Cut a release

### Step 1: Build

```powershell
.\scripts\build.ps1
```

This compiles the app in Docker (cargo-xwin), produces `.\dist\switchboard.exe` (approximately 390 KB; size is profile-dependent and not guaranteed), and strips symbols per the release profile.

Typical build time: 1–2 minutes (first build with empty `.xwin-cache` may take longer while SDK downloads; subsequent builds reuse cache).

### Step 2: Package and hash

```powershell
.\scripts\dist.ps1 -Version 0.1.0
```

This script:
- Verifies `switchboard.exe` exists and is non-empty (approximately 390 KB expected).
- Reads and echoes the exe size.
- Compares the Cargo.toml version (0.1.0) to your `-Version` parameter — fails if mismatch.
- Stages a release directory: `dist\switchboard-v0.1.0-aarch64-pc-windows-msvc\`
- Copies the exe, README.md, ARCHITECTURE.md, `.env.example` (required — users must rename to `.env` and fill in their Nuphy MAC, otherwise BLE never subscribes), and LICENSE.
- Creates a zip: `dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip`
- Computes SHA256 of the zip and writes to `dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip.sha256` in standard format (`<hash>  <filename>`).
- Echoes the final manifest (file, size, sha256).

The `-SkipBuild` flag (optional) skips step 1 if you've already built and want to re-package without rebuilding.

### Step 3: Verify the artifact

On the same machine where you packaged it, confirm the hash:

```powershell
Get-FileHash -Path dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip -Algorithm SHA256
```

Compare the hash to the one printed by the release script and stored in `.sha256`. They must match exactly.

On another machine (or after moving the zip), verify the sidecar:

```powershell
# Extract the sidecar format (hash  filename)
$expected = Get-Content dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip.sha256
$computed = "$(Get-FileHash -Path dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip -Algorithm SHA256).Hash  switchboard-v0.1.0-aarch64-pc-windows-msvc.zip"
if ($expected -eq $computed) { Write-Host "✓ Hash match" } else { Write-Error "Hash mismatch!" }
```

Or on Unix:

```bash
cd dist
sha256sum -c switchboard-v0.1.0-aarch64-pc-windows-msvc.zip.sha256
```

## Tag the release (optional, for git history)

```powershell
git tag v0.1.0
git push --tags
```

## Publish (optional, manual upload)

The packaged zip and `.sha256` sidecar are in `dist\`.

### GitHub Releases

If you maintain a GitHub repo, upload both files:

```powershell
gh release create v0.1.0 `
  dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip `
  dist\switchboard-v0.1.0-aarch64-pc-windows-msvc.zip.sha256 `
  --notes-file RELEASE-NOTES-v0.1.0.md
```

(See the RELEASE-NOTES template section below.)

### Other platforms

Copy the `.zip` and `.sha256` to your distribution point (internal file server, cloud storage, email, etc.). Consumers can verify the hash as shown in "Verify the artifact" above.

## Release notes template

Before uploading to GitHub or sharing, create a `RELEASE-NOTES-v0.1.0.md` file in the repo root:

```markdown
# v0.1.0 — Initial Release

## Changelog

First stable release. Disables Surface Laptop 7 internal keyboard when Nuphy Air75 (Bluetooth) connects.

## Safety

This release has passed Jerry's 12-test safety gate:
1. Daily use: connect/disconnect/resume/toggle.
2. Forced crash and recovery.
3. Hung-instance escape hatch.
4. Quit-while-disabled correctness.

See the Behaviors and Threading model sections of ARCHITECTURE.md for the fail-safe behaviors.

## Known Limitations

- **Unsigned:** SmartScreen may warn on first download. Click "More info" → "Run anyway" or right-click file → Properties → Unblock.
- **Manual launch only out of the box:** Use the **Auto-start SwitchBoard at login** tray item to enable per-user autostart, or **Lockout protection (recommended)** for a SYSTEM-level boot recovery task.
- **Hardware-specific:** Hardcoded for Surface Laptop 7 + Nuphy Air75 V3. Target Bluetooth MAC is loaded at runtime from `SWITCHBOARD_NUPHY_BD_ADDR` (env var or `.env` file) — see `.env.example`.
- **Pre-OS scenarios:** BitLocker recovery, UEFI, WinRE require USB keyboard (OS-level limitation, not app-specific).

## Installation

1. Extract the zip — you should see `switchboard.exe`, `README.md`, `ARCHITECTURE.md`, `LICENSE`, and `.env.example`.
2. **Rename `.env.example` to `.env`** and replace the placeholder MAC with your Nuphy's actual Bluetooth address (Settings → Bluetooth & devices → your Nuphy → Properties → Bluetooth address). Without this step the app launches but never tracks Nuphy connection state.
3. Run `switchboard.exe`. UAC prompt → click "Yes".
4. Tray icon appears.

See README.md for full details.

## Recovery

If the app crashes while the keyboard is disabled:
- Right-click tray → uncheck "Active" (if app is still running).
- Or run `switchboard.exe --recover` (touchpad + Run dialog or Task Manager).
- Or sign in via touchpad + On-Screen Keyboard at lock screen.
- Universal fallback: plug a USB keyboard.

See README.md §Recovery for complete procedures.

## Signing

The binary ships **unsigned**.
```

Edit this file to match your communication style and any additional release notes specific to your deployment.

## Smoke verification (post-install)

On a clean Surface Laptop 7 (or before distributing to users):

1. Download and extract the zip.
2. Double-click `switchboard.exe`.
3. UAC prompt → click "Yes".
4. Confirm tray icon appears in the lower-right corner.
5. Connect Nuphy Air75.
6. Confirm internal keyboard stops working (try typing).
7. Disconnect Nuphy.
8. Confirm internal keyboard works again.
9. Right-click tray → uncheck "Active".
10. Confirm internal keyboard works regardless of Nuphy state.
11. Right-click tray → Quit.
12. Confirm internal keyboard still works and process exits.

If all 12 steps pass, the build is safe to distribute. This is a subset of the full owner-execution checklist.

## Rollback

If the installed app crashes or misbehaves:

```powershell
# Unconditionally re-enable internal keyboard (works even if instance is hung)
.\switchboard.exe --recover
```

Or delete the exe and the keyboard remains enabled. There is no registry, no `Program Files`, no state to clean up. Total uninstall time: 5 seconds.

---

**Questions?** Check README.md §Recovery and §Troubleshooting, or review ARCHITECTURE.md for design context.
