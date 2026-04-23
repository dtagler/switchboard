# Archived Decisions — 2026-04-22T23-55-00Z

# Team Decisions

> Canonical decision ledger. Append-only. Agents propose via .squad/decisions/inbox/; Scribe merges here.

## 2026-04-22: SVG → ICO Pipeline (Soft Accent tray icons)

**By:** Elaine. **Date:** 2026-04-22. **Status:** Final.

### Decision

The SwitchBoard tray `.ico` files are generated reproducibly via a single Docker container — `debian:bookworm-slim` with `inkscape`, `imagemagick` (v6), and `fonts-urw-base35` installed at run time. No host installs.

Driver script: `scripts/build-icons.sh` (run via `bash scripts/build-icons.sh` inside the container, with the repo root mounted at `/work`).

Outputs:
- `assets/icons/switchboard-light.ico` — 16, 20, 24, 32, 48, 256
- `assets/icons/switchboard-dark.ico`  — 16, 20, 24, 32, 48, 256
- `assets/icons/switchboard.ico` — copy of the dark variant; Win11 default fallback when `SystemUsesLightTheme` cannot be read.

Reproduce:

```powershell
docker run --rm -v "${PWD}:/work" -w /work debian:bookworm-slim `
    bash /work/scripts/build-icons.sh
