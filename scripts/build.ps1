$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot | Split-Path -Parent

# ═══════════════════════════════════════════════════════════════════
# Build Docker image (needed for both gates and cross-compile)
# ═══════════════════════════════════════════════════════════════════

Write-Host "=== Building Docker image ===" -ForegroundColor Cyan
docker build -t switchboard:build -f $root/docker/Dockerfile.build $root

# ═══════════════════════════════════════════════════════════════════
# Quality gates in Docker (Jerry's test architecture Tier (a))
# ═══════════════════════════════════════════════════════════════════

$cwd = $root -replace '\\','/'

Write-Host "=== Running quality gates in Docker ===" -ForegroundColor Cyan

# Build command based on SWITCHBOARD_SKIP_TESTS
if ($env:SWITCHBOARD_SKIP_TESTS -eq '1') {
    Write-Host "⚠️  WARNING: Skipping cargo test (SWITCHBOARD_SKIP_TESTS=1)" -ForegroundColor Yellow
    $gateCmd = "cargo fmt --check && cargo xwin clippy --target x86_64-pc-windows-msvc"
} else {
    # Strategy C: Compile tests without running (wine doesn't work in container)
    $gateCmd = "cargo fmt --check && cargo xwin clippy --target x86_64-pc-windows-msvc && cargo xwin build --tests --target x86_64-pc-windows-msvc"
}

docker run --rm `
  -v "${cwd}:/build" `
  -v "${cwd}/.xwin-cache:/xwin-cache" `
  -w /build `
  switchboard:build `
  bash -c "$gateCmd"

if ($LASTEXITCODE -ne 0) {
    throw "Quality gates failed in Docker"
}

Write-Host "=== Quality gates passed — starting cross-compile build ===" -ForegroundColor Green

# ═══════════════════════════════════════════════════════════════════
# Docker cross-compile for aarch64-pc-windows-msvc
# ═══════════════════════════════════════════════════════════════════

docker run --rm `
  -v "${cwd}:/build" `
  -v "${cwd}/dist:/dist" `
  -v "${cwd}/.xwin-cache:/xwin-cache" `
  switchboard:build
Write-Host "Built: $root\dist\switchboard.exe"