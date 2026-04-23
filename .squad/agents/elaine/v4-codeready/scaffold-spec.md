# Scaffold Specification — Production Build Environment (v4)

**Author:** Elaine (Build/Toolchain/DevOps)  
**Date:** 2026-04-21  
**Status:** SPECIFICATION — Awaiting owner approval  
**Trigger:** Spike 1 PASSED; toolchain validated for ARM64 Windows cross-compilation

---

## Spike 1 Summary — What We Proved

✅ `cargo-xwin 0.18.4` + `rust:1-bookworm` + `aarch64-pc-windows-msvc` target produces a working 125 KB ARM64 PE (Machine 0xAA64).  
✅ MSVC SDK caches to `.xwin-cache` (~80s first download, ~15s subsequent compiles).  
✅ `windows-rs 0.58.0`, `tray-icon 0.21.3`, SetupAPI, and WinRT DeviceWatcher all work.  
⚠️ **CRITICAL footgun discovered:** cargo-xwin 0.18.4 requires edition2024 → Rust 1.85+. Use `rust:1-bookworm` (rolling), NOT pinned `rust:1.83`.

---

## §1. Repository Layout (LOCKED)

This tree shows the complete repository structure at `cargo new` time. File contents are owned by other agents (Newman for `.rs`, Peterman for docs). This spec defines ONLY the structure.

```
bluetooth-keyboard-app/
├── Cargo.toml                        # Root manifest (§6)
├── Cargo.lock                        # COMMITTED (this is a binary, not library)
├── .cargo/
│   └── config.toml                   # Target default + linker config (§7)
├── src/
│   ├── main.rs                       # Entry point (Newman)
│   ├── device_controller.rs          # SetupAPI calls (Newman)
│   ├── bluetooth_watcher.rs          # BLE DeviceWatcher (Kramer)
│   ├── power_handler.rs              # WM_POWERBROADCAST logic (Newman)
│   ├── shutdown_handler.rs           # WM_QUERYENDSESSION/ENDSESSION (Newman)
│   ├── cold_start.rs                 # Layer 1 invariant (Newman)
│   ├── tray.rs                       # System tray UI (Elaine + Newman)
│   ├── multi_session_guard.rs        # WTSEnumerateSessions check (Newman)
│   ├── config.rs                     # Config file I/O (Newman)
│   └── log.rs                        # tracing setup (Newman)
├── build.rs                          # Manifest + icon embed (§9)
├── app.manifest                      # UAC requireAdministrator (§8)
├── assets/
│   └── icon.png                      # Tray icon (256x256 PNG — owner supplies)
├── docker/
│   └── Dockerfile.build              # Production build container (§2)
├── scripts/
│   ├── build.ps1                     # Windows build driver (§4)
│   ├── build.sh                      # Unix build driver (§4)
│   ├── check.ps1                     # cargo check + clippy runner (§4)
│   ├── check.sh                      # Unix check runner (§4)
│   ├── clean.ps1                     # Remove target/ + xwin cache (§4)
│   └── clean.sh                      # Unix clean runner (§4)
├── .dockerignore                     # Build context exclusions (§5)
├── .gitignore                        # VCS exclusions (§5)
├── .github/
│   └── workflows/
│       └── build.yml                 # CI pipeline (§10)
├── README.md                         # User-facing docs (Peterman)
├── ARCHITECTURE.md                   # Internal design docs (Peterman)
├── LICENSE                           # DECISION REQUIRED: MIT or Apache-2.0 (§11)
└── spike1/                           # Spike 1 artifacts (retained as reference)
    ├── Dockerfile.xwin
    ├── Cargo.toml
    ├── src/
    ├── output/
    └── .xwin-cache/
```