```

### Approach: font-bearing container (option a)

The Soft Accent SVGs render the `§` glyph from `Palatino Linotype`. That font isn't redistributable and isn't in any container by default, so the rasterizer needs either (a) a metric-equivalent font installed, or (b) the text element flattened to a path before render time.

We chose **option (a)**. The Debian package `fonts-urw-base35` ships URW++ **P052**, the GPL-licensed font metric-equivalent to Palatino designed for Ghostscript. The build script `sed`-injects `P052` ahead of `Palatino Linotype` in the SVG `font-family` list so Inkscape resolves to P052 directly instead of falling back to a sans-serif. The rendered `§` is visually indistinguishable from real Palatino at tray sizes.

Why not text-to-path (option b): Inkscape's `--export-text-to-path` still needs the source font installed to compute glyph outlines, so it gains us nothing over rendering with P052 directly, and it would mutate the source SVG.

### Pipeline (per variant)

1. `sed` the source SVG to put `P052` first in `font-family`. Result lives in `/tmp/icons/keycap-s-{variant}.svg` — source SVGs are never modified.
2. Inkscape rasterizes the working SVG to PNG at each of the 6 target sizes (`--export-width=N --export-height=N`). Inkscape's renderer handles the rounded-rect stroke + glyph anti-aliasing better than rsvg-convert at small sizes.
3. ImageMagick v6 `convert` packs the 6 PNGs into a single multi-resolution `.ico`. Order matters for some Windows shell consumers (smallest first), so the script lists them explicitly.
4. `switchboard.ico` is a byte-for-byte copy of the dark variant.

Verification: the script ends by running `identify` on each `.ico` and printing the contained sizes. All three files were confirmed to contain `[16, 20, 24, 32, 48, 256]`.

### Constraints met

- **Docker only.** No host packages installed. Image is `debian:bookworm-slim` pulled at first run; subsequent runs reuse the cached layer.
- **ARM64 friendly.** `debian:bookworm-slim` has a native `linux/arm64` manifest — Docker on Surface Laptop 7 pulls the arm64 variant automatically. No `--platform` override needed.
- **Font fidelity.** P052 renders `§` correctly; verified visually at 256 px.
- **Reproducible.** The script is checked in; the only inputs are the two source SVGs and one Debian image tag.

### Trade-offs / known limitations

- ImageMagick on Debian bookworm is **v6**, so the binary is `convert`, not `magick`. If we ever pin a newer base image, swap `convert` → `magick`.
- The `apt-get install` re-runs on every container invocation (adds ~25 s of cold-start). Acceptable because icon regeneration is not a hot path. If we start regenerating in CI on every commit, we should bake a small `Dockerfile.icons` and push it to GHCR.
- P052 is metric-equivalent but not pixel-identical to Palatino Linotype. Side-by-side, designers might notice <1 px differences in stroke contrast. At 16 px tray size, no human can see the difference.

### Cross-ref

- Soft Accent strategy: Entry below (2026-04-22 Tray Icon decision)
- Source SVGs: `.squad/files/icon-concepts/keycap-s-{light,dark}.svg`
- Visual previews: `.squad/files/icon-concepts/preview-png/`
- Reusable pattern: `.squad/skills/svg-to-ico-docker/SKILL.md`

---

## 2026-04-22: Tray Icon — Soft Accent Strategy Selected

**By:** Brady (Copilot directive). **Status:** Final. **Decision:** Two ICOs shipped with theme-aware swap at runtime.

### Decision
Tray icon strategy #4 (Soft Accent):
- **Keycap stroke:** Surface Blue (#0078D4)
- **Glyph:** Theme-aware (white on dark taskbar, near-black on light)
- **Implementation:** Two .ico files; swap at runtime via SystemUsesLightTheme registry check + WM_SETTINGCHANGE listener
- **Deliverables:** keycap-s-dark.svg, keycap-s-light.svg, keycap-s.svg, final-icon.html

### Rationale
User selection from theme-variants.html gallery. Provides visual consistency with Surface Blue branding and adapts to taskbar theme dynamically.

### Cross-ref
- Directive source: .squad/decisions/inbox/copilot-directive-icon-soft-accent-2026-04-22T13-17-30Z.md
- Owner: elaine (icon design specialist)

---

## 2026-04-22: kbblock → switchboard / SwitchBoard rename — EXECUTED

**By:** Jerry. **Status:** Final. **Trigger:** Brady directive (checkpoint 004 + prior naming decision).

### Decision

The previously planned crate / binary / brand rename has been executed end-to-end across source, manifest, build glue, scripts, and docs. Identifier-vs-brand discipline:

- **Lowercase `switchboard`** = crate name, bin name, mutex name, window class, registry Run value, scheduled task names, log paths, CLI prefix.
- **CamelCase `SwitchBoard`** = user-facing brand only — tray tooltips, menu items, balloons, dialog titles/bodies, README/RELEASING titles, manifest `<description>`.

### (a) Upgrade-affecting identifier changes — FLAG FOR OWNER

These are behavioral renames that break continuity with any installed kbblock build. Document for the first install of the renamed build:

| Surface | Old | New | Impact |
|---|---|---|---|
| Single-instance mutex | `Local\kbblock-singleton-v1` | `Local\switchboard-singleton-v1` | Old + new can run simultaneously. Kill old process before launching new. |
| Hidden window class | `kbblock_msg_window` | `switchboard_msg_window` | `--recover` IPC won't find an old running instance. Benign. |
| HKCU Run value | `…\Run\kbblock` | `…\Run\switchboard` | Old autostart entry orphaned; user must re-tick "Start at login". |
| Logon task name | `kbblock-logon` | `switchboard-logon` | Old task lingers until removed; harmless if old exe is gone. |
| Boot recovery task | `kbblock-boot-recover` | `switchboard-boot-recover` | Same as above. Recommend `schtasks /Delete /TN kbblock-boot-recover` after migration. |
| Log/data dir | `%LOCALAPPDATA%\kbblock\` | `%LOCALAPPDATA%\switchboard\` | Old crash logs / running.lock stranded. Cosmetic only. |

### (b) Build verification

`cargo xwin check --target aarch64-pc-windows-msvc` ran inside the existing `kbblock:build` Docker image (no host installs, per Brady directive). **Result: PASS.** Compile finished in 15.27s with 18 pre-existing warnings (unused `BOOL`/`Result` returns on `KillTimer`/`TranslateMessage`), zero errors, zero rename-induced failures.

### Files changed (19)

`Cargo.toml`, `Cargo.lock`, `build.rs`, `manifest/switchboard.rc` (renamed from kbblock.rc), `manifest/switchboard.exe.manifest` (renamed from kbblock.exe.manifest), `docker/Dockerfile.build`, `scripts/build.ps1`, `scripts/build.sh`, `scripts/release.ps1`, `scripts/release.sh`, `scripts/register-recovery-task.ps1`, `scripts/unregister-recovery-task.ps1`, `src/main.rs`, `src/autostart.rs`, `src/boot_task.rs`, `README.md`, `ARCHITECTURE.md`, `RELEASING.md`. (`src/ble.rs` and `src/device.rs` had zero references — left alone.)

### Out of scope (deferred)

- ICO embedding into the .rc — waiting on Elaine's final assets. `build.rs` has a TODO comment marking the spot. Existing `embed-resource` pipeline can carry the icons via additional `ICON` statements in `manifest/switchboard.rc`; no new crate (winres, etc.) needed.
- `target/` artifacts (build cache, will regenerate).
- `spikes/` (no source references; only stale build cache).
- `.squad/` historical logs (correctly preserve historical context).

---

## 2026-04-22: ICO Embedding + Runtime Theme Swap (Soft Accent)

**By:** Jerry (Windows Architect). **Status:** Implemented; ready for release build validation.

### Decision

Elaine's Soft Accent icon assets (switchboard-light.ico / switchboard-dark.ico) are embedded into the SwitchBoard exe via the existing `embed-resource = "2"` pipeline. No separate crate (winres, etc.). Resource IDs: 101 (dark, also exe icon default) and 102 (light). Theme swap occurs at runtime via Windows registry read (`HKCU\...\Themes\Personalize\SystemUsesLightTheme`) + WM_SETTINGCHANGE listener in the wndproc.

### Approach: Resource IDs and RC consolidation

The two ICO lines are added directly to `manifest/switchboard.rc`:

```
101 ICON "../assets/icons/switchboard-dark.ico"   ; IDI_TRAY_DARK — also EXE icon
102 ICON "../assets/icons/switchboard-light.ico"  ; IDI_TRAY_LIGHT
```

This avoids a separate `assets/icons/switchboard.rc` file and matches the prior design decision (recorded in build.rs § "Out of scope"). Single resource compile invocation, one rerun-if-changed list, one place to look.

ID 101 serves as the exe icon default (Win32 "lowest ID wins" convention picks it), which is appropriate because dark taskbar is Win11's default.

### Theme detection and WM_SETTINGCHANGE handler

New module `src/theme.rs` exposes:

- `system_uses_light_theme() -> bool` — reads the registry DWORD; 1 = light, 0 = dark, missing/error = dark.
- `is_immersive_color_set(lparam) -> bool` — filters `WM_SETTINGCHANGE` by lParam string (`"ImmersiveColorSet"`).

Integration in `main.rs`:
- `AppState` gains `current_theme_light: bool`, initialized at construction.
- New `WM_SETTINGCHANGE` arm in wndproc checks `is_immersive_color_set(lparam)`. If true, calls `refresh_tray_theme(state)`, which re-reads the registry and swaps the icon via `tray_icon.set_icon(Some(current_theme_icon(...)))` if the value changed.
- All three `TrayIconBuilder::new()` sites (admin, needs-admin, error) now use `current_theme_icon()` instead of the old flat default.

### Resource lifetime and handle safety

We use `Icon::from_resource(id, None)` (built-in resource loading) instead of `include_bytes!(...) + CreateIconFromResourceEx`. The tray-icon crate calls `DestroyIcon` on the previous icon when `Shell_NotifyIcon NIM_MODIFY` runs; our `Icon` value is dropped immediately after handoff. `Icon::from_resource` returns shared (not owned) handles — `DestroyIcon` on those is a no-op per MSDN, so there is nothing to leak. This avoids an entire class of handle-lifetime bugs that `CreateIconFromResourceEx` would require manual management for.

### Constraints met

- **Docker-only build.** No host installs; existing `kbblock:build` image used.
- **ARM64 friendly.** Compiles cleanly under `cargo xwin` targeting `aarch64-pc-windows-msvc`.
- **No new crate.** Used existing `embed-resource` pipeline.

### Caveats / known limitations

- **WM_SETTINGCHANGE delivery:** Only fires while the message loop is pumping. If the user toggles the taskbar theme during the 3s initial-policy grace window, the swap still happens (the loop is alive at that point). Verified by code path inspection.
- **HiDPI scaling:** `Icon::from_resource(id, None)` lets Windows pick the size (16px at 100% DPI, 32px at 200%). Elaine shipped 16/20/24/32/48/256px variants; no code change needed for different scales.
- **Runtime testing limited.** `system_uses_light_theme()` requires HKCU registry access, unavailable in the Docker test container. Manual validation on Windows required.
- **No unit tests for theme module.** Future tier-A pass: George can add trait-based abstraction to make the registry call testable in Docker.

### Build verification

`docker run --rm -v $PWD:/build -w /build kbblock:build cargo xwin check --target aarch64-pc-windows-msvc` → **PASS**, 19.22s. 18 pre-existing warnings (unused BOOL/Result on KillTimer/TranslateMessage), zero new errors, zero new warnings. The `embed_resource` call ran without complaint, confirming both .ico files were resolved relative to the .rc file.

Full `cargo xwin build --release` deferred (Brady will run before release tag).

### Files changed

- `manifest/switchboard.rc` — +2 ICON lines
- `src/theme.rs` — new (~80 LOC)
- `src/main.rs` — `mod theme;` + `current_theme_light` field + `WM_SETTINGCHANGE` handler + three `TrayIconBuilder` sites updated
- `build.rs` — removed TODO, added rerun-if-changed for .ico files

### Cross-ref

- **Implements:** 2026-04-22 "Tray Icon — Soft Accent Strategy Selected" (decisions.md)
- **ICO pipeline:** 2026-04-22 "SVG → ICO Pipeline (Soft Accent tray icons)" (decisions.md)
- **Orchestration log:** `.squad/orchestration-log/2026-04-22T23-55-00Z-jerry-ico-embed.md`

---

## 2026-04-22: Release build verified — switchboard.exe with embedded ICOs

**By:** Jerry (Windows Architect). **Date:** 2026-04-22 (later). **Status:** Verified — ready for signing / packaging.

### Decision

The post-rename, post-ICO-embed release build of SwitchBoard is verified end-to-end. No code changes; this records the verification result so the next stage (sign, package, tag) has an unambiguous "this exact artifact passed" reference.

### Build

Command (Docker only — no host installs):

```powershell
docker run --rm -v "${PWD}:/build" -w /build kbblock:build `
    cargo xwin build --release --target aarch64-pc-windows-msvc
```

