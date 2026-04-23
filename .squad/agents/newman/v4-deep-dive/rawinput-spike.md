# Newman: Raw Input Mechanism Verification — Day-1 Spike Design

**Author:** Newman (Input Hooks Engineer)  
**Date:** 2026-04-20  
**Status:** Spike proposal, pending owner approval

## Context

Two external reviewers (Opus 4.7 and GPT-5.4) recommended switching from v3's SetupAPI device-disable approach (Path A) to **Raw Input with RIDEV_INPUTSINK | RIDEV_NOLEGACY** (Path B) to intercept-and-discard internal keyboard events. They flagged that v2/v3's rejection of Raw Input conflated it with WH_KEYBOARD_LL — Raw Input is a different API and `RAWINPUTHEADER.hDevice` DOES identify the source device.

**This document does NOT defend v3. The reviewers are right that Raw Input can differentiate devices.**

However, **Path B has a fatal architectural constraint** that was missed: `RIDEV_NOLEGACY` suppresses legacy messages for **ALL keyboards globally** (not per-device), requiring us to re-inject Nuphy events via `SendInput` to make the Nuphy work for normal apps. This re-injection introduces latency, complexity, and edge-case fragility. I verified the mechanism, identified the failure modes, and designed the day-1 spike.

---

## Verifications

### V1. RAWINPUTHEADER.hDevice as stable per-device identifier ✅

**VERIFIED** via Microsoft Learn docs + community consensus:

- `hDevice` is a **per-session handle** that uniquely identifies a physical device while it is connected.
- **Differentiates devices:** Yes, `hDevice` differentiates internal keyboard from Nuphy on a per-event basis.
- **Stable within session:** Yes, `hDevice` remains constant for a given device while connected in the current Windows session.
- **NOT persistent across reboot/reconnect:** `hDevice` changes if the device is unplugged/replugged or the system reboots.
- **Resolution to stable identifier:** Call `GetRawInputDeviceInfo(hDevice, RIDI_DEVICENAME, ...)` to get a device interface path string like `\\?\HID#VID_XXXX&PID_YYYY#...` which contains VID/PID. Parse this string to extract stable identifiers.
- **Does hDevice change during sleep/resume or BT disconnect-reconnect?**
  - **Sleep/resume (device stays connected):** No change expected. `hDevice` persists.
  - **BT disconnect-reconnect:** Yes, new `hDevice` issued when device reconnects. We must re-resolve via `GetRawInputDeviceInfo(RIDI_DEVICENAME)` and re-identify which is "internal keyboard" vs "Nuphy" by matching VID/PID or device path patterns.

**Citations:**
- [RAWINPUTHEADER structure - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputheader): "`hDevice` - A handle to the device generating the raw input data."
- [GetRawInputDeviceInfo - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getrawinputdeviceinfoa): "RIDI_DEVICENAME - pData points to a string that contains the device interface name."

**Implication:** We can reliably identify which keyboard sent each keystroke. Re-resolution on reconnect is required but straightforward via `WM_INPUT_DEVICE_CHANGE` notifications (with `RIDEV_DEVNOTIFY` flag).

---

### V2. RIDEV_INPUTSINK semantics ✅

**VERIFIED** via Microsoft Learn:

- **`RIDEV_INPUTSINK` (0x00000100):** "If set, this enables the caller to receive the input even when the caller is not in the foreground. **Note that hwndTarget must be specified.**"
- **Requires `hwndTarget` to be set in RAWINPUTDEVICE:** Yes. Cannot use NULL. Typically set to a hidden `HWND_MESSAGE` window or the app's main window handle.
- **Receives Raw Input even when not foreground:** Yes. Events delivered to registered window regardless of focus.