**CRITICAL NOTES:**
- **NO `.devcontainer/` directory.** This project uses a CLI/Docker workflow (one-shot `docker run` builds), NOT VS Code Dev Containers. The build runs in an ephemeral container, not a long-running devcontainer. Clarify this in README.
- Module names are Newman's **likely** choices per v4 §X. Newman owns the final module decomposition; if he renames/splits/merges modules, this spec is updated to match.
- `spike1/` directory is retained as reference material, but `.dockerignore` excludes it from production build contexts.

---

## §2. `Dockerfile.build` — Production Build Container

This is the authoritative build image for the real project. It mirrors `spike1/Dockerfile.xwin` but with production-grade polish.

### Key changes from spike:
- **Base:** `rust:1-bookworm` (rolling — REQUIRED for edition2024 / Rust 1.85+)
- **Pinned versions:** cargo-xwin 0.18.4, explicit LLVM/Clang versions where feasible
- **Cache-friendly layering:** Separate `ADD Cargo.toml` + `cargo fetch`, then `ADD src/`, then build
- **Named volume for xwin cache:** External mount at `/root/.cache/cargo-xwin` (see driver scripts)
- **Output convention:** Artifact lands in `target/aarch64-pc-windows-msvc/release/bluetooth-keyboard-blocker.exe`

```dockerfile
FROM rust:1-bookworm

# Install cross-compile toolchain dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        clang \
        lld \
        llvm \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-xwin (pinned version)
RUN cargo install cargo-xwin --version 0.18.4

# Add ARM64 Windows target
RUN rustup target add aarch64-pc-windows-msvc

# Set xwin cache directory (externally mounted volume)
ENV XWIN_CACHE_DIR=/root/.cache/cargo-xwin
RUN mkdir -p /root/.cache/cargo-xwin

WORKDIR /workspace

# Cache-friendly Rust build layering:
# 1. Copy Cargo manifests first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs

# 2. Pre-fetch dependencies (cached layer if Cargo.toml unchanged)
RUN cargo xwin fetch --target aarch64-pc-windows-msvc || true

# 3. Build dummy binary (populates cargo cache)
RUN cargo xwin build --release --target aarch64-pc-windows-msvc || true

# 4. Remove dummy src (real src comes from volume mount or next ADD)
RUN rm -rf src

# Build command will be invoked by driver scripts
# Default: no-op (driver scripts use `docker run` with explicit command)
CMD ["echo", "Use scripts/build.ps1 or scripts/build.sh to build"]
```

