---
task_uid: "01m4ry9x6rd4ytz06v5nz34fbh"
task: "T63"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "problems_panel_tests::lists: with the fake server publishing for two open files (one in two splits), the panel shows two file groups with counts and one row per diagnostic, and updates when they clear"
    required: true
    manual: false
  - id: "C2"
    text: "problems_panel_tests::open: problems.open on a row (and a click) activates that file's editor and selects the range; select_next/select_previous move over problem rows"
    required: true
    manual: false
  - id: "C3"
    text: "problems_panel_tests::next_previous: diagnostics.next and diagnostics.previous move the active editor's caret to the next/previous diagnostic, wrapping"
    required: true
    manual: false
  - id: "C4"
    text: "problems_panel_tests::status: the status bar item shows the error and warning counts and runs problems.show when clicked"
    required: true
    manual: false
  - id: "C5"
    text: "problems_panel_tests::vim: with vim on, the panel carries the vim flag, diagnostics.next leaves the editor in normal mode, and problems.open puts focus in the editor in normal mode; cargo test -p fe-workbench and clippy -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Problems panel: the open buffers' diagnostics grouped by file, click to jump, next and previous problem, and error and warning counts in the status bar (Mac T515)

Filed on helium from Mac T515 (~/coordination/helium-cards/T515.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-z.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch Z to fill idle lanes; independent of helium's T47-T55 chain.

## Problem

There's no Problems panel and no next/previous diagnostic (day-one audit 2026-10-12). The previous card adds
`next_diagnostic`, `diagnostic_counts` and `group_problems` in fe-editor.

## Change

- New `crates/fe-workbench/src/problems_panel.rs` (declared from dock.rs): a dock panel (`DockPanel::Problems`,
  `dock_item!` in `crates/fe-workbench/src/dock.rs`) listing the language-server diagnostics of every open buffer
  in the window, once per buffer however many splits show it, grouped by file (file rows with counts, then one row
  per problem: severity icon, message, source, `row:col`). It updates as diagnostics arrive or clear. Known limit,
  in its docs: fe-lsp delivers diagnostics per open document, so files never opened aren't listed.
- Commands: `problems.show` "Show Problems" (opens and focuses it), `problems.select_next`,
  `problems.select_previous`, `problems.open` (click or Enter: activates the editor and selects the range).
- `diagnostics.next` "Go to Next Problem" and `diagnostics.previous` on the active editor (context `workbench`):
  select the diagnostic's range start with `EditorModel::next_diagnostic`, wrapping, and reveal it.
- A status bar item (left, through `StatusBar::set_item`, status_bar.rs untouched) with the window's error and
  warning counts; clicking it runs `problems.show`. Built and kept current in `crates/fe-workbench/src/
  workbench.rs`.
- Vim: the panel's element adds the `vim` flag to its key context while `fe_gpui::vim_enabled`, for the keys
  card's j/k; `diagnostics.next` from vim normal mode moves the caret and stays in normal mode (in visual mode it
  extends nothing: it's bound only in normal mode); leaving the panel with `problems.open` puts focus in the
  editor in normal mode. Keys (`]d`, `[d`, F8) are the keys card.

## Tests

New file `crates/fe-workbench/src/problems_panel_tests.rs`, declared from problems_panel.rs, with the fake
language server of `tests/lsp_diagnostics.rs`.

## Implementation notes

- The commands live in the dock's registry as `DockOp::Problems(ProblemsOp)` (`lib.rs` is outside the
  scope, and `dock::init` and `dock::registry` already feed the registry, catalog and palette).
  `problems.show` is the dock's op; the rest are the workbench's (`Workbench::run_problems_op`).
- The workbench lists the problems again (`Workbench::sync_problems`) on every open editor's
  notification, which covers diagnostics arriving or clearing and edits moving them, and as tabs open
  and close. Files are deduplicated by path before `group_problems`, which would otherwise merge a
  split's copy into its file's group twice.
- The status item shows only while there are errors or warnings, as the config problems item does:
  the existing status bar tests expect an empty left side without diagnostics.
- The Problems rail button joins the rail once the panel has shown, as References does, so the
  rail's existing tests hold. Its icon is `Icon::X`: the bundled set has no warning or error sign.
- Severity marks are coloured letters (`E`, `W`, `I`, `H`) in the editor theme's diagnostic colours,
  as the Changes panel's status letters are; a message shows its first line.
- `problems.open` opens in the active pane if it shows the file, else in the first pane that does,
  activating it. With vim on, the caret goes to the problem's start; selecting leaves a visual mode
  and drops a pending command. An editor left in insert or replace mode is sent vim's
  `vim.normal_mode` command (not an `escape` keystroke, which the user's keymap may rebind) once
  the workbench's next frame has drawn it (`Workbench::land_problem`): a background tab isn't in
  the rendered frame yet to take the command. Its caret is then put back at the problem.
- `diagnostics.next` and `diagnostics.previous` step from the primary selection's head, the caret,
  whichever way the selection runs.
- When the list changes, the selected problem stays selected if it's unchanged; else the nearest
  (by row, then column) with its severity, message and source is.
