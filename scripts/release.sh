#!/bin/bash
set -euo pipefail

version=""
skip_build=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            version="$2"
            shift 2
            ;;
        --skip-build)
            skip_build=true
            shift
            ;;
        *)
            echo "Usage: $0 --version X.Y.Z [--skip-build]"
            exit 1
            ;;
    esac
done

if [[ -z "$version" ]]; then
    echo "Error: --version is required"
    echo "Usage: $0 --version X.Y.Z [--skip-build]"
    exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Build step if not skipped
if [[ "$skip_build" != "true" ]]; then
    echo "Building..."
    "$root/scripts/build.sh"
fi

# Verify exe exists and is non-empty
exe_path="$root/dist/switchboard.exe"
if [[ ! -f "$exe_path" ]]; then
    echo "Error: Exe not found: $exe_path. Run './scripts/build.sh' first."
    exit 1
fi

exe_size=$(stat -f%z "$exe_path" 2>/dev/null || stat -c%s "$exe_path" 2>/dev/null || echo "0")
if [[ "$exe_size" -eq 0 ]]; then
    echo "Error: Exe is empty: $exe_path"
    exit 1
fi

# Parse Cargo.toml version and verify match
cargo_path="$root/Cargo.toml"
cargo_version=$(grep '^version\s*=' "$cargo_path" | head -1 | sed 's/^version\s*=\s*"\([^"]*\)".*/\1/')

if [[ "$cargo_version" != "$version" ]]; then
    echo "Error: Version mismatch: Cargo.toml has '$cargo_version' but you passed '--version $version'"
    exit 1
fi

# Display exe info
exe_size_kb=$(( (exe_size + 1023) / 1024 ))
echo "switchboard.exe — $exe_size bytes ($exe_size_kb KB)"

# Compute SHA256 of exe
exe_sha256=$(sha256sum "$exe_path" | cut -d' ' -f1)

# Stage release directory
release_dir="$root/dist/switchboard-v$version-aarch64-pc-windows-msvc"
if [[ -d "$release_dir" ]]; then
    rm -rf "$release_dir"
fi
mkdir -p "$release_dir"

# Copy exe and docs
cp "$exe_path" "$release_dir/"
cp "$root/README.md" "$release_dir/"
cp "$root/ARCHITECTURE.md" "$release_dir/"

# Copy .env.example so users know how to configure SWITCHBOARD_NUPHY_BD_ADDR.
# Required: without it, the app launches but BLE never subscribes (silent
# regression where Nuphy connection state is never tracked).
if [[ ! -f "$root/.env.example" ]]; then
    echo "Error: .env.example not found at repo root: $root/.env.example. Cannot package release."
    exit 1
fi
cp "$root/.env.example" "$release_dir/"

# Copy LICENSE — required, this repo ships under MIT
if [[ ! -f "$root/LICENSE" ]]; then
    echo "Error: LICENSE not found at repo root: $root/LICENSE. Cannot package release."
    exit 1
fi
cp "$root/LICENSE" "$release_dir/"

# Create zip
zip_path="$root/dist/switchboard-v$version-aarch64-pc-windows-msvc.zip"
cd "$root/dist"
zip -r -q "switchboard-v$version-aarch64-pc-windows-msvc.zip" "switchboard-v$version-aarch64-pc-windows-msvc"
cd - > /dev/null

# Compute SHA256 of zip
zip_sha256=$(sha256sum "$zip_path" | cut -d' ' -f1)

# Write SHA256 sidecar in standard format (hash  filename)
sha256_file="$zip_path.sha256"
echo "$zip_sha256  switchboard-v$version-aarch64-pc-windows-msvc.zip" > "$sha256_file"

# Echo final manifest
echo ""
echo "Release packaged:"
echo "  File:   $(basename "$zip_path")"
echo "  Size:   $(stat -f%z "$zip_path" 2>/dev/null || stat -c%s "$zip_path" 2>/dev/null) bytes"
echo "  SHA256: $zip_sha256"
echo ""
echo "Sidecar: $(basename "$sha256_file")"
echo "  Verify with: sha256sum -c $sha256_file"
