# SwitchBoard — Copilot instructions

A Windows ARM64 tray app (Surface Laptop 7 only) that disables the laptop's internal keyboard while a Nuphy Bluetooth keyboard is connected. Single binary, no installer. v0.1, MIT, unsigned, repo: <https://github.com/dtagler/switchboard>.

## Build, test, lint

The build runs **inside Docker via `cargo-xwin`** because the target is `aarch64-pc-windows-msvc` and the host may be anything. Do not invoke `cargo` directly on the host.

| Task | Command |
|---|---|
| Full build (gates + cross-compile → `dist/switchboard.exe`) | `.\scripts\build.ps1` |
| Apply formatting in container | `docker run --rm -v "${PWD}:/build" -w /build switchboard:build cargo fmt` |
| Skip the test-compile gate | `$env:SWITCHBOARD_SKIP_TESTS = '1'; .\scripts\build.ps1` |
| Stage a release zip | `.\scripts\dist.ps1` (requires `.env.example` at repo root) |

`scripts/build.ps1` runs (in this order, all in Docker): `cargo fmt --check` → `cargo xwin clippy --target x86_64-pc-windows-msvc` → `cargo xwin build --tests --target x86_64-pc-windows-msvc` → release `cargo xwin build --target aarch64-pc-windows-msvc`. **A failing `cargo fmt --check` fails the entire build** — run `cargo fmt` in the container first. Tests are **compiled but not executed** in the container (Wine can't run them on the WinAPI surface); to actually run them, do `cargo test --target aarch64-pc-windows-msvc <name>` on a Windows ARM64 host.

## Architecture (`ARCHITECTURE.md` is authoritative)

**Two threads:**
- **Message-loop thread** (`main.rs`) owns all state and makes all *decisions*. Never calls SetupAPI directly except: Quit fallback (after worker join timeout), `--recover` argv path, and `shutdown_cleanup()` (registered via `SetConsoleCtrlHandler` + atexit).
- **Worker thread** (`worker_thread_main`) performs all blocking SetupAPI disable/enable + verify. Receives `Cmd` over `mpsc::Receiver<Cmd>`; posts `WM_WORKER_RESULT` (WM_APP+3) back via `PostMessageW`.

**Stale-result immunity:** every `Cmd` carries an `op_id`; the loop bumps `current_generation` on state-changing events (toggle, resume, quit). Results with `op_id < current_generation` are dropped. **Always increment generation when you mutate `desired_active` or trigger a transition** or you'll race with stale worker replies.

**Fail-open posture:** on any uncertainty (BLE not configured, Nuphy never paired, worker dead, verify mismatch, COM init fails, resolve >1 or 0 matches) the internal keyboard stays *enabled*. Never invent a path that leaves it disabled on uncertainty.

**Cold-start unconditional ENABLE:** every launch begins with a worker ENABLE + verify *before* anything else. `CONFIGFLAG_DISABLED` survives reboot, so a crash mid-disable means the keyboard is disabled at next boot. `--recover` does the same thing inline (no mutex, no worker).

**Elevation:** the embedded manifest is `requireAdministrator`, so every launch triggers UAC. `main.rs` still has a `relaunch_elevated` `ShellExecuteExW("runas")` fallback as a safety net for environments where the manifest is bypassed; it is currently unreachable in normal use.

**Module map:**
- `src/main.rs` — message loop, worker, tray, state, power/session events, elevation, single-instance mutex (`Local\switchboard-singleton-v1`)
- `src/device.rs` — SetupAPI enumeration + the 3-clause target predicate (Service `kbdhid` AND HardwareIds contains `VID_045E&PID_006C` AND Parent path starts with the SAM GUID). Predicate must match exactly one device or DISABLE is refused (fail-closed).
- `src/ble.rs` — BLE `ConnectionStatusChanged` subscription. BD_ADDR is loaded at runtime from `SWITCHBOARD_NUPHY_BD_ADDR` env var or a `.env` file (precedence: env → exe-dir/.env → cwd/.env). Returns `BleError::NotConfigured` if missing — caller logs and continues fail-open.
- `src/theme.rs` — light/dark tray icon swap based on `SystemUsesLightTheme` registry value.
- `src/autostart.rs` — Per-user autostart via Task Scheduler logon task (`switchboard-logon`, `RunLevel=HighestAvailable`, silent elevation at logon).
- `src/boot_task.rs` — Optional SYSTEM-level boot task (`switchboard-boot-recover`) that runs `switchboard.exe --recover` for lockout protection.

## Conventions

- **No SetupAPI on the message-loop thread** outside the three named exceptions above.
- **COM lifetime is paired with `BleHandle`.** `ble::start()` calls `CoInitializeEx` *only after* BD_ADDR is loaded; every error path after init calls `CoUninitialize`; the success path's `CoUninitialize` lives in `BleHandle::Drop`. Do not add an early-return between init and handle construction without a balancing uninit.
- **HWND across threads** — `HWND` is `*mut c_void` and not `Send`. The BLE event handler captures `hwnd as isize` and reconstructs `HWND(raw as *mut _)` inside the closure. Follow this pattern for any new cross-thread WinAPI handle.
- **No `panic!` in normal paths.** Use `shutdown_cleanup()` + `std::process::exit(code)` so the keyboard gets re-enabled before the process dies.
- **Bluetooth MAC is PII** — never hardcode in source, tests, docs, or commit messages. `.gitignore` covers `.env`, `.env.local`, `bd_addr.txt`, `*.bdaddr`. Only `.env.example` (placeholder format) is committed; `dist/.env` is the runtime working copy.
- **`ARCHITECTURE.md` is the source of truth** for behaviors and threading. When changing a behavior, update both the code and ARCHITECTURE.md (and README.md's user-facing equivalents) so they stay in sync.
- **Cargo edits stay in container:** prefer the in-Docker `cargo fmt`; the host may not have a Windows-targeted toolchain.
- **Off-limits paths** (gitignored framework infrastructure — never modify, never commit): `.squad/`, `.copilot/`, `.github/agents/`, `.github/workflows/squad-*.yml`, `.github/workflows/sync-squad-labels.yml`.