Result: **PASS** in 15.56 s. 18 warnings (all pre-existing, identical to the rename + ICO-embed `cargo xwin check` runs — unused imports in device.rs/main.rs, dead constants/fields, `KillTimer`/`TranslateMessage` Result/BOOL ignores). Zero new warnings, zero errors. `embed_resource` compiled `manifest/switchboard.rc` cleanly; `llvm-rc` resolved both relative `.ico` paths.

### Artifact

| Field | Value |
|---|---|
| Path | `target/aarch64-pc-windows-msvc/release/switchboard.exe` |
| Size | **464,896 bytes** (454 KB / 0.44 MB) |
| Target | `aarch64-pc-windows-msvc` (Snapdragon X Elite native) |
| Profile | `release` (optimized) |

### ICO verification

Performed in a fresh `debian:bookworm-slim` container with `icoutils` (`wrestool`) and `imagemagick` (`identify`). Both group_icon resources present and intact:

| Resource | Type | ID | Lang | Frames | Sizes (px) | Byte-equiv to source |
|---|---|---|---|---|---|---|
| Dark (EXE icon + tray default) | RT_GROUP_ICON | 101 | 1033 | 6 | 16 / 20 / 24 / 32 / 48 / 256 | yes (Δ90 B = ICONDIR header overhead) |
| Light (runtime swap target) | RT_GROUP_ICON | 102 | 1033 | 6 | 16 / 20 / 24 / 32 / 48 / 256 | yes (Δ90 B = ICONDIR header overhead) |