**IMPORTANT:** The final build step is NOT baked into the Dockerfile. Driver scripts mount the workspace and run `cargo xwin build` explicitly. This allows:
- Incremental builds (source changes don't invalidate Docker layers)
- Using the same image for `cargo check`, `cargo clippy`, `cargo test`

---

## §3. `Dockerfile.dev` — DECISION: Same Image

**Recommendation:** Use `Dockerfile.build` for ALL development workflows. No separate dev image.

**Rationale:**
- `cargo check`, `cargo clippy`, `cargo build` — all run in the same container.
- `cargo test` — host-side unit tests (pure Rust logic, no Win32 calls) run in container.
- Integration tests targeting actual Windows behavior (SetupAPI, WinRT, power events) CANNOT run in the container — they require a real Windows ARM64 host (the Surface). These are separate workflows executed on-device.

**Policy:**
- **In-container tests:** Newman's unit tests for config parsing, predicate logic, error handling. Safe to run in Docker.
- **On-device tests:** George's scenario tests (power-state transitions, device re-enumeration after suspend/resume, multi-session checks). Require real Windows.

If owner later wants a dev image with additional tooling (e.g., `cargo-watch`, debuggers), we create `Dockerfile.dev` at that time. For now: **one Dockerfile, multiple uses**.

---

## §4. Driver Scripts

Owner needs both PowerShell (Windows host) and Bash (Linux/Mac host) drivers for cross-platform developer experience.

All scripts use a **NAMED DOCKER VOLUME** `btkb-xwin-cache` for the MSVC SDK cache. This is CRITICAL — without it, every fresh container re-downloads ~80s of SDK.

### `scripts/build.ps1`

```powershell
#!/usr/bin/env pwsh
# Build the production binary inside Docker

$ErrorActionPreference = "Stop"

Write-Host "Building Docker image..." -ForegroundColor Cyan
docker build -f docker/Dockerfile.build -t btkb-build .

Write-Host "`nBuilding ARM64 Windows binary..." -ForegroundColor Cyan
docker run --rm `
    -v "${PWD}:/workspace" `
    -v btkb-xwin-cache:/root/.cache/cargo-xwin `
    btkb-build `
    cargo xwin build --release --target aarch64-pc-windows-msvc

Write-Host "`nBuild complete!" -ForegroundColor Green
Write-Host "Artifact: target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe"

# Sanity check
$artifact = "target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe"
if (Test-Path $artifact) {
    $size = (Get-Item $artifact).Length / 1KB
    Write-Host "Binary size: $([math]::Round($size, 2)) KB" -ForegroundColor Yellow
} else {
    Write-Host "ERROR: Artifact not found!" -ForegroundColor Red
    exit 1
}
```

### `scripts/build.sh`

```bash
#!/usr/bin/env bash
# Build the production binary inside Docker

set -euo pipefail

echo "Building Docker image..."
docker build -f docker/Dockerfile.build -t btkb-build .

echo -e "\nBuilding ARM64 Windows binary..."
docker run --rm \
    -v "$(pwd):/workspace" \
    -v btkb-xwin-cache:/root/.cache/cargo-xwin \
    btkb-build \
    cargo xwin build --release --target aarch64-pc-windows-msvc

echo -e "\nBuild complete!"
echo "Artifact: target/aarch64-pc-windows-msvc/release/bluetooth-keyboard-blocker.exe"

# Sanity check
artifact="target/aarch64-pc-windows-msvc/release/bluetooth-keyboard-blocker.exe"
if [ -f "$artifact" ]; then
    size=$(stat -c%s "$artifact" 2>/dev/null || stat -f%z "$artifact" 2>/dev/null)
    echo "Binary size: $((size / 1024)) KB"
else
    echo "ERROR: Artifact not found!"
    exit 1
fi
```

### `scripts/check.ps1`

```powershell
#!/usr/bin/env pwsh
# Run cargo check + clippy without building artifact

$ErrorActionPreference = "Stop"

Write-Host "Running cargo check..." -ForegroundColor Cyan
docker run --rm `
    -v "${PWD}:/workspace" `
    -v btkb-xwin-cache:/root/.cache/cargo-xwin `
    btkb-build `
    cargo xwin check --target aarch64-pc-windows-msvc

Write-Host "`nRunning cargo clippy..." -ForegroundColor Cyan
docker run --rm `
    -v "${PWD}:/workspace" `
    -v btkb-xwin-cache:/root/.cache/cargo-xwin `
    btkb-build `
    cargo xwin clippy --target aarch64-pc-windows-msvc -- -D warnings

Write-Host "`nAll checks passed!" -ForegroundColor Green
```

### `scripts/check.sh`

```bash
#!/usr/bin/env bash
# Run cargo check + clippy without building artifact

set -euo pipefail

echo "Running cargo check..."
docker run --rm \
    -v "$(pwd):/workspace" \
    -v btkb-xwin-cache:/root/.cache/cargo-xwin \
    btkb-build \
    cargo xwin check --target aarch64-pc-windows-msvc

echo -e "\nRunning cargo clippy..."
docker run --rm \
    -v "$(pwd):/workspace" \
    -v btkb-xwin-cache:/root/.cache/cargo-xwin \
    btkb-build \
    cargo xwin clippy --target aarch64-pc-windows-msvc -- -D warnings

echo -e "\nAll checks passed!"
```

### `scripts/clean.ps1`

```powershell
#!/usr/bin/env pwsh
# Clean build artifacts and xwin cache

$ErrorActionPreference = "Stop"

Write-Host "Removing target/ directory..." -ForegroundColor Cyan
if (Test-Path "target") {
    Remove-Item -Recurse -Force "target"
}

Write-Host "Removing Docker volume btkb-xwin-cache..." -ForegroundColor Cyan
docker volume rm btkb-xwin-cache -f 2>$null

Write-Host "`nClean complete!" -ForegroundColor Green
```

### `scripts/clean.sh`

```bash
#!/usr/bin/env bash
# Clean build artifacts and xwin cache

set -euo pipefail

echo "Removing target/ directory..."
rm -rf target

echo "Removing Docker volume btkb-xwin-cache..."
docker volume rm btkb-xwin-cache -f 2>/dev/null || true

echo -e "\nClean complete!"
```

---

## §5. `.dockerignore` and `.gitignore`

### `.dockerignore`

Exclude unnecessary files from Docker build context (speeds up `docker build`):

```
# Build artifacts
target/
*.exe
*.pdb

# Spike artifacts (retained in repo for reference, but not needed in build)
spike1/

# VCS
.git/
.github/

# Scripts (not needed inside container)
scripts/

# Docs (not needed for build)
README.md
ARCHITECTURE.md
LICENSE

# IDE / Editor
.vscode/
.vs/
.idea/
*.swp
*.swo
*~

# OS
.DS_Store
Thumbs.db
```

### `.gitignore`

Standard Rust + Windows development exclusions:

```
# Build artifacts
/target/
*.exe
*.pdb
*.ilk

# Cargo lock for libraries (we COMMIT it for binaries)
# Cargo.lock  # COMMENTED OUT — we commit Cargo.lock

# xwin SDK cache (downloaded on first build)
.xwin-cache/
/spike1/.xwin-cache/
/spike1/target/
/spike1/output/

# IDE / Editor
.vscode/
!.vscode/settings.json
!.vscode/tasks.json
!.vscode/extensions.json
.vs/
.idea/
*.swp
*.swo
*~

# OS
.DS_Store
Thumbs.db
desktop.ini

# Logs
*.log
```

---

## §6. `Cargo.toml` — Complete Production Manifest

This is the FULL manifest, not minimal. All versions pinned.

```toml
[package]
name = "bluetooth-keyboard-blocker"
version = "0.1.0"
edition = "2024"
authors = ["owner"]
license = "MIT OR Apache-2.0"  # DECISION REQUIRED: pick one (§11)
description = "Disables Surface Laptop internal keyboard when Nuphy Air75 is connected"
repository = "https://github.com/OWNER/REPO"  # DECISION REQUIRED (§11)
readme = "README.md"
keywords = ["bluetooth", "keyboard", "surface", "windows", "tray"]
categories = ["os::windows-apis", "hardware-support"]

[[bin]]
name = "bluetooth-keyboard-blocker"
path = "src/main.rs"

[dependencies]
# Windows APIs (SetupAPI, WinRT, Win32)
windows = { version = "0.58.0", features = [
    "Win32_Devices_DeviceAndDriverInstallation",  # SetupAPI
    "Win32_Devices_HumanInterfaceDevice",         # HID constants
    "Win32_Foundation",                           # BOOL, HWND, HRESULT, etc.
    "Win32_System_Power",                         # PBT_* power events
    "Win32_System_RemoteDesktop",                 # WTSEnumerateSessions
    "Win32_System_Threading",                     # Process/thread APIs
    "Win32_UI_Shell",                             # ShellExecuteW (self-elevate)
    "Win32_UI_WindowsAndMessaging",               # Message loop, WM_* constants
    "Foundation",                                 # WinRT foundation types
    "Devices_Bluetooth",                          # BluetoothLEDevice
    "Devices_Enumeration",                        # DeviceWatcher, DeviceInformation
] }
windows-core = "0.58.0"  # HSTRING, GUID, etc.

# System tray
tray-icon = "0.21.3"

# Event loop (pairs with tray-icon)
tao = "0.30.9"

# Configuration file parsing
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# Logging (Newman/George demand diagnostic logs)
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "time"] }
tracing-appender = "0.2"  # File-based log rotation

