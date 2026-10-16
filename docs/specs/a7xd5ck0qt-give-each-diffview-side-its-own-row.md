---
task_uid: "01m4xewdd0yz65xna7xd5ck0qt"
task: "T121"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor tests row_changes_supplied_tests::own_state and ::never_diffs: after own_row_changes() on a split of a model with a HEAD base, supply_row_changes on it changes its row_changes() but leaves the original's base and row_changes() as they were, and later splits of it share the supplied ones; in supplied mode an edit leaves row_changes() at the last supplied changes until ones for the new version arrive, with row_changes::diff_count() unchanged; revert_changes reverts to the supplied base; set_diff_base returns to computed mode"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test diff_view_compare_tests::editable_side_compares: a DiffView whose editable right side is a split of an open EditorView's model (that model has a HEAD diff base) gives each side row_changes() against the other side's text, at a small size and at 1.5 MiB once the background diff lands, while the open EditorView's row_changes() stay against HEAD; after typing on the right they update when the next diff lands; with vim on, ]c on the right side goes to the first hunk's row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test diff_view_compare_tests::catalog: commands::command_catalog() accepts diff.next_change, diff.previous_change and diff.toggle_inline; a user keymap binding f7 to diff.next_change in context DiffView loads through KeymapStore::load with that catalog without diagnostics, and f7 in a DiffView moves the focused side's caret to the next hunk; commands::command_list() lists all three with their titles"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test diff_view_compare_tests::shares_both_ways: an EditorView and a DiffView over the same buffer each report shares_document of the other, and a DiffView over another file does not; with the buffer dirty, closing either one through a kiln-shell pane while the other stays shows no prompt, and closing the survivor afterwards prompts once"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor and cargo test -p fe-gpui pass; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Give each DiffView side its own row changes against the other side, register diff.next_change and diff.previous_change in the catalog and palette, and let an editor recognise a DiffView over its buffer (T545 review F1, F2, F3, F7) (Mac T660)

## Problem

