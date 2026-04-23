# Squad Learning History

## 2025-01-21 — Tray Icon Strategy Locked

**Decision:** Brady selected Strategy #4 Soft Accent for the SwitchBoard tray icon.

**Details:**
- Keycap stroke color: #0078D4 (Microsoft/Surface blue)
- Ship 2 ICOs: `keycap-s-dark.svg` (white glyph) and `keycap-s-light.svg` (dark glyph)
- Theme detection: Read `SystemUsesLightTheme` from `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
- Runtime swap: Watch for `WM_SETTINGCHANGE`, swap HICON via `Shell_NotifyIcon` with `NIM_MODIFY`
- Default: Dark taskbar variant (Win11 standard)