# Error handling
thiserror = "2.0"  # Typed errors at module boundaries
anyhow = "1.0"     # Context-rich errors in main.rs

# Utilities
directories = "5.0"       # %APPDATA% path resolution
parking_lot = "0.12"      # Fast Mutex (no poisoning)

[build-dependencies]
embed-resource = "3.1"  # Embed manifest + icon

[profile.release]
opt-level = "z"          # Optimize for size
lto = "thin"             # Thin LTO (NOT fat — cargo-xwin link times suffer with fat LTO on ARM64)
codegen-units = 1        # Single codegen unit for best optimization
strip = true             # Strip debug symbols
panic = "abort"          # No unwinding
```

**NOTES:**
- `edition = "2024"` — REQUIRED per Spike 1 footgun. cargo-xwin 0.18.4 demands Rust 1.85+.
- `lto = "thin"` — NOT "fat". Fat LTO hurts cargo-xwin link times on ARM64 (observed in Spike 1).
- `license = "MIT OR Apache-2.0"` — Dual-license is Rust ecosystem standard. Owner can restrict to one (§11).
- `repository` URL — Placeholder; owner must provide (§11).
- Dependency versions are EXACT PINS from Spike 1 validation.

---

## §7. `.cargo/config.toml`

Set default target and linker hints for `aarch64-pc-windows-msvc`.

```toml
[build]
target = "aarch64-pc-windows-msvc"

