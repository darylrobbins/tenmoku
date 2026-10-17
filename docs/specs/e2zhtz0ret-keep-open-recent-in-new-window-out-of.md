---
task_uid: "01m54v6awehwgrf2e2zhtz0ret"
task: "T134"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "#[command] takes a defaulted palette = false option; a command marked so is registered and dispatches by ID and key but kiln-shell's palette doesn't list it, and every existing command is listed as before (kiln test command_palette_hidden.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "recent.open_in_new_window is marked palette = false; the palette opened over the recent picker or another picker doesn't list Open Recent in New Window, and its picker key still opens the chosen recent folder in a new window"
    required: true
    manual: false
  - id: "C3"
    text: "The regression test tests::palette_recent::the_palette_over_a_picker_leaves_out_open_recent_in_new_window fails before the fix and passes after; no other command ID, title, key or context changes"
    required: true
    manual: false
---

# Keep Open Recent in New Window out of the command palette, through a palette = false option on kiln's #[command] (Mac T698 review F1)

Argon follow-up to T114 (Mac T698) review F1 (medium, a real bug, filed under note d). Inserted into the zo chain after T115 (Mac T699) and before T116 (Mac T701), because T116 moves `toast.dismiss_all`, which the old registry also kept out of the palette. Title the host commit "... (Mac T698 review F1)".

## Problem

T114 made `recent.open_in_new_window` a typed command (`#[command(id = "recent.open_in_new_window", context = "picker")]`,
crates/fe-workbench/src/workbench_commands.rs ~262). kiln-shell's command palette lists every typed command whose context
holds (command_palette.rs, `typed_commands(cx)` ~88), so the palette now lists "Open Recent in New Window" whenever it
opens over any picker. Choosing it does nothing: opening the palette replaces the recent picker, so the command's
Palette::Recent guard fails. The deleted registry kept that command out of the palette on purpose, and kiln's
`#[command]` has no option to do the same.

## Change

- Add a defaulted, additive palette option to kiln's typed commands: `#[command(id = "..", palette = false)]`
  (kiln-macros command.rs, recorded with the command), and make kiln-shell's palette skip a typed command marked so.
  Default `true`; every existing command is unchanged.
- Mark `recent.open_in_new_window` with `palette = false`. Its picker key binding and its run from the recent picker are
  unchanged.

## Tests

- kiln: a new test (kiln/crates/kiln/tests/command_palette_hidden.rs) shows a `palette = false` command is registered,
  dispatches by ID and key, and isn't listed by the palette, while a default command is.
- fe-workbench: a new src/tests/palette_recent.rs, declared with one `mod palette_recent;` line in src/tests/mod.rs. The
  palette opened over the recent picker and over another picker doesn't list "Open Recent in New Window". Regression
  test `tests::palette_recent::the_palette_over_a_picker_leaves_out_open_recent_in_new_window` (full module path).