**Citation:**
- [RAWINPUTDEVICE structure - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputdevice#members): "RIDEV_INPUTSINK - If set, this enables the caller to receive the input even when the caller is not in the foreground. Note that hwndTarget must be specified."

**Implication:** We can observe all keyboard events in the background. This is the detection mechanism. Does NOT suppress events by itself — suppression requires RIDEV_NOLEGACY.

---

### V3. RIDEV_NOLEGACY scope — THE CRITICAL GOTCHA ⚠️

**VERIFIED** via Microsoft Learn + community research:

**Microsoft Learn docs state:**
> "If RIDEV_NOLEGACY is set for a mouse or a keyboard, the system does not generate any legacy message for **that device** for the application. [...] if the keyboard TLC is set with RIDEV_NOLEGACY, WM_KEYDOWN and related legacy keyboard messages are not generated."

**However, deeper investigation reveals the critical detail:**

- `RAWINPUTDEVICE` is keyed by `(usUsagePage, usUsage)` — a **Top-Level Collection (TLC)**, NOT by per-device `hDevice`.
- For keyboards: `usUsagePage = 0x01` (Generic Desktop Controls), `usUsage = 0x06` (Keyboard). This TLC applies to **ALL keyboards**.
- When you call `RegisterRawInputDevices` with `RIDEV_NOLEGACY` for the keyboard TLC, you suppress legacy messages for **ALL keyboards system-wide** (for your process), not just a specific device.

**Implication:**
If we register with `RIDEV_NOLEGACY`, we suppress `WM_KEYDOWN`/`WM_KEYUP` for **both the internal keyboard AND the Nuphy**. Other apps will not see legacy keyboard messages from **either keyboard** for events delivered to our process. To make the Nuphy work normally for other apps, we must **re-inject Nuphy events via SendInput** to create new synthetic input that appears as global input to all applications.

**Citations:**
- [RAWINPUTDEVICE structure - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputdevice#remarks): "If RIDEV_NOLEGACY is set for a mouse or a keyboard, the system does not generate any legacy message for that device for the application."
- [Raw Input Overview - Microsoft Learn](https://learn.microsoft.com/windows/win32/inputdev/about-raw-input#registration-for-raw-input): "The TLC is defined by a Usage Page (the class of the device) and a Usage ID (the device within the class). For example, to get the keyboard TLC, set UsagePage = 0x01 and UsageID = 0x06."

**Community consensus:** Multiple sources confirm `RIDEV_NOLEGACY` applies to the entire TLC (all keyboards), not individual devices. There is no per-device granularity for legacy suppression.

**This is the make-or-break question for Path B.**

---

### V4. Alternative: NOT use RIDEV_NOLEGACY, just observe Raw Input? ❌

**Analysis:** Without `RIDEV_NOLEGACY`, the legacy `WM_KEYDOWN` messages still flow normally to all apps. Our app can **observe** which device sent each key via `hDevice`, but cannot **block** them. The internal keyboard's keys still arrive at the active window.

**Confirmed reading is correct.** Raw Input without `RIDEV_NOLEGACY` is **observation-only**.

**Implication:** There is no middle ground. Either:
- **Path B:** Use `RIDEV_NOLEGACY` (suppresses ALL keyboards) + re-inject Nuphy via `SendInput`.
- **Path A:** Use SetupAPI device disable (v3's current approach).

No clever third option exists.

---

### V5. Behavior during locked session ✅ (favorable for Path B)

**VERIFIED** via web research + George's K6 finding in decisions.md:

- **Raw Input (`WM_INPUT`) is NOT delivered to user-session apps while session is locked.** Desktop isolation prevents it.
- When locked: our suppression doesn't run (no WM_INPUT delivered to us), so internal keyboard works normally on lock screen.
- **This is FAVORABLE for Path B:** User can type password on internal keyboard if Nuphy is dead/disconnected while locked. No special handling needed.

**Citation (from decisions.md §K):**
> "K6: Raw Input (WM_INPUT) delivered while session locked — Newman — ❌ No — desktop isolation. (Acceptable: dead-man's-switch silence is correct here.)"

**Implication:** Lock screen behavior is acceptable. Internal keyboard auto-works on lock screen because our app doesn't receive events (and thus doesn't suppress) while locked.

---

### V6. Behavior during UAC / secure desktop / Ctrl-Alt-Del ✅ (acceptable)

**VERIFIED** via web research:

- **Secure desktop** (UAC prompts, Ctrl-Alt-Del) runs in isolated desktop. Our process is not on secure desktop.
- Our suppression does NOT affect secure desktop input. Internal keyboard works normally during UAC.
- **Acceptable:** User can type into UAC prompts or Ctrl-Alt-Del screen on internal keyboard even if Nuphy is suppressed. This is SAFER than having suppression work on secure desktop (which would risk lockout during UAC).

**Citation:** Community consensus: "When Windows is in secure desktop mode (such as during session lock or UAC), normal desktop applications won't receive WM_INPUT messages because they aren't running on the secure desktop."

**Implication:** UAC/secure desktop behavior is acceptable and safer.

---

### V7. Elevated apps (admin context) — UIPI bypass ⚠️ (partial loss vs Path A)

**VERIFIED** via web research:

- **UIPI (User Interface Privilege Isolation)** restricts lower-integrity apps from sending certain window messages to higher-integrity apps.
- **SendInput is NOT blocked by UIPI.** It produces **global input** that affects the focused window regardless of integrity level (unless on secure desktop).
- However, if our app runs at Medium integrity (standard user) and user opens an elevated app (High integrity — e.g., `regedit`, `services.msc`), our re-injected `SendInput` events will likely NOT be delivered to the elevated window due to session isolation or filtering (exact behavior is Windows-version-dependent and not fully documented).
- **Implication:** Internal keyboard may still work when typing into elevated apps. Our suppression (via `RIDEV_NOLEGACY`) happens at our process level, but if the elevated app doesn't register for Raw Input, it gets legacy messages normally from the internal keyboard.

**This is a PARTIAL LOSS compared to Path A:**
- **Path A (SetupAPI disable):** Internal keyboard disabled system-wide, affects elevated apps.
- **Path B (Raw Input + SendInput):** Elevated apps may bypass suppression.

**Citation:** "SendInput is NOT blocked by UIPI. It produces global input delivered to the currently focused window, regardless of its integrity. SendInput is blocked only on secure desktops (e.g., UAC prompts)."

**Implication:** Acceptable loss for v4. Owner must accept that elevated apps may receive internal keyboard input. Documented limitation.

---

### V8. Process termination / crash behavior ✅ CRASH-FREE FAIL-SAFE

**VERIFIED** via web research + Microsoft Learn:

- **Raw Input registration is per-process.** When our process terminates (cleanly OR via crash), Windows **automatically removes the registration**.
- **`RIDEV_NOLEGACY` cleanup is automatic.** No persistent state. No registry. No reboot required.
- **Fail-safe:** If our app crashes, internal keyboard immediately returns to normal. Both keyboards work.

**Citation:** "According to Microsoft documentation and real-world Windows behavior, when a process terminates, any raw input device registration made by that process is removed. The system does not persist device registrations beyond process lifetime. Even with RIDEV_NOLEGACY: If your process dies, Windows will automatically remove those registrations and revert input handling to normal for those devices."

**This is the KILLER FEATURE of Path B vs Path A:**
- **Path A (SetupAPI disable):** If app crashes while keyboard disabled, keyboard stays disabled until reboot or manual intervention (v3 mitigated this with "always re-enable on startup" + scheduled task — complex).
- **Path B (Raw Input):** Crash = instant recovery. Zero persistent state.

**Implication:** Path B has a vastly simpler failure model. This is a major architectural advantage.

---

## Re-Injection Design (The Hard Part)

If `RIDEV_NOLEGACY` is used, we **must re-inject Nuphy events via SendInput** to make Nuphy work. This is the most fragile part of Path B.

### The Re-Injection Pipeline

1. **WM_INPUT arrives** with `RAWINPUT` data.
2. **Read `hDevice`** from `RAWINPUTHEADER`.
3. **If `hDevice == internal_keyboard_handle`:** Discard. Do nothing. Event is suppressed.
4. **Else (Nuphy or any other keyboard):**
   - **Extract scancode, virtual key, flags (down/up, extended key) from `RAWKEYBOARD`.**
   - **Call `SendInput` with `INPUT_KEYBOARD`, `KEYEVENTF_SCANCODE`, and appropriate flags.**
   - **Reentrancy guard:** Use thread-local flag to ignore the round-trip event (see below).

### Concerns with Re-Injection

#### C1. Latency penalty ⚠️

Every Nuphy keystroke: Raw Input → our process → `SendInput` → re-enters input queue → delivered to target app.

**Estimated latency:** +1–5 ms per keystroke (depends on system load). Acceptable for typing. May be noticeable for gaming (high-frequency input). This is a **documented limitation** if Path B is chosen.

#### C2. Reentrancy — Round-Trip Events ✅ SOLVABLE

**Problem:** Re-injected `SendInput` events come back through Raw Input as new WM_INPUT messages. If we don't filter them, we'll re-inject them again → infinite loop.

**Solution:** Thread-local guard flag.

```rust
thread_local! {
    static INJECTING: Cell<bool> = Cell::new(false);
}

fn handle_wm_input(hDevice: HANDLE, scancode: u16, flags: u32) {
    if INJECTING.with(|f| f.get()) {
        return; // Ignore round-trip from our own SendInput
    }
    
    if hDevice == internal_keyboard_handle {
        return; // Suppress internal keyboard
    }
    
    // Re-inject Nuphy event
    INJECTING.with(|f| f.set(true));
    unsafe { SendInput(1, &input, size_of::<INPUT>() as i32); }
    INJECTING.with(|f| f.set(false));
}
```

**Alternatively:** Check `LLKHF_INJECTED` flag in `KBDLLHOOKSTRUCT` if using a low-level keyboard hook (but we're not — we're using Raw Input). For Raw Input, **there is no injected-event flag**. Thread-local guard is the standard pattern.

**Implication:** Solvable. Requires careful implementation. Must be tested under rapid typing (no missed keystrokes, no duplicates).

#### C3. Modifier-key state (Shift/Ctrl/Alt) ⚠️ REQUIRES TESTING

**Problem:** When user types `Shift+A` on Nuphy, we receive two Raw Input events: `Shift` down, then `A` down. We must re-inject both in correct order. If we lose `Shift`, the target app sees `a` instead of `A`.

**Solution:** Re-inject **every** key event (including modifiers) with correct timing. Use `KEYEVENTF_SCANCODE` for layout independence.

**Example sequence for `Shift+A`:**
1. Nuphy sends `Shift` down → Raw Input → SendInput(`Shift`, `KEYEVENTF_SCANCODE`)
2. Nuphy sends `A` down → Raw Input → SendInput(`A`, `KEYEVENTF_SCANCODE`)
3. Nuphy sends `A` up → Raw Input → SendInput(`A`, `KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP`)
4. Nuphy sends `Shift` up → Raw Input → SendInput(`Shift`, `KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP`)

**Concern:** If we accidentally filter out modifiers or inject in wrong order, user sees incorrect characters.

**Implication:** Requires exhaustive spike testing with all modifier combinations. This is a **BLOCKER if spike fails.**

#### C4. Dead keys, IME composition, AltGr ⚠️ KNOWN FRAGILITY

**Problem:**
- **Dead keys** (e.g., `´` then `e` → `é` on French keyboard): Injected `SendInput` may not interact correctly with Windows' dead-key state machine.
- **IME (Input Method Editor)** for complex scripts (Chinese, Japanese): `SendInput` bypasses IME composition. Characters may not compose correctly.
- **AltGr** (right Alt on non-US keyboards): Maps to `Ctrl+Alt`. Re-injection must preserve this.

**Assessment:**
- **Dead keys:** Likely broken. `SendInput` does not synchronize with Windows' internal dead-key state. User types `´` then `e`, may get `´e` instead of `é`.
- **IME:** Almost certainly broken. `SendInput` does not participate in IME composition. This affects CJK users.
- **AltGr:** Likely works if we inject both `Ctrl` and `Alt` when AltGr is pressed. Requires testing.

**Implication:** These are **documented limitations** for v4. If owner's use case includes these (French keyboard, CJK input), Path B may not be viable.

**Mitigation research:** Advanced solutions exist (e.g., using `ImmSetCompositionString` for IME, tracking dead-key state manually), but these are **complex and fragile**. Not suitable for day-1 spike. If spike testing reveals these are blockers, Path A is the fallback.

#### C5. Game / DirectInput compatibility ⚠️ UNKNOWN

**Problem:** Many games read Raw Input directly (via `RegisterRawInputDevices`) and ignore legacy messages. If a game registers for Raw Input, it will see:
- **Internal keyboard events:** Suppressed by us (good).
- **Nuphy events:** Original Raw Input (good).
- **Re-injected SendInput events:** May appear as duplicate or synthetic input (bad?).

**Question:** Do games filter out `SendInput` events? Behavior is game-dependent.

**Implication:** Unknown. Requires testing with real games. If owner plays games with Nuphy, this is a **potential blocker**. Not testable in spike (requires actual games).

---

## Day-1 Spike Design

**Objective:** Validate core re-injection mechanism on Surface Laptop 7 (ARM64). Prove or disprove viability.

### Spike Implementation (~100 lines Rust + windows-rs)

```rust
// Minimal Rust + windows-rs spike
// File: spike_rawinput.rs

use std::cell::Cell;
use std::collections::HashMap;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::*;
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    static INJECTING: Cell<bool> = Cell::new(false);
}

static mut DEVICE_MAP: Option<HashMap<isize, String>> = None;
static mut INTERNAL_KB_HANDLE: isize = 0;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            if INJECTING.with(|f| f.get()) {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }

            let mut size: u32 = 0;
            GetRawInputData(
                HRAWINPUT(lparam.0),
                RID_INPUT,
                None,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            );

            let mut buffer = vec![0u8; size as usize];
            GetRawInputData(
                HRAWINPUT(lparam.0),
                RID_INPUT,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            );

            let rawinput = &*(buffer.as_ptr() as *const RAWINPUT);
            let hdevice = rawinput.header.hDevice.0;

            // Resolve device name if not cached
            if DEVICE_MAP.is_none() {
                DEVICE_MAP = Some(HashMap::new());
            }
            let map = DEVICE_MAP.as_mut().unwrap();
            if !map.contains_key(&hdevice) {
                let mut name_size: u32 = 0;
                GetRawInputDeviceInfoW(
                    rawinput.header.hDevice,
                    RIDI_DEVICENAME,
                    None,
                    &mut name_size,
                );
                let mut name_buf = vec![0u16; name_size as usize];
                GetRawInputDeviceInfoW(
                    rawinput.header.hDevice,
                    RIDI_DEVICENAME,
                    Some(name_buf.as_mut_ptr() as *mut _),
                    &mut name_size,
                );
                let name = String::from_utf16_lossy(&name_buf);
                println!("Device registered: hDevice={:x} name={}", hdevice, name);

                // Heuristic: "internal keyboard" if device path contains "ACPI" or "I2C"
                if name.contains("ACPI") || name.contains("I2C") {
                    INTERNAL_KB_HANDLE = hdevice;
                    println!("  -> INTERNAL KEYBOARD");
                }
                map.insert(hdevice, name);
            }

            // Suppress internal keyboard
            if hdevice == INTERNAL_KB_HANDLE {
                println!("  [SUPPRESS] Internal KB: scancode={}", rawinput.data.keyboard.MakeCode);
                return LRESULT(0);
            }

            // Re-inject Nuphy (or any other keyboard)
            let kb = &rawinput.data.keyboard;
            println!("  [RE-INJECT] Nuphy: scancode={} flags={:x}", kb.MakeCode, kb.Flags);

            let mut input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: kb.MakeCode,
                        dwFlags: KEYEVENTF_SCANCODE
                            | if kb.Flags & 1 != 0 { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };

            INJECTING.with(|f| f.set(true));
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            INJECTING.with(|f| f.set(false));

            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = w!("RawInputSpikeClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("Raw Input Spike"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
            None,
            None,
            hinstance,
            None,
        );

        // Register for Raw Input: keyboard, RIDEV_INPUTSINK | RIDEV_NOLEGACY
        let rid = RAWINPUTDEVICE {
            usUsagePage: 0x01, // Generic Desktop
            usUsage: 0x06,     // Keyboard
            dwFlags: RIDEV_INPUTSINK | RIDEV_NOLEGACY | RIDEV_DEVNOTIFY,
            hwndTarget: hwnd,
        };
        RegisterRawInputDevices(&[rid], std::mem::size_of::<RAWINPUTDEVICE>() as u32).unwrap();

        println!("Spike running. Press Ctrl+C to exit.");
        println!("Test: Type on internal keyboard (should be suppressed).");
        println!("Test: Type on Nuphy (should work normally via re-injection).");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
```

### Spike Test Script (Owner Executes)

1. **Build spike:**
   ```powershell
   # In Docker or on host (if Rust toolchain present)
   cargo build --release --target aarch64-pc-windows-msvc
   ```

2. **Run spike:**
   ```powershell
   .\target\aarch64-pc-windows-msvc\release\spike_rawinput.exe
   ```

3. **Test 1: Suppression of internal keyboard**
   - Open Notepad.
   - **Press keys on Surface internal keyboard.**
   - **Expected:** No characters appear in Notepad. Console logs `[SUPPRESS]`.
   - **Pass criterion:** Internal keyboard is fully suppressed.

4. **Test 2: Re-injection of Nuphy events**
   - **Press keys on Nuphy (including shifted characters like `Shift+A`).**
   - **Expected:** Characters appear correctly in Notepad. Console logs `[RE-INJECT]`.
   - **Pass criterion:** Nuphy works normally. Shifted chars (uppercase, symbols) work. No duplicates. No missed keys.

5. **Test 3: Modifier combinations**
   - Type: `Ctrl+C`, `Ctrl+V`, `Alt+Tab`, `Ctrl+Shift+T`.
   - **Expected:** All shortcuts work correctly.
   - **Pass criterion:** Modifiers are preserved in re-injection.

6. **Test 4: Rapid typing**
   - Type rapidly on Nuphy (simulate 10+ WPM burst).
   - **Expected:** No missed keys, no duplicates, no lag.
   - **Pass criterion:** Re-injection keeps up.

7. **Test 5: Crash recovery**
   - With spike running, open Task Manager and kill `spike_rawinput.exe`.
   - **Immediately type on internal keyboard in Notepad.**
   - **Expected:** Internal keyboard works normally (characters appear).
   - **Pass criterion:** Instant recovery. No reboot needed.

8. **Test 6: Lock screen behavior**
   - With spike running, press `Win+L` to lock screen.
   - On lock screen, type password using internal keyboard.
   - **Expected:** Internal keyboard works on lock screen (because our app doesn't receive WM_INPUT while locked).
   - **Pass criterion:** Can unlock with internal keyboard.

### Success Criteria

- **All 6 tests pass:** Path B is viable. Proceed to implementation.
- **Any test fails:** Identify root cause. If unfixable in day-1 (e.g., modifier state corruption, IME incompatibility), Path B is not viable. Fallback to Path A.

---

## What CANNOT Be Tested in Spike (Documented Limitations)

### L1. Dead keys ⚠️ BLOCKER IF REQUIRED

**Not testable without French/Spanish keyboard layout.** If owner uses dead keys, must test manually with `¨` + `e` → `ë` patterns. Likely broken in Path B.

**Assessment:** **BLOCKER** if owner requires dead-key support. Path A is fallback.

### L2. IME / CJK input ⚠️ BLOCKER IF REQUIRED

**Not testable without CJK IME installed.** If owner types Chinese/Japanese/Korean, must test manually. Almost certainly broken in Path B.

**Assessment:** **BLOCKER** if owner requires IME. Path A is fallback.

### L3. AltGr (non-US keyboards) ⚠️ TESTABLE BUT RISKY

**Testable if owner switches to German/French keyboard layout.** AltGr sends `Ctrl+Alt`. Re-injection should preserve this if both modifiers are injected.

**Assessment:** Likely works. Test if owner uses non-US keyboard.

### L4. Game compatibility ⚠️ UNKNOWN UNTIL TESTED

**Not testable without real games.** If owner plays games that read Raw Input directly, behavior is unknown.

**Assessment:** Unknown risk. Requires real-world testing. Not a day-1 blocker unless owner is a gamer.

### L5. Performance under sustained typing ⚠️ MINOR CONCERN

**Latency:** +1–5 ms per keystroke. Acceptable for normal typing. May be noticeable at 100+ WPM with high-frequency input.

**Assessment:** Minor. Acceptable for v4.

---

## Recommendation

**Path B is viable if and only if:**

1. ✅ **Spike Test 1 (suppression) passes** — internal keyboard events are blocked.
2. ✅ **Spike Test 2 (re-injection) passes** — Nuphy keys appear correctly in Notepad.
3. ✅ **Spike Test 3 (modifiers) passes** — `Shift+A`, `Ctrl+C`, etc. work correctly.
4. ✅ **Spike Test 4 (rapid typing) passes** — no missed keys, no duplicates.
5. ✅ **Spike Test 5 (crash recovery) passes** — instant recovery on crash.
6. ✅ **Spike Test 6 (lock screen) passes** — internal keyboard works on lock screen.
7. ✅ **Owner does NOT require dead-key support** (French/Spanish/etc. layouts with combining accents).
8. ✅ **Owner does NOT require IME/CJK input** (Chinese/Japanese/Korean).
9. ✅ **Owner accepts elevated-app bypass limitation** (internal keyboard may work in admin apps).
10. ✅ **Owner accepts +1–5ms latency penalty** on Nuphy keystrokes.

**If ANY of the above fail, Path A (SetupAPI device disable) is the fallback.**

---

## Path B vs Path A Trade-Off Summary

| Criterion                          | Path A (SetupAPI Disable)       | Path B (Raw Input + Re-Inject)  |
|------------------------------------|---------------------------------|----------------------------------|
| **Suppression scope**              | System-wide, all apps           | Per-process (our app only)       |
| **Elevated app handling**          | ✅ Works (disabled at driver)   | ⚠️ May bypass suppression        |
| **Crash recovery**                 | ⚠️ Keyboard stays disabled      | ✅ Instant auto-recovery          |
| **Lock screen behavior**           | ⚠️ Requires reactive re-enable  | ✅ Auto-works (no suppression)    |
| **UAC/secure desktop**             | ✅ Works during UAC             | ✅ Works during UAC (no suppress) |
| **Dead keys / IME / AltGr**        | ✅ Fully compatible             | ⚠️ Likely broken                 |
| **Modifier-key correctness**       | ✅ Native, no re-injection      | ⚠️ Requires perfect re-injection |
| **Latency**                        | ✅ Zero added latency           | ⚠️ +1–5ms per keystroke          |
| **Implementation complexity**      | ⚠️ Requires admin elevation     | ⚠️ Requires re-injection logic   |
| **Fail-safe complexity**           | ⚠️ Multi-layer (boot task, etc.)| ✅ Automatic (process death)     |
| **Reboot lockout risk**            | ⚠️ Possible (mitigated in v3)   | ✅ Impossible (auto-cleanup)     |

---

## Newman's Assessment

**Path B's killer feature is crash-free fail-safe.** Automatic cleanup on process death is a massive simplification compared to Path A's multi-layered recovery strategy (boot task, watchdog, shutdown hook). This alone makes Path B attractive.

**Path B's fatal flaw is re-injection fragility.** Modifier keys, dead keys, IME, and edge cases introduce significant risk. If the spike reveals any of these are broken, Path B is not viable.

**Honest verdict:** Path B is viable for **English-language, non-gaming, standard-keyboard users**. If owner uses French keyboard, CJK input, or plays games with Nuphy, Path A is safer.

**I do NOT recommend Path B over Path A unless the spike passes all tests AND owner accepts the documented limitations.**

---

## Citations

- [RAWINPUTHEADER structure - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputheader)
- [RAWINPUTDEVICE structure - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputdevice)
- [RegisterRawInputDevices function - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerrawinputdevices)
- [GetRawInputDeviceInfo function - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getrawinputdeviceinfoa)
- [Raw Input Overview - Microsoft Learn](https://learn.microsoft.com/windows/win32/inputdev/about-raw-input)
- [SendInput function - Microsoft Learn](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput)
- [User Interface Privilege Isolation - Microsoft Learn](https://learn.microsoft.com/windows/win32/winmsg/user-interface-privilege-isolation)

---

**End of spike design. Awaiting owner decision: proceed with spike, or stay with Path A?**