[target.aarch64-pc-windows-msvc]
linker = "rust-lld"
```

**NOTES:**
- `linker = "rust-lld"` — cargo-xwin automatically configures this, but explicit is better.
- Developers can override target with `--target x86_64-pc-windows-msvc` for local Windows x64 testing if needed (not primary workflow).

---

## §8. `app.manifest` — UAC + DPI Awareness

UAC elevation is REQUIRED per v4 design (SetupAPI device disable needs admin). DPI awareness ensures tray icon renders correctly on high-DPI displays.

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
    version="1.0.0.0"
    processorArchitecture="*"
    name="BluetoothKeyboardBlocker"
    type="win32"
  />
  <description>Bluetooth Keyboard Blocker</description>

  <!-- UAC: Require Administrator -->
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>

  <!-- DPI Awareness -->
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>

  <!-- Windows 11 Compatibility -->
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 / 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
</assembly>
```

---

## §9. `build.rs` — Embed Manifest + Tray Icon

Use `embed-resource` crate to embed `app.manifest` and tray icon into the binary.

```rust
fn main() {
    // Only run on Windows targets
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        return;
    }

    // Embed app.manifest (UAC + DPI awareness)
    embed_resource::compile("app.manifest", embed_resource::NONE);

    // Embed tray icon (if exists)
    if std::path::Path::new("assets/icon.png").exists() {
        // Note: tray-icon loads from filesystem at runtime (no embed needed)
        // This is a placeholder for future icon resource embedding if we convert to .ico
        println!("cargo:rerun-if-changed=assets/icon.png");
    }

    // Rerun if manifest changes
    println!("cargo:rerun-if-changed=app.manifest");
}
```

**NOTES:**
- `embed-resource` handles Windows resource compilation via `rc.exe` (MSVC) or `windres` (MinGW). cargo-xwin provides the MSVC tooling.
- Tray icon: `tray-icon` crate loads PNG at runtime. If we later need a .ico file embedded as a resource, this build script is updated.

---

## §10. CI Hook — `.github/workflows/build.yml`

GitHub Actions workflow for automated builds on push/PR. Runs on `ubuntu-latest` (cross-compiles from Linux, no Windows runner needed).

