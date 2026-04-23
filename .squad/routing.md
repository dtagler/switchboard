# Work Routing

How to decide who handles what.

## Routing Table

| Work Type | Route To | Examples |
|-----------|----------|----------|
| Architecture, stack choice, scope, code review | Jerry | "Should we use WinUI 3 or WPF?", "Review this PR", "Is this safe?" |
| Low-level keyboard hook, P/Invoke, fail-safe | Newman | "Implement the WH_KEYBOARD_LL hook", "Add the Escape-10s detector", "Why is the hook leaking?" |
| Bluetooth detection, HID watcher, device identity | Kramer | "Detect when the Nuphy connects", "Add device allowlist", "Debounce flapping connections" |
| System tray, context menu, autostart, packaging | Elaine | "Build the tray icon", "Add MSIX packaging", "Register for autostart" |
| Tests, edge cases, lockout-prevention, safety review | George | "Test fail-safe", "What if the app crashes?", "Verify uninstall is clean" |
| README, user guide, install/uninstall docs, fail-safe docs | Peterman | "Write the README", "Document the troubleshooting steps" |
| Code review (general) | Jerry | Default reviewer for non-trivial PRs |
| Safety review (anything touching the hook or BT signal) | George + Jerry | Both must approve safety-critical changes |
| Session logging, decisions merge | Scribe | Automatic — never needs routing |
| Backlog watch, issue triage cycle | Ralph | Activated by user ("Ralph, go") |

## Issue Routing

| Label | Action | Who |
|-------|--------|-----|
| `squad` | Triage: analyze issue, assign `squad:{member}` label | Jerry |
| `squad:jerry` | Architecture, scope, review | Jerry |
| `squad:newman` | Hook / fail-safe work | Newman |
| `squad:kramer` | Bluetooth / HID work | Kramer |
| `squad:elaine` | Tray, packaging, autostart | Elaine |
| `squad:george` | Test design, safety review | George |
| `squad:peterman` | Docs work | Peterman |

### How Issue Assignment Works

1. When a GitHub issue gets the `squad` label, **Jerry** triages it — analyzing content, assigning the right `squad:{member}` label, and commenting with triage notes.
2. When a `squad:{member}` label is applied, that member picks up the issue in their next session.
3. Members can reassign by removing their label and adding another member's label.
4. The `squad` label is the "inbox" — untriaged issues waiting for Jerry's review.

## Project-Specific Rules

1. **Safety overrides everything.** Anything that touches the keyboard hook lifecycle, the fail-safe, or the BT signal-to-hook bridge requires George's review in addition to Jerry's. Reviewer-rejection lockout applies — original author cannot revise rejected safety code.
2. **The hook never trusts the BT signal alone.** Newman owns the safety contract; Kramer's signal is one input among several.
3. **Fail-safe is a separate subsystem.** It must be testable in isolation (George owns the test).
4. **Docs follow code in the same change set.** When observable behavior changes, Peterman updates docs as part of the same PR.

## General Rules

1. **Eager by default** — spawn all agents who could usefully start work, including anticipatory downstream work (e.g., George writing test cases while Newman builds the hook).
2. **Scribe always runs** after substantial work, always as `mode: "background"`. Never blocks.
3. **Quick facts → coordinator answers directly.** Don't spawn an agent for "what port does the server run on?"
4. **When two agents could handle it**, pick the one whose domain is the primary concern.
5. **"Team, ..." → fan-out.** Spawn all relevant agents in parallel as `mode: "background"`.
6. **Anticipate downstream work.** If Newman is building the hook, spawn George to write fail-safe test scenarios in parallel.
7. **Issue-labeled work** — when `squad:{member}` is applied, route to that member. Jerry handles all `squad` (base label) triage.
