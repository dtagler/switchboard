# SwitchBoard Icon Concepts

Four candidate tray icons for Brady to choose from. All designed for:
- 16x16 legibility (Windows tray minimum)
- Monochrome (uses `currentColor` — works on light and dark tray backgrounds)
- Single-state (no variants per Brady's simplification)

**Preview:** Open any `.svg` in a browser (no PNG converter available on host).

---

## 1. keyboard-toggle.svg
**What it looks like:** Small keyboard base (3 key rows) with a toggle switch slider above it.

**Why it works:** Directly illustrates "SwitchBoard" — a keyboard you switch on/off. Toggle slider is a universal "control" symbol. The two elements (keyboard + switch) stack vertically, each readable at 16px.

**Tradeoffs:** Two distinct visual elements may compete at smallest sizes. The toggle switch reads as "settings" more than "patching" (the old-school switchboard sense).

**Squad recommendation:** This was the team's #1 pick from the concept list.

---

## 2. spacebar-slice.svg
**What it looks like:** A single wide rounded rectangle — the spacebar.

**Why it works:** Ultra-minimal. Instantly recognizable as "keyboard" to anyone who's typed. No fine detail to lose at 16px. Bold, distinctive shape.

**Tradeoffs:** Doesn't reference "switch" or "board" at all. Could be mistaken for a generic "minus" or "loading bar" without context. Very abstract.

---

## 3. three-rows-dots.svg
**What it looks like:** 4 dots (top row), 4 dots offset (middle row), wide pill (spacebar bottom).

**Why it works:** Captures keyboard essence with maximum simplicity. The offset rows hint at QWERTY stagger. Spacebar anchors the composition. Reads clearly even at 16px.

**Tradeoffs:** Abstract — requires viewer to mentally map "dots = keys." No SwitchBoard reference. Could be confused with a loading indicator or signal bars.

---

## 4. switchboard-panel.svg
**What it looks like:** Square panel with a 3×3 grid of jack holes, one filled (plugged).

**Why it works:** Literal reference to old telephone switchboard panels — the "SwitchBoard" name origin. The filled jack shows "connection" state. Distinctive visual that's unlike any other tray icon.

**Tradeoffs:** Loses the keyboard reference entirely. Users under 40 may not recognize the switchboard metaphor. More complex than the others (9 circles + panel border).

---

## My Recommendation

**Keyboard-toggle** (`keyboard-toggle.svg`) balances both references: keyboard is visible, toggle switch conveys "switch." It's the safest choice for an app named SwitchBoard.

**If Brady wants bolder:** `spacebar-slice.svg` is the most distinctive and will stand out in the tray, but relies on tooltip/name to convey purpose.

**If Brady wants literal SwitchBoard:** `switchboard-panel.svg` is a conversation starter but loses keyboard context.

---

## Next Steps

After Brady picks a direction:
1. Refine the chosen concept (adjust proportions, stroke weights)
2. Generate multi-resolution ICO (16, 20, 24, 32, 256px)
3. Test on actual Windows 11 light + dark tray backgrounds
