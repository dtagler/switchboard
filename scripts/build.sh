#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== Building Docker image ==="
docker build -t switchboard:build -f "$root/docker/Dockerfile.build" "$root"

# Quality gates in the same container, mirroring scripts/build.ps1.
# Strategy C: compile tests for x86_64 (wine can't run them in-container; the
# real run happens on a Windows ARM64 host). Set SWITCHBOARD_SKIP_TESTS=1 to
# skip the test build only.
if [[ "${SWITCHBOARD_SKIP_TESTS:-0}" == "1" ]]; then
  echo "⚠️  Skipping cargo test build (SWITCHBOARD_SKIP_TESTS=1)"
  gate_cmd="cargo fmt --check && cargo xwin clippy --target x86_64-pc-windows-msvc"
else
  gate_cmd="cargo fmt --check && cargo xwin clippy --target x86_64-pc-windows-msvc && cargo xwin build --tests --target x86_64-pc-windows-msvc"
fi

echo "=== Running quality gates in Docker ==="
docker run --rm \
  -v "$root:/build" \
  -v "$root/.xwin-cache:/xwin-cache" \
  -w /build \
  switchboard:build \
  bash -c "$gate_cmd"

echo "=== Quality gates passed — starting cross-compile build ==="
docker run --rm \
  -v "$root:/build" \
  -v "$root/dist:/dist" \
  -v "$root/.xwin-cache:/xwin-cache" \
  switchboard:build
echo "Built: $root/dist/switchboard.exe"