T545 (the DiffView, built on neon as neon T109, branch `lane/t2zd8recbg`) left four of codex's review findings
(review 7stkhd on candidate 3drrny, 2026-10-13) to this card, because each needs a file outside T545's scope
(neon lead's rev 2 note):

- **F1 (high), `crates/fe-gpui/src/diff_view.rs:154/301/305`**: "Editable right models still receive no comparison
  base, so their gutters and Vim change navigation are absent or compare against HEAD; large comparisons also clear
  read-only sides' bases." `DiffView::new` sets a diff base only on read-only sides (`sets_base = [true,
  right.read_only]`), because the right side is a `EditorModel::split` of the file's model and splits share one
  `RowChangeState` (`crates/fe-editor/src/lib.rs` `split`, `row_changes: Rc::clone(&self.row_changes)`): setting it
  would replace the file tabs' HEAD base. `DiffView::land` also clears every base when a side is over `ASYNC_BYTES`
  (1 MiB), since `RowChangeState::changes` would run `diff_rows` on the main thread when a frame paints.
- **F3 (medium), `diff_view.rs:304`** (same root): "An editable split retains its HEAD base, so painting its gutter
  after edits still synchronously converts and diffs texts between 1 and 5 MiB despite the background comparison."
- **F2 (high), `diff_view.rs:48/52`**: "Both navigation commands remain absent from the application command catalog
  and palette, so validated user keymaps reject them." `diff.next_change` / `diff.previous_change` are only constants;
  `crates/fe-gpui/src/commands.rs` `command_catalog()` and `command_list()` don't know them, so a keymap binding them
  (T557 binds f7 / shift-f7 in `DiffView`) fails validation, and the palette doesn't list them.
- **F7 (medium), `diff_view.rs:615`**: "Document sharing remains asymmetric: DiffView recognizes EditorView, but
  EditorView does not recognize DiffView, causing unnecessary or duplicate dirty-close prompts." kiln's close
  (`kiln/crates/kiln-shell/src/close.rs` `shown_by`, `ask`) asks each surviving item `shares_document`; an
  `EditorView` surviving a dirty DiffView's close answers false (`crates/fe-gpui/src/pane_item.rs`
  `EditorView::shares_document` only downcasts to `EditorView`), so the DiffView prompts although the editor still
  shows the buffer.

## Change

**fe-editor: a model's own comparison state, fed by its host** (`crates/fe-editor/src/lib.rs`,
`crates/fe-editor/src/row_changes.rs`):

- `EditorModel::own_row_changes(&mut self)`: gives this model a fresh `RowChangeState` (no base), tracked by its
  `caught_up`. Models split from it earlier keep the old state, so the file's tabs keep their HEAD base and change
  bars; models split from it afterwards share the new one. Doc it next to `split`.
- `EditorModel::supply_row_changes(base: Arc<str>, version: Version, changes: Arc<[RowChange]>)`: sets the base (so
  `revert_changes`, which the diff wiring card's `do`/`dp` uses, reverts to it) and puts the state in supplied mode:
  `row_changes()` returns the changes supplied for its snapshot's version, or until those arrive the last ones
  supplied, and never calls `diff_rows`. Rows past the text's end are already clamped by `RowChangeDecorations`.
  `set_diff_base` returns the state to computed mode.

**DiffView** (`crates/fe-gpui/src/diff_view.rs`):

- `DiffView::new` calls `own_row_changes()` on both sides' models before making their views; drop `sets_base`.
- `land` supplies each side its row changes from the view's own `TextDiff` (made off the main thread over
  `ASYNC_BYTES`, as now) for every size: each side's changes against the other side's text as base (a hunk with
  rows only on this side `Added`, on both `Modified`, only on the other `Deleted` at this side's start row), with
  the other side's text as base and the version each text was read at (carry it with the texts in `Diffed`). A
  too-large comparison (`TextDiff::too_large`) supplies no changes. Change bars, `row_changes()` and vim's `]c`/`[c`
  then work on an editable right side, and no paint of either side diffs on the main thread.
- A `diff_view::registry()` (a `CommandTarget` like `crates/fe-gpui/src/view/proposal_commands.rs`) registering
  `diff.next_change` "Go to Next Difference" and `diff.previous_change` "Go to Previous Difference" (and
  `diff.toggle_inline` "Toggle Inline Diff", which T546 adds), each in the `DiffView` context, their handlers doing
  nothing: the view's `RunCommand` handler runs them, as today.
- `pub(crate) fn right_model(&self) -> &EditorModel` (or equivalent) for `pane_item.rs`.

**Catalog and palette** (`crates/fe-gpui/src/commands.rs`): `Catalog` gains the diff registry (checked after the
proposal commands, before the app's) and `command_list()` chains its commands, so `fe_workbench::command_catalog()`
and the palette get them with no fe-workbench change.

**Symmetric sharing** (`crates/fe-gpui/src/pane_item.rs`): `EditorView::shares_document` also answers true for a
`DiffView` whose right side shares its buffer (`shares_buffer_with`).

Not in this card: key bindings and menus (T557), opening diffs from the workbench (T551).

## Tests

New files `crates/fe-editor/src/row_changes_supplied_tests.rs` (declared `#[cfg(test)]` from row_changes.rs) and
`crates/fe-gpui/src/diff_view_compare_tests.rs` (declared `#[cfg(test)]` from diff_view.rs). Close-order tests go
through a kiln-shell pane's `close_active_item` and `has_pending_prompt`, as `kiln/crates/kiln-shell/tests/layout.rs`
does.

## Implementation notes

- Supplied mode lives in `RowChangeState` (`supplied: Option<Arc<[RowChange]>>` beside the per-version cache): a
  version not yet supplied gets the last changes supplied, and nothing diffs until `set_diff_base`, which leaves
  supplied mode even when given the same text again.
- A too-large comparison supplies an empty set of changes (with the other side's text as base), so no stale bars stay.
- `Catalog::check` walks the editor, vim, proposal and diff registries in order before the app's, rather than nesting
  one more `match`.
- Carried T118 F2 is covered by C2's `]c` / `[c` on an editable right side with a `HEAD` base on the file, side by
  side and inline. Carried T118 F3: `visited` keeps `[Option<usize>; 2]`, `None` for the left side inline, so a left
  caret at offset 0 no longer matches a fictitious record after switching back. The regression test is
  `diff_view_compare_tests::inline_navigation_leaves_no_left_caret`; `inline.rs` needed no change.
- Scope widened (rev 2→3) to `crates/fe-gpui/src/diff_view_tests.rs`: `large_sides_diff_off_the_main_thread`
  asserted that a large left side had no row changes. It now has the changes from the landed diff.
