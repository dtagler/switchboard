#!/usr/bin/env bash
# build-icons.sh — Run inside debian:bookworm-slim container with /work mounted to repo root.
# Generates switchboard-{light,dark,}.ico from the Soft Accent SVGs.
set -euo pipefail

echo "==> Installing toolchain (inkscape, imagemagick, URW base35 fonts)..."
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
    inkscape imagemagick fonts-urw-base35 ca-certificates >/dev/null

echo "==> Verifying P052 (Palatino-equivalent) is present..."
fc-list | grep -i "p052\|palladio" | head -3 || { echo "P052 font missing"; exit 1; }

mkdir -p /tmp/icons
cd /work

SIZES=(16 20 24 32 48 256)

for variant in light dark; do
    src=".squad/files/icon-concepts/keycap-s-${variant}.svg"
    work="/tmp/icons/keycap-s-${variant}.svg"
    # Substitute font-family so P052 (Palatino metric clone) is first; this avoids
    # silent fallback to a sans-serif when Palatino Linotype isn't installed.
    sed "s|'Palatino Linotype',Palatino,'Book Antiqua',serif|P052,'URW Palladio L','Palatino Linotype',Palatino,'Book Antiqua',serif|" "$src" > "$work"

    echo "==> Rendering $variant PNGs..."
    for size in "${SIZES[@]}"; do
        inkscape --export-type=png \
                 --export-width=$size --export-height=$size \
                 --export-filename="/tmp/icons/${variant}-${size}.png" \
                 "$work" >/dev/null 2>&1
    done

    echo "==> Assembling switchboard-${variant}.ico..."
    convert \
        "/tmp/icons/${variant}-16.png" \
        "/tmp/icons/${variant}-20.png" \
        "/tmp/icons/${variant}-24.png" \
        "/tmp/icons/${variant}-32.png" \
        "/tmp/icons/${variant}-48.png" \
        "/tmp/icons/${variant}-256.png" \
        "assets/icons/switchboard-${variant}.ico"
done

echo "==> Copying preview PNGs..."
for variant in light dark; do
    for size in 16 32 256; do
        cp "/tmp/icons/${variant}-${size}.png" \
           ".squad/files/icon-concepts/preview-png/keycap-s-${variant}-${size}.png"
    done
done

echo "==> Verifying outputs..."
for f in assets/icons/switchboard-light.ico assets/icons/switchboard-dark.ico; do
    echo "--- $f ---"
    ls -la "$f"
    identify "$f" | sed 's|^.*\(\[.*\]\).*\( [0-9]*x[0-9]*\) .*|  \1\2|'
done

echo "==> Done."
