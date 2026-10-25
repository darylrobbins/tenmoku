---
task_uid: "01m5qpht7269smffjw43xqtrtc"
task: "T321"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap tests/outline_keys.rs: in context outline_panel, up, down and enter resolve to outline.select_previous, select_next and open_selected on both platforms, and with vim on k, j, g g, shift-g and o resolve to select_previous, select_next, select_first, select_last and open_selected; with vim off k j o do not resolve; none is bound in an editor context"
    required: true
    manual: false
  - id: "C2"
    text: "outline_panel_keys_tests::focus: outline.show focuses the panel (focus_handle().is_focused) and selects the symbol containing the editor's caret, or the first row with none; a panel with no outline (NoFile, PlainText, Unsupported) ignores the keys without panicking"
    required: true
    manual: false
  - id: "C3"
    text: "outline_panel_keys_tests::move: simulate_keystrokes down, j, k, g g, shift-g move a highlighted selection over rows(), clamp at both ends, scroll it into view, and take counts (3j) in vim mode"
    required: true
    manual: false
  - id: "C4"
    text: "outline_panel_keys_tests::jump: enter and o on a row put the editor's caret on the symbol's name exactly as a click does (same generation guard: a key pressed while the outline is replaced by another editor's does not land) and move focus to the editor; the panel stays shown"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-workbench -p fe-gpui and cargo clippy -p fe-keymap -p fe-workbench -p fe-gpui --all-targets -- -D warnings pass; the only edits to existing tests are adding outline_panel and outline_panel \u0026\u0026 vim to the known-context lists of fe-gpui command_tests.rs, keymap_tests.rs (default_keymap_matches_the_old_table) and vim/tests.rs (vim_layer_of_the_default_keymap), and changing the assertions that outline.show leaves focus in the editor (explorer_views_tests.rs show_project_and_outline_show_show_the_explorer, tests/outline_panel.rs shows_the_active_tabs_outline, tests/activity_rail.rs only_the_shown_panel_is_highlighted, tests/search_panel_followups.rs header_tabs_move_focus_off_the_search_input) to assert the outline has it, as C2 requires; outline.show's focus is set in dock.rs's DockOp::ShowOutline, as DockOp::ShowChanges focuses Changes"
    required: true
    manual: false
---

# Drive the Outline panel from the keyboard: it takes focus from outline.show and moves with up, down, j, k, gg, G, jumps to the symbol with Enter or o, in vim mode as well as out of it (Mac T1129)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The Outline panel takes no keys at all: no selection, no j/k, no Enter to jump. Only a mouse click jumps (outline_panel.rs:276 `jump`).

## Current state

- outline_panel.rs:57-70: `focus` is "never focused: the panel takes no keys"; `render` (:357) sets no key_context and no track_focus; there is no selected row. `items()` (:241) is the pre-order row list; `render_rows` (:290) draws them.
- outline.show (workbench_commands.rs:494, DockOp::ShowOutline) shows the Explorer with the outline expanded but does not focus it.
- Overlap: T1108 (blocked, scope includes outline_panel.rs) adds `OutlinePanel::set_served` and swaps what `items()` returns; this card does not touch that and puts its logic in the new outline_panel_keys.rs, with only a small hunk in outline_panel.rs (a selected-row field, track_focus and key_context in render). No dependency is taken so this does not wait on the blocked LSP chain; if T1108 merges first the rebase is a few lines.

## Change

- New outline_panel_keys.rs (declared from outline_panel.rs): `selected: Option<usize>`, commands outline.select_previous/select_next/select_first/select_last/open_selected as impl methods, a key context `outline_panel` (+ `vim` flag, as problems_panel.rs:452 builds it), selection kept by symbol position across refreshes.
- Selection highlight uses the project tree's focused-row colour (fe_ui::TreeRow already used by render_rows).
- outline.show also focuses the panel (the project tree does the same via dock.show_project).
- fastedit.toml: a block `# The Outline panel's list, when it has focus` after the Problems block: up/down/enter always; with `vim`: k j, g g, shift-g, o (neo-tree/nvim-tree conventions; same keys as the other lists, see the sibling list-key cards).
- command_tests.rs: add `outline_panel` to the known-context list (table_is_the_only_binding_source), a one-line edit.

## Tests

New files only: outline_panel_keys_tests.rs (#[cfg(test)] #[path] from outline_panel.rs) and fe-keymap tests/outline_keys.rs.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- Respecced (rev 2) before the first CI run: `outline.show`'s focus is set in
  `dock.rs` (`DockOp::ShowOutline` calls `OutlinePanel::focus_from_show`, as
  `ShowChanges` focuses Changes), and the two existing assertions that
  `outline.show` leaves focus in the editor (`explorer_views_tests.rs`,
  `tests/outline_panel.rs`) now assert the outline has it. C5 says so.
  Rev 3, after the first fast CI: two more such assertions
  (`tests/activity_rail.rs`, `tests/search_panel_followups.rs`), and
  `outline_panel` in fe-gpui's two other known-context lists
  (`keymap_tests.rs`, `vim/tests.rs`), which check the default keymap
  against fe-gpui's catalog, where the outline's commands aren't.
- The commands are a `#[kiln::command(context = "outline_panel")]` block on
  `OutlinePanel` in `outline_panel_keys.rs`, attached with
  `ViewCommands::attach`; the element then sets its own key context with the
  `vim` flag, as the Problems panel builds it.
- Counts: kiln's interceptor dispatches bound keys and swallows a pending
  `g`; an unbound digit reaches the panel's `on_key_down`, which adds it to
  the count (a leading `0` isn't one). Any other unbound key drops it. With a
  count, `gg` and `G` go to that row, as vim's do to a line.
- The selection is dropped when the panel follows another editor (so a key
  pressed then lands nowhere) and kept on the same symbol (name, kind,
  depth, name start nearest, both outlines' offsets mapped to the current
  text with `BufferSnapshot::map_offset`) when the same editor's outline is
  found again (review F2). `outline.show`'s request for the caret's symbol
  stays pending until an outline of the editor's current text version shows,
  being answered again as each lands, unless a key moves the selection first
  (review F1). A click selects its row too.