The 256 px frame in each group is PNG-compressed (correct convention for that size). Frames 1–6 (RT_ICON) belong to group 101; frames 7–12 belong to group 102. Frame bytes are identical to Elaine's `assets/icons/switchboard-{dark,light}.ico`; only the group header differs by the expected fixed amount.

ID 101 is the lowest icon group ID, so Win32 picks it as the **EXE icon** shown in Explorer / Alt-Tab / taskbar — appropriate because Win11's default taskbar is dark. The runtime tray defaults to ID 101 and swaps to ID 102 via `theme.rs` + the `WM_SETTINGCHANGE` arm in wndproc when `SystemUsesLightTheme = 1`.

### Constraints met

- **Docker only.** Existing `kbblock:build` image used; no host installs introduced. Verification container (`debian:bookworm-slim`) is throwaway.
- **ARM64 native.** `aarch64-pc-windows-msvc` target; runs natively on the Surface Laptop 7 (Snapdragon X Elite).
- **No new crates.** `embed-resource = "2"` pipeline carried both ICOs as designed.

### Not done (deliberately — Brady owns these before release tag)

- No on-Windows runtime smoke (icon visible in tray, theme swap on dark↔light flip).
- No code signing.
- No MSIX packaging.
- No installer.

### Cross-ref

- Implements verification of: 2026-04-22 "kbblock → switchboard rename" + 2026-04-22 "ICO Embedding + Runtime Theme Swap" (decisions.md).
- Prior step: `cargo xwin check` only (logged in both prior history entries).
- Orchestration log: `.squad/orchestration-log/2026-04-22T23-55-01Z-jerry-release-build.md`.

