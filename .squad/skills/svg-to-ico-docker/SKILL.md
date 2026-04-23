# Skill: SVG → multi-resolution `.ico` via Docker (zero host installs)

**When to use:** You have an SVG and need a Windows `.ico` containing several
resolutions (typically 16, 20, 24, 32, 48, 256) and you can't install Inkscape
or ImageMagick on the host. Works on Windows ARM64 (Surface Laptop 7).

**Don't use for:** macOS `.icns`, animated icons, or SVGs that depend on
licensed fonts you can't substitute with a metric-equivalent.

## Recipe

One container, one script. Mount the repo, run a Bash driver.

### `scripts/build-icons.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
    inkscape imagemagick fonts-urw-base35 ca-certificates >/dev/null

SIZES=(16 20 24 32 48 256)
mkdir -p /tmp/icons
cd /work

for variant in light dark; do
    src="path/to/source-${variant}.svg"
    work="/tmp/icons/source-${variant}.svg"
    # Optional: substitute font-family if the SVG references a non-free font.
    # Example: swap 'Palatino Linotype' → P052 (URW Palatino-equivalent).
    sed "s|'Palatino Linotype'|P052|" "$src" > "$work"

    for size in "${SIZES[@]}"; do
        inkscape --export-type=png \
                 --export-width=$size --export-height=$size \
                 --export-filename="/tmp/icons/${variant}-${size}.png" \
                 "$work" >/dev/null 2>&1
    done

    # ImageMagick v6 on Debian bookworm: command is `convert`, not `magick`.
    convert \
        "/tmp/icons/${variant}-16.png"  "/tmp/icons/${variant}-20.png" \
        "/tmp/icons/${variant}-24.png"  "/tmp/icons/${variant}-32.png" \
        "/tmp/icons/${variant}-48.png"  "/tmp/icons/${variant}-256.png" \
        "assets/icons/output-${variant}.ico"
done

for f in assets/icons/*.ico; do identify "$f"; done
```

### Run from PowerShell

```powershell
docker run --rm -v "${PWD}:/work" -w /work debian:bookworm-slim `
    bash /work/scripts/build-icons.sh
```

## Why these choices

- **`debian:bookworm-slim`** — has both `linux/amd64` and `linux/arm64`
  manifests, so Docker on ARM64 Windows pulls native arm64 (no emulation).
- **Inkscape for SVG→PNG** — handles complex SVG features (gradients, strokes,
  text on path) better than `rsvg-convert`, especially at small target sizes.
- **ImageMagick `convert` for PNG→ICO** — correct multi-res ICO container
  encoding in one command.
- **`fonts-urw-base35`** — provides GPL metric-equivalents to the 35 PostScript
  base fonts: P052 ≈ Palatino, NimbusRoman ≈ Times, NimbusSans ≈ Helvetica,
  C059 ≈ Century Schoolbook, URWGothic ≈ ITC Avant Garde. Use this package any
  time your SVG references a proprietary font you can't ship.

## Gotchas

1. **CRLF line endings.** If you author the script on Windows, convert to LF
   before mounting. Otherwise Bash inside the container will fail with
   `set: pipefail: invalid option name`. PowerShell one-liner:
   `$c = Get-Content -Raw script.sh; [IO.File]::WriteAllText("$PWD\script.sh", ($c -replace "`r`n","`n"))`.
2. **`magick` vs `convert`.** ImageMagick v6 (Debian bookworm) uses `convert`.
   v7 (newer images) uses `magick`. Pick one and pin the base image.
3. **Font fallback is silent.** If the requested font isn't available, browsers
   and Inkscape silently fall back — you won't get an error, just a wrong
   glyph. Always verify the rendered PNG visually for any glyph that depends
   on a specific font.
4. **ImageMagick SVG policy.** Modern Debian disables MSVG/RSVG delegates in
   ImageMagick's `policy.xml` for security. We sidestep this by using Inkscape
   for the rasterization step; ImageMagick only sees PNGs.
5. **Cold start.** `apt-get install` adds ~25 s per run. For CI, bake a
   small `Dockerfile` extending `debian:bookworm-slim` with the three packages
   and push to a registry.

## Verification

```bash
identify assets/icons/output-dark.ico
# Should print one line per contained resolution, e.g.:
#   output-dark.ico[0] ICO 16x16 16x16+0+0 8-bit sRGB ...
#   output-dark.ico[1] ICO 20x20 20x20+0+0 8-bit sRGB ...
```

If a size is missing, the most common cause is an Inkscape render that
produced a 0-byte PNG (font missing, SVG invalid). Re-run with the
`>/dev/null 2>&1` redirect removed to see Inkscape's actual error.