```yaml
name: Build

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    name: Build ARM64 Windows Binary
    runs-on: ubuntu-latest

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Cache Docker layers
        uses: actions/cache@v4
        with:
          path: /tmp/.buildx-cache
          key: ${{ runner.os }}-buildx-${{ github.sha }}
          restore-keys: |
            ${{ runner.os }}-buildx-

      - name: Cache xwin SDK
        uses: actions/cache@v4
        with:
          path: .xwin-cache
          key: ${{ runner.os }}-xwin-${{ hashFiles('Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-xwin-

      - name: Build Docker image
        run: docker build -f docker/Dockerfile.build -t btkb-build .

      - name: Run cargo check
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/workspace" \
            -v "${{ github.workspace }}/.xwin-cache:/root/.cache/cargo-xwin" \
            btkb-build \
            cargo xwin check --target aarch64-pc-windows-msvc

      - name: Run cargo clippy
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/workspace" \
            -v "${{ github.workspace }}/.xwin-cache:/root/.cache/cargo-xwin" \
            btkb-build \
            cargo xwin clippy --target aarch64-pc-windows-msvc -- -D warnings

      - name: Run cargo test
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/workspace" \
            -v "${{ github.workspace }}/.xwin-cache:/root/.cache/cargo-xwin" \
            btkb-build \
            cargo xwin test --target aarch64-pc-windows-msvc --lib --bins

      - name: Build release binary
        run: |
          docker run --rm \
            -v "${{ github.workspace }}:/workspace" \
            -v "${{ github.workspace }}/.xwin-cache:/root/.cache/cargo-xwin" \
            btkb-build \
            cargo xwin build --release --target aarch64-pc-windows-msvc

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: bluetooth-keyboard-blocker-arm64
          path: target/aarch64-pc-windows-msvc/release/bluetooth-keyboard-blocker.exe
          if-no-files-found: error

      - name: Verify artifact
        run: |
          ls -lh target/aarch64-pc-windows-msvc/release/bluetooth-keyboard-blocker.exe
          file target/aarch64-pc-windows-msvc/release/bluetooth-keyboard-blocker.exe || true
```

**NOTES:**
- Caches xwin SDK between runs (saves ~80s per build).
- Runs `cargo check`, `cargo clippy`, `cargo test` (unit tests only — integration tests require Windows).
- Uploads `.exe` as a workflow artifact (downloadable from GitHub Actions UI).
- **Signing/release:** Deferred (§11). Owner can add code-signing step later (requires signing certificate + `signtool`).

---

## §11. Owner Decisions Still Required

Before implementation begins, owner must decide:

| # | Decision | Recommendation | Rationale |
|---|----------|----------------|-----------|
| 1 | **License** | MIT OR Apache-2.0 (dual-license) | Rust ecosystem standard; maximum compatibility |
| 2 | **Repository URL** | (owner provides) | For Cargo.toml `repository` field |
| 3 | **Crate name confirmation** | `bluetooth-keyboard-blocker` | Descriptive; follows Rust naming conventions (lowercase + hyphens) |
| 4 | **App display name** | "Bluetooth Keyboard Blocker" | For UAC prompt, tray tooltip, About dialog |
| 5 | **Tray icon** | Owner supplies 256x256 PNG at `assets/icon.png` | Placeholder acceptable for initial build; real icon before v1.0 |

**Tray icon placeholder:** If owner doesn't have an icon yet, use a 256x256 PNG of a keyboard with a Bluetooth symbol. `tray-icon` will render it. Owner can replace later without code changes.

---

## §12. Build Verification Protocol

After first successful `scripts/build.ps1` run, owner executes this checklist ON THE SURFACE LAPTOP 7 (ARM64 Windows):

### ✅ Checklist

1. **Artifact exists**
   ```powershell
   Test-Path target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe
   ```
   Expected: `True`

