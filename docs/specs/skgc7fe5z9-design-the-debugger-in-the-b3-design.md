---
task_uid: "01m7cafd4dz5ffttskgc7fe5z9"
task: "T642"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/design/design-system.md gains a Debugger section under Panels and screens, and 'Debugger: breakpoints, variables, call stack' leaves Still to design; the header's revision number goes up by one"
    required: true
    manual: false
  - id: "C2"
    text: "The section settles each of: breakpoint glyphs drawn over the 81 px gutter's line-number column (enabled, unverified, disabled, the current-frame arrow, and the later conditional and logpoint glyphs), with sizes in px at 13 px code and what happens to the row's number; the click target; the current-line and caller-frame bands"
    required: true
    manual: false
  - id: "C3"
    text: "It settles the title bar's run widget: a Debug button beside run and stop, launch configurations listed beside tasks in its picker, and where Continue, Pause, the steps, Restart and Stop sit while a session runs with a title bar, and the floating toolbar (§5) where there is none"
    required: true
    manual: false
  - id: "C4"
    text: "It settles the Debug tool window in the bottom panel: its rail icon in the bottom group next to Runs, its views (Variables, Watch, Call Stack, Breakpoints with exception filters), their layout and empty states, and whether the Debug Console is a tab of this tool window or of the terminal panel"
    required: true
    manual: false
  - id: "C5"
    text: "It settles the debugger's other entry points and surfaces: a debug session's row in Runs and its entry in the status bar's operation in progress, the test lens reading `▷ Run test · Debug`, Debug this test in a test failure, and the hover value card"
    required: true
    manual: false
  - id: "C6"
    text: "Workbench anatomy's Title bar, Rail and Bottom panel rows name the debugger's parts, and the Panels and screens table lists each debugger artboard to draw on the canvas (gutter states, run widget with a session, Debug tool window while stopped, console, hover value), in Yohen and Hakuji"
    required: true
    manual: false
  - id: "C7"
    text: "Each new colour role (at least the current-line band and the caller-frame band; the breakpoint states reuse existing roles or get their own) has Yohen and Hakuji values in the doc; the decision file includes the script that checks them and its output: code text on each band keeps the doc's floors (6:1 in Yohen, 4.5:1 in Hakuji, also on the cursor line), every glyph is at least 3:1 on the gutter and on the cursor line, and each role's OKLab distance to its nearest role"
    required: true
    manual: false
  - id: "C8"
    text: "The section names, for every control, its accessible name and the announcement texts (Stopped at line N and the rest), and every size it uses as an existing kiln-ui roles.rs constant or a new role to add (toolbar button, glyph, panel row)"
    required: true
    manual: false
---

# Design the debugger in the B3 design system: breakpoints over the line numbers, the run widget's Debug button, a Debug tool window in the bottom panel, and the debug colour roles (debugger D0) (Mac T1839)

# Design the debugger in the B3 design system: breakpoints over the line numbers, the run widget's Debug button, a Debug tool window in the bottom panel, and the debug colour roles (debugger D0)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

The debugger record (`docs/decisions/r1b9dw3g0g-debugger.md`) was approved with the B3 direction (§13, "B3 design
first"): the UI cards D6, D8, D8b, D9, D10 and D11 must fit `docs/design/design-system.md`,
where the debugger is still listed under "Still to design". §5's layout predates B3:
it puts a breakpoint lane in a gutter that B3 pins at 81 px, and a Run and Debug
container in a right dock B3 doesn't have.

## What to build (docs only)

A Debugger section in `docs/design/design-system.md`, written as the rest of the
file is (rules, sizes, roles, keys), that the UI cards implement. §13's direction:

- **No right dock.** Variables, Watch, Call Stack and Breakpoints become a **Debug
  tool window in the bottom panel**, its icon (Lucide `bug`, already in kiln-ui) in
  the rail's bottom group next to Runs, instead of §5's right-dock container.
- **Breakpoints over the line-number column.** The B3 gutter is a fixed 81 px
  (`crates/fe-gpui/tests/gutter_b3.rs` pins it), so there is no new lane: say how
  each §5 state (D6 rows of the state table, and the D17 rows for later) draws over
  the number, and the current-frame arrow.
- **The title bar's run widget** (T1539, `crates/fe-workbench/src/run_widget.rs`)
  gets a Debug button and lists launch configurations beside tasks. Say where the
  step controls go while a session runs with a title bar. §5's floating toolbar
  stays where there is no title bar (Linux; `crates/fastedit/src/main.rs:758`).
- **Runs and operations.** A debug session shows in the Runs panel (T1570's model,
  T1571's panel) and in the status bar's operation in progress (T1585's model,
  T1586's view); the test lens reads `▷ Run test · Debug` (`test_lens.rs`), and a
  test failure's Debug this test (T1577, T1590) opens D13's debug test.
- **Theme roles.** The new tints (§5's `Tint::DebugCurrent`, `Tint::DebugFrame`)
  get roles with values in Yohen and Hakuji, checked against the doc's contrast
  rules; sizes come from kiln-ui's `roles.rs` or are named as new roles.
- **Accessibility**: accessible names for each control and the announcements.

Read the record's §5 (UI), §6 (commands and keys) and §13. The canvas artboards
(the doc's "Artboard on the canvas" column) are listed for drawing; the canvas
itself is outside the repo.

## Not in this card

Any code, theme file or golden: D6 adds the roles to `crates/fe-config` and every
theme. Inline values (D18) and conditional breakpoints (D17) only need their glyph
rows reserved here.

## Notes

- `docs/design/design-system.md` is also in Mac T1817's scope (lent to argon): its
  edit is a row in the colour table; rebase over it if it lands first.
- Design calls and the contrast script go in your decision file
  `docs/decisions/<lane>-*.md` (exempt from scope).

Platform: **Linux-OK** (docs only; nothing renders, no goldens move).
