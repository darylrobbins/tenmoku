---
task_uid: "01m4rwgewmt4zxksjqpadchty1"
task: "T55"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test revert_changes::reverts_hunks_in_range: with a base and edits making three separate changes, revert_changes over the middle change's rows restores exactly those base rows, returns 1, leaves the other two changes, and row_changes() then lists two"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test revert_changes::one_undo_step: after revert_changes over two changes, one undo restores the text from before the revert exactly and redo reapplies it; the caret sits on the first reverted row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test revert_changes::edges: a deletion change reverts when the range holds its row or the row above; a range meeting no change and a model without a base return 0 and add no undo step"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-editor passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Revert the changes in a row range to the editor's diff base as one undo step (Mac T534)

Filed on argon from Mac T534 (card ~/coordination/argon-cards/T534.json rev 1). Mac task uid 01m4rhpgabezfsm0wqt9hr9p2b. Its dependency Mac T533 is on argon main since the aa07c7b sync, so it isn't recorded here. Title the host commit "... (Mac T534)".

## Problem

The user (2026-10-12) asked to "discard (revert) a hunk" from the gutter and the diff view, and the vim-mode
requirement (coordinator, same day) asks for `:Gitsigns reset_hunk` and diff mode's `do`. An editor already holds its
diff base (`EditorModel::set_diff_base`, `crates/fe-editor/src/lib.rs` :361) and its changed rows (`row_changes`), but
nothing puts the base's rows back.

## Change

In `crates/fe-editor/src/lib.rs`, `EditorModel::revert_changes(&mut self, rows: Range<u32>) -> usize`:

- Diff the base against the current text with `TextDiff::new(base, text)` (the text-diff card). Every hunk meeting
  `rows` (on the current text's side; a `Removed` hunk meets it when `rows` holds its insertion row or the row above)
  gets its current rows replaced by the base's rows. Whole hunks only: a partial range reverts each hunk it meets
  entirely (VS Code's Revert Change does the same per change).
- All of it is one edit, one undo step (`cmd-z`/`u` brings the change back), with the caret on the first row of the
  first reverted hunk. Returns how many hunks reverted; 0 (and no edit, no undo step) without a base or a hunk in
  `rows`.
- Read-only buffers are the host's business (the diff view card refuses there).

## Tests

New file `crates/fe-editor/tests/revert_changes.rs`.
