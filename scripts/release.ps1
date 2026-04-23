param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    
    [switch]$SkipBuild = $false
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = $PSScriptRoot | Split-Path -Parent

# Build step if not skipped
if (-not $SkipBuild) {
    Write-Host "Building..."
    & "$root\scripts\build.ps1"
}

# Verify exe exists and is non-empty
$exePath = Join-Path $root "dist\switchboard.exe"
if (-not (Test-Path $exePath)) {
    Write-Error "Exe not found: $exePath. Run '.\scripts\build.ps1' first."
    exit 1
}

$exeSize = (Get-Item $exePath).Length
if ($exeSize -eq 0) {
    Write-Error "Exe is empty: $exePath"
    exit 1
}

# Parse Cargo.toml version and verify match
$cargoPath = Join-Path $root "Cargo.toml"
$cargoContent = Get-Content $cargoPath -Raw
$versionMatch = $cargoContent | Select-String 'version\s*=\s*"([^"]+)"'
if (-not $versionMatch) {
    Write-Error "Could not parse version from Cargo.toml"
    exit 1
}
$cargoVersion = $versionMatch.Matches[0].Groups[1].Value

if ($cargoVersion -ne $Version) {
    Write-Error "Version mismatch: Cargo.toml has '$cargoVersion' but you passed '-Version $Version'"
    exit 1
}

# Display exe info
$exeSizeKB = [math]::Ceiling($exeSize / 1024)
Write-Host "switchboard.exe — $exeSize bytes ($exeSizeKB KB)"

# Compute SHA256 of exe
$exeSHA256 = (Get-FileHash -Path $exePath -Algorithm SHA256).Hash

# Stage release directory
$releaseDir = Join-Path $root "dist\switchboard-v$Version-aarch64-pc-windows-msvc"
if (Test-Path $releaseDir) {
    Remove-Item -Recurse -Force $releaseDir
}
New-Item -ItemType Directory -Path $releaseDir -ErrorAction Stop | Out-Null

# Copy exe
Copy-Item -Path $exePath -Destination $releaseDir

# Copy documentation
Copy-Item -Path (Join-Path $root "README.md") -Destination $releaseDir
Copy-Item -Path (Join-Path $root "ARCHITECTURE.md") -Destination $releaseDir

# Copy .env.example so users know how to configure SWITCHBOARD_NUPHY_BD_ADDR
# (without it, BLE never subscribes — see README "Configure your keyboard's
# Bluetooth address"). Required, not optional: a missing .env.example would
# silently reproduce the regression where the app launches but never tracks
# Nuphy connection state.
$envExamplePath = Join-Path $root ".env.example"
if (-not (Test-Path $envExamplePath)) {
    Write-Error ".env.example not found at repo root: $envExamplePath. Cannot package release."
    exit 1
}
Copy-Item -Path $envExamplePath -Destination $releaseDir

# Copy LICENSE — required, this repo ships under MIT
$licensePath = Join-Path $root "LICENSE"
if (-not (Test-Path $licensePath)) {
    Write-Error "LICENSE not found at repo root: $licensePath. Cannot package release."
    exit 1
}
Copy-Item -Path $licensePath -Destination $releaseDir

# Create zip
$zipPath = Join-Path $root "dist\switchboard-v$Version-aarch64-pc-windows-msvc.zip"
Compress-Archive -Path $releaseDir -DestinationPath $zipPath -Force

# Compute SHA256 of zip
$zipSHA256 = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash

# Write SHA256 sidecar in standard format (hash  filename)
$sha256File = "$zipPath.sha256"
$sha256Content = "$zipSHA256  switchboard-v$Version-aarch64-pc-windows-msvc.zip"
Set-Content -Path $sha256File -Value $sha256Content -NoNewline

# Echo final manifest
Write-Host ""
Write-Host "Release packaged:"
Write-Host "  File:   $(Split-Path $zipPath -Leaf)"
Write-Host "  Size:   $((Get-Item $zipPath).Length) bytes"
Write-Host "  SHA256: $zipSHA256"
Write-Host ""
Write-Host "Sidecar: $(Split-Path $sha256File -Leaf)"
Write-Host "  Verify with: Get-FileHash -Path switchboard-v$Version-aarch64-pc-windows-msvc.zip -Algorithm SHA256 | Compare to above"
