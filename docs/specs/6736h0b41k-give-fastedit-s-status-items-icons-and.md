---
task_uid: "01m4sjfhmz1grz486736h0b41k"
task: "T76"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test status_bar_icons::diagnostics_pair: for 2 errors and 1 warning the problems panel's items are 'problems' (text 2, Icon::CircleX, tone Error) and 'problems.warnings' (text 1, Icon::TriangleAlert, tone Warning), both with tooltip '2 errors, 1 warning' and command problems.show; for 0 errors and 1 warning 'problems' reads 0 with tone Default; for none, neither item is on the bar"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test status_bar_icons::lsp_and_config: the lsp item has Icon::Server, tone Error when stopped with an error, Warning while restarting, Default while starting or indexing, tooltip 'Show Language Server Log', text unchanged; the config problems item has Icon::FileCog, tone Warning, tooltip 'Show Config Problems', text unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test status_bar_icons::editor_and_viewer: an editor's cursor item has tooltip 'Go to Line/Column' and its language item 'Change Language Mode'; a file viewer's Read-only item has Icon::Lock"
    required: true
    manual: false
  - id: "C4"
    text: "problems_panel_tests pass with their status text assertions updated to the pair; cargo test -p fe-gpui -p fe-workbench passes; clippy -D warnings clean on both; no screenshot golden changes"
    required: true
    manual: false
---

# Give fastedit's status items icons and tooltips: error and warning counts, config problems, the language server, read-only, and the clickable cursor and language (Mac T599)

Filed on helium from Mac T599 (~/coordination/helium-cards/T599.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zk.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-13): "Maybe some fancier toolbar icons. Let's make it a fancier status bar than just pure text."

fastedit's status items are plain text. With the framework card this depends on, a `kiln_shell::StatusItem` can
carry an icon, a tone (tints the icon from `status.error` / `warning` / `info` / `success`) and a tooltip. The
items that have a natural icon:

- diagnostics, `crates/fe-workbench/src/problems_panel.rs` `status_item` (id `problems`, left, priority 60):
  one item reading `1 error, 1 warning`, hidden when both are 0, click runs `problems.show`;
- config problems, `crates/fe-workbench/src/config_problems.rs` `status_item` (id `config.problems`, left, 50):
  `1 config problem`, click runs `workbench.show_config_problems`;
- the language server, `crates/fe-workbench/src/lsp.rs` (id `lsp`, right, priority 0, around line 1504): `rust-analyzer:
  indexing …`, `…: stopped: …`, click runs `lsp.show_log`;
- the large-file viewer's `Read-only`, `crates/fe-gpui/src/file_viewer/pane_item.rs` `status_items`.

The editor's cursor and language items are clickable (go to line, language picker) but nothing says so.

## Change

Build these items with `kiln_shell::StatusItem::new(…)` and its builders (not fe-workbench's `StatusItem` mirror,
which stays as it is):

- **Diagnostics** become two items, VS Code's compact form, both shown when errors + warnings > 0 and both removed
  at 0 (as the one item is today), both running `problems.show`:
  - `problems` (priority 60): `Icon::CircleX`, text the error count (`2`), tone `Error` when errors > 0 else
    `Default`;
  - `problems.warnings` (priority 61, a new `WARNINGS_ITEM` const): `Icon::TriangleAlert`, text the warning count,
    tone `Warning` when warnings > 0 else `Default`;
  - each with tooltip the old text, `1 error, 1 warning`.
  Update the sync that sets or removes the item (around `problems_panel.rs:534`) for the pair, and the existing
  assertions in `crates/fe-workbench/src/problems_panel_tests.rs` that read `1 error, 1 warning` as the `problems`
  item's text (it is now `1`; the warnings item `1`).
- **Config problems**: `Icon::FileCog`, tone `Warning`, tooltip `Show Config Problems`; text unchanged.
- **Language server**: `Icon::Server`; tone `Error` for `Stopped(Some(_))` and a server message, `Warning` for
  `Restarting`, else `Default`; tooltip `Show Language Server Log`; text unchanged. Move the item's construction
  into a pure function of what it shows, so it can be tested without a server.
- **Read-only** (file viewer): `Icon::Lock`; text unchanged.
- **Editor items** (`crates/fe-gpui/src/pane_item.rs`, `status_items`): tooltip `Go to Line/Column` on the cursor
  item and `Change Language Mode` on the language item (the titles of `go_to_line.toggle` and `language.select`, without their `…`).
  No other editor item changes; no icons on them, so the workbench screenshot golden doesn't change.

## Tests

New file `crates/fe-workbench/src/tests/status_bar_icons.rs`, declared in `crates/fe-workbench/src/tests/mod.rs`.
The file viewer's and editor's items can be checked from it through the active item's `status_items`.