---

## 2026-04-22: MSIX scaffold + kbblock legacy migration script

**By:** Elaine (Release Engineering). **Date:** 2026-04-22 (later). **Status:** Proposed (scaffold; final signing/MSIX build deferred).

### Context

Brady asked for two things in one round:

1. An MSIX packaging scaffold for the renamed `switchboard` binary so that, when a signing certificate is available, the only remaining step is `MakeAppx pack` + `signtool sign`.
2. A migration script that wipes leftover `kbblock` install state, because the rename changed five process-affecting identifiers and a fresh SwitchBoard install will leave the old autostart entries running in parallel.

### Decisions

#### MSIX identity

- **`Identity.Name = "bradygaster.Switchboard"`** (two-part `publisher.app` form).
  - The bare `Switchboard` form is too generic for the Microsoft Store namespace and risks collision with a third-party reservation. Two-part form keeps the door open for future Brady-published apps and matches Store conventions.
- **`DisplayName = "SwitchBoard"`** — CamelCase per the existing branding decision (decisions.md, 2026-04-22).
- **`Publisher = "CN=Brady Gaster"`** — placeholder. Brady must replace with the exact subject DN of the signing cert byte-for-byte.
- **`Version = "0.1.0.0"`** — MSIX requires 4 parts; Cargo's `0.1.0` plus a build counter.
- **`ProcessorArchitecture = "arm64"`** — single arch, matches `aarch64-pc-windows-msvc` cross-compile output.
- **Capabilities: `runFullTrust` only.** Required because the app installs a `WH_KEYBOARD_LL` hook and toggles a PnP device via SetupAPI — neither permitted from the restricted MSIX AppContainer.

#### Signing strategy

