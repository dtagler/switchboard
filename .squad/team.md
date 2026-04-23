# Squad Team

> bluetooth-keyboard-app — Windows tray app that disables the Surface Laptop 7's built-in keyboard when the Nuphy Air75 (Bluetooth) connects. Cast: Seinfeld.

<!-- copilot-auto-assign: false -->

## Coordinator

| Name | Role | Notes |
|------|------|-------|
| Squad | Coordinator | Routes work, enforces handoffs and reviewer gates. |

## Members

| Name | Role | Charter | Status |
|------|------|---------|--------|
| Jerry | 🏗️ Lead / Windows Architect | `.squad/agents/jerry/charter.md` | Active |
| Newman | 🎮 Input Hooks Engineer | `.squad/agents/newman/charter.md` | Active |
| Kramer | 📡 Bluetooth / HID Engineer | `.squad/agents/kramer/charter.md` | Active |
| Elaine | 🎨 Tray UI & Packaging | `.squad/agents/elaine/charter.md` | Active |
| George | 🧪 QA / Safety Tester | `.squad/agents/george/charter.md` | Active |
| Peterman | 📝 Docs Writer | `.squad/agents/peterman/charter.md` | Active |
| Scribe | 📋 Memory & Logs (silent) | `.squad/agents/scribe/charter.md` | Active |
| Ralph | 🔄 Work Monitor | `.squad/agents/ralph/charter.md` | Active |

## Project Context

- **Owner:** owner
- **Project:** bluetooth-keyboard-app
- **What it does:** Windows system tray app that automatically disables the Surface Laptop 7's built-in physical keyboard when the Nuphy Air75 (Bluetooth) is connected. Right-click tray menu to enable/disable. Should start with Windows.
- **Hardware:** Surface Laptop 7 15", Snapdragon X Elite, 32GB / 1TB (ARM64 Windows)
- **Target external keyboard:** Nuphy Air75 (Bluetooth)
- **Fail-safe (NON-NEGOTIABLE):** Holding `Escape` for 10 seconds disables the app and restores the built-in keyboard.
- **Stack (proposed, pending Jerry's confirmation):** .NET 8 + WinUI 3 (or WPF) + Win32 P/Invoke (`SetWindowsHookEx WH_KEYBOARD_LL`) + WinRT `Windows.Devices.Bluetooth` + MSIX packaging.
- **Created:** 2026-04-20
- **Cast universe:** Seinfeld
