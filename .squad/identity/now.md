**Phase:** v0.1 SHIP-READY — Code APPROVED + Release Tooling STAGED.

**Last action:** Elaine released scripts/release.{ps1,sh} + RELEASING.md (2026-04-21). Release workflow established: Windows driver, Linux/macOS parity, operator runbook. Staging pattern finalized. SHA256 sidecar format standardized. Unsigned by design per v0.2 scope.

**Status summary:**
- ✅ device.rs (794 LOC, SetupAPI integration, 3-clause predicate, fail-closed)
- ✅ ble.rs (208 LOC, WinRT BluetoothLEDevice, fresh reads, no cache)
- ✅ main.rs (850 LOC, 6 event handlers, apply_policy, threading model, all 12 invariants enforced)
- ✅ README.md (3,400 words, install, recovery, troubleshooting, safety model)
- ✅ ARCHITECTURE.md (5,200 words, mechanism, predicate, behaviors, threading, 12 invariants, v0.2 roadmap)
- ✅ Owner checklist (31.6 KB, 12 tests fully specified, ready to print and execute)
- ✅ Release scripts (scripts/release.{ps1,sh}, RELEASING.md runbook, staged)
- ⏳ Owner smoke test (todo 5, physical execution on Surface Laptop 7 pending)

**Awaiting:** Owner executes:
1. `.\scripts\build.ps1` → produces kbblock.exe (~100–150 KB)
2. George's owner-execution-checklist.md (12 tests: smoke + adversarial)
3. If PASS: `.\scripts\release.ps1 -Version 0.1.0` → produces zip + SHA256 sidecar
4. Results back to squad for v0.2 planning

**Squad idle** until owner returns with smoke test outcome.

**Project:** bluetooth-keyboard-app — Surface Laptop 7 (Snapdragon X Elite, ARM64) tray app, blocks built-in keyboard when Nuphy Air75 connects. Fail-safe: Escape held 10s.

**Stack:** Rust (windows-rs, tray-icon, tokio), Docker dev-container, cargo-xwin, aarch64-pc-windows-msvc target.