- **Sideload-only for v0.1.** `runFullTrust` requires either sideload or special Microsoft Store partner-onboarding. Brady installs personally on the Surface, so sideload is fine.
- **MSIX-README.md documents two cert paths:** self-signed (sideload testing on Brady's machine) vs. real public-CA cert (any future distribution). Both flows include the `Publisher` ↔ cert subject DN match requirement, which is the most common cause of `MakeAppx` failure.
- **No cert generated in this round.** Out of scope per Brady's task instructions.

#### Asset pipeline

- Reused Elaine's existing `debian:bookworm-slim` + Inkscape + URW base35 fonts container from `.squad/skills/svg-to-ico-docker/SKILL.md`.
- New driver: `scripts/build-msix-assets.sh`. Sibling to `build-icons.sh`, not folded into it — the two scripts have different output sets (.ico vs .png) and different consumers (Win32 resource compiler vs MSIX manifest), and keeping them separate makes it obvious which to re-run after an SVG edit.
- Source SVG: `keycap-s-dark.svg`. The MSIX tile background is set to `transparent`, so theme accent shows through; the dark-glyph variant reads correctly on the default Win 11 dark accent.
- Outputs all six required tile/logo sizes: Square44x44, Square71x71, Square150x150, Square310x310, Wide310x150, StoreLogo (50x50).

### Migration script — idempotency design

`scripts/uninstall-kbblock-legacy.ps1` removes five categories of legacy state:

| Category | Probe | Action |
|---|---|---|
| Running process | `Get-Process -ErrorAction SilentlyContinue` | `Stop-Process -Force` |
| HKCU Run value | `Get-ItemProperty -ErrorAction SilentlyContinue` | `Remove-ItemProperty` |
| Scheduled tasks (×2) | `Get-ScheduledTask -ErrorAction SilentlyContinue` | `Unregister-ScheduledTask` |
| Single-instance mutex | n/a (auto-released on exit) | warning + Explorer-restart hint |
| App-data dirs (×2) | `Test-Path` | `Remove-Item -Recurse` (gated by `-PurgeAppData`) |

Idempotency rules followed:

1. **Probe-then-act.** Every step uses `-ErrorAction SilentlyContinue` on the probe; if absent, prints `not present` in dim text and continues.
2. **No assumptions about prior state.** A second run finds nothing and does nothing. Exit is clean.
3. **Side-effect categorization in the summary.** The closing report distinguishes "removed", "not present", and (for app-data) "preserved" so a re-run shows the same final state regardless of which run actually did the removal.
4. **Auto-elevation.** If launched non-elevated, the script re-launches itself elevated (preferring `pwsh`, falling back to `powershell`). Avoids the "Unregister-ScheduledTask requires admin for SYSTEM tasks" failure mode.
5. **`-WhatIf` honored throughout.** All destructive operations are wrapped in `$PSCmdlet.ShouldProcess(...)`, so dry-run preview is supported.
6. **`-PurgeAppData` is opt-in.** Default behavior preserves `%LOCALAPPDATA%\kbblock\` so post-mortem logs survive. Mentioned explicitly in the summary so users know the option exists.

### Deferred to next round

1. **Signing cert.** Real or self-signed; either way Brady decides + provisions.
2. **`scripts/build-msix.ps1`.** Wraps `MakeAppx pack` + `signtool sign` and writes the staged `Assets\…` tree from the lowercase `assets/msix/` source. Trivial once a cert exists.
3. **End-to-end install + uninstall test on the Surface.** Verifies the `runFullTrust` keyboard hook actually works inside the MSIX container at runtime (theory says yes; needs confirmation).
4. **Distribution decision.** Keep both the bare-exe zip (current `release.ps1`) and the MSIX in parallel for v0.2, or retire the zip path once MSIX is signed and tested.
5. **Optional GHCR-published `Dockerfile.icons`** to skip the ~25 s `apt-get install` cold start if asset regeneration moves into CI.

### Files created / modified

Created:
- `manifest/AppxManifest.xml`
- `manifest/MSIX-README.md`
- `scripts/build-msix-assets.sh`
- `scripts/uninstall-kbblock-legacy.ps1`
- `MIGRATION.md`

Modified:
- `README.md` — added "Upgrading from kbblock?" callout pointing at MIGRATION.md.

### Cross-ref

- `.squad/decisions.md` — 2026-04-22 rename ledger (source of the five legacy identifiers).
- `.squad/skills/svg-to-ico-docker/SKILL.md` — reused for the PNG asset pipeline.
- Orchestration log: `.squad/orchestration-log/2026-04-22T23-55-02Z-elaine-msix-migration.md`.