2. **File size sanity**
   ```powershell
   (Get-Item target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe).Length / 1MB
   ```
   Expected: **< 5 MB** (likely 500 KB - 2 MB depending on dependencies)

3. **Machine type verification** (confirm ARM64 PE)
   ```powershell
   dumpbin /headers target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe | Select-String "machine"
   ```
   Expected output: `AA64 machine (ARM64)`
   
   **Alternative** (if `dumpbin` not available):
   ```powershell
   $bytes = [System.IO.File]::ReadAllBytes("target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe")
   $machine = [BitConverter]::ToUInt16($bytes, 0x3C + 4)
   if ($machine -eq 0xAA64) { "✓ ARM64" } else { "✗ Wrong architecture: 0x$($machine.ToString('X4'))" }
   ```

4. **Code signing status** (expected to fail until signing is wired)
   ```powershell
   Get-AuthenticodeSignature target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe
   ```
   Expected: `NotSigned` — **THIS IS NORMAL FOR NOW.** Signing is deferred (§11).

5. **Run on Surface**
   ```powershell
   .\target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe
   ```
   Expected:
   - UAC prompt appears (requireAdministrator)
   - After elevation: tray icon appears in system tray
   - No missing-DLL errors
   - App runs for at least 30 seconds without crashing

6. **Missing DLL check** (if app fails to launch)
   ```powershell
   dumpbin /dependents target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe | Select-String ".dll"
   ```
   Expected: Only system DLLs (KERNEL32.dll, USER32.dll, etc.). No external dependencies.

7. **Log file verification**
   ```powershell
   Get-ChildItem $env:APPDATA\bluetooth-keyboard-blocker\logs\ -Recurse
   ```
   Expected: Log files created (if `tracing-appender` is configured to write logs).

---

## §13. Implementation Workflow

This is the EXACT order of operations for standing up the production scaffold:

### Phase 1: Scaffold Files (Elaine + Newman)
1. Owner runs `cargo new --bin bluetooth-keyboard-blocker` in `~\Code\`
2. Elaine commits: `Cargo.toml` (§6), `.cargo/config.toml` (§7), `app.manifest` (§8), `build.rs` (§9), `docker/Dockerfile.build` (§2), all `scripts/*.ps1` and `scripts/*.sh` (§4), `.dockerignore` (§5), `.gitignore` (§5), `.github/workflows/build.yml` (§10)
3. Newman creates stub `src/*.rs` files with `pub fn placeholder() {}` in each (no logic yet — just file structure)
4. Owner creates `assets/icon.png` (placeholder: any 256x256 PNG)
5. Owner creates `README.md` stub, `ARCHITECTURE.md` stub, `LICENSE` file

### Phase 2: First Build (Elaine validates)
1. Run `scripts\build.ps1` from repo root
2. Observe: Docker image builds (~5 min first time), xwin downloads SDK (~80s), binary compiles (~15s)
3. Verify: Artifact exists at `target\aarch64-pc-windows-msvc\release\bluetooth-keyboard-blocker.exe`
4. Run §12 verification checklist on Surface

### Phase 3: CI Validation (Elaine)
1. Push to GitHub
2. Observe: GitHub Actions workflow triggers, runs `cargo check` + `clippy` + `test` + `build`
3. Verify: Workflow artifact uploads successfully

### Phase 4: Newman's Module Implementation
1. Newman implements `.rs` files per v4 §X module decomposition
2. After each module: run `scripts\check.ps1` (cargo check + clippy)
3. After integration: run `scripts\build.ps1`, test on Surface

---

## §14. Deferred Items (Not in v1 Scaffold)

These are NOT BLOCKING for initial scaffold. Owner can add later:

| Item | When to add | Owner |
|------|-------------|-------|
| Code signing | Before v1.0 release | Elaine + owner |
| Release automation (GitHub Releases + auto-tagging) | After first manual release | Elaine |
| MSIX packaging | If owner wants Microsoft Store distribution | Elaine |
| Multi-architecture CI matrix (x64 + ARM64) | If owner wants x64 Windows support | Elaine |
| Benchmark suite | After Newman's implementation, if perf concerns arise | George |
| Fuzz testing for predicate logic | After Newman's §AC implementation | George |

---

## §15. Known Constraints & Limitations

1. **edition2024 footgun:** Rust 1.85+ REQUIRED. Pinning to `rust:1.83-bookworm` WILL FAIL with cargo-xwin 0.18.4. Use `rust:1-bookworm` (rolling) or manually pin to `rust:1.85-bookworm` once available.

2. **xwin SDK download time:** First build in a fresh container downloads ~500 MB of MSVC SDK (~80s on typical broadband). Subsequent builds use cached volume (~15s compile time). **CI caching CRITICAL** — without it, every CI run re-downloads.

3. **Integration tests on Surface only:** `cargo test` in CI runs unit tests only. Integration tests (SetupAPI, WinRT, power events) MUST run on real ARM64 Windows — no CI automation for those until we have ARM64 Windows GitHub-hosted runners (not available as of 2026-04-21).

4. **No cross-platform dev** for Windows-specific code: Docker provides Rust toolchain, but you can't F5-debug Windows APIs in VS Code on Linux. Owner must copy `.exe` to Surface for manual testing.

---

## §16. Elaine's Handoff Notes

**To Newman:** Module stubs in `src/*.rs` are yours. File names match v4 §X; contents are empty. Implement per your spec. Elaine owns `tray.rs` UI logic (menu items, event handlers) — coordinate on that one.

**To Kramer:** `bluetooth_watcher.rs` is yours. Implement DeviceWatcher per v4 §AD. Elaine's scaffold assumes you'll use `windows::Devices::Enumeration::DeviceWatcher` — if you switch mechanisms, update `Cargo.toml` features.

**To George:** After Newman's §AC implementation, your scenario tests (see v4 decisions.md §AB–§AK) run ON THE SURFACE, not in CI. Elaine can add a `scripts/test-on-device.ps1` if you need automation, but it's a manual workflow for now.

**To Peterman:** `README.md`, `ARCHITECTURE.md`, `LICENSE` are yours. README must document: (1) Docker-only dev workflow, (2) NO local Rust install needed, (3) Build instructions (`scripts\build.ps1`), (4) §12 verification checklist.

**To Owner:** After Phase 2 completes, you have a working ARM64 binary that does nothing (stubs only). This is INTENTIONAL — Newman's implementation follows. The scaffold's job is proving the toolchain; Newman's job is proving the logic.

---

## §17. Success Criteria

This scaffold is DONE when:

✅ `scripts\build.ps1` produces a working ARM64 PE (Machine 0xAA64)  
✅ Binary runs on Surface without DLL errors  
✅ Tray icon appears  
✅ GitHub Actions CI passes (check + clippy + test + build)  
✅ Binary size < 5 MB  
✅ xwin cache persists between builds (15s incremental compile time)  
✅ Newman has empty `.rs` files to implement in  
✅ Owner has §12 verification checklist results logged  

---

**END OF SPEC**

---

## Appendix A: File Count Summary

For owner's sanity check after scaffold setup:

| Category | Count | Files |
|----------|-------|-------|
| Rust source | 10 | main.rs + 9 modules |
| Build system | 3 | Cargo.toml, Cargo.lock, .cargo/config.toml |
| Docker | 1 | docker/Dockerfile.build |
| Scripts | 6 | build.ps1/.sh, check.ps1/.sh, clean.ps1/.sh |
| Assets | 1 | assets/icon.png |
| Config | 3 | app.manifest, build.rs, .dockerignore, .gitignore |
| CI/CD | 1 | .github/workflows/build.yml |
| Docs | 3 | README.md, ARCHITECTURE.md, LICENSE |
| **TOTAL** | **28** | (excluding spike1/ reference files) |

---

**Elaine out.**
