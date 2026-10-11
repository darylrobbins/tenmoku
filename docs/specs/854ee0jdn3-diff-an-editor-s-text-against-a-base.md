---
task_uid: "01m4p1wnm7pt3s35854ee0jdn3"
task: "T2"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-editor/src/row_changes_tests.rs: with base \"a\\nb\\nc\\n\", inserting a row after b gives one Added change over that row, replacing b gives Modified over row 1, deleting b gives one Deleted change with rows 1..1; no base, or a base equal to the text, gives no changes"
    required: true
    manual: false
  - id: "C2"
    text: "A CRLF buffer against an LF base with the same rows gives no changes"
    required: true
    manual: false
  - id: "C3"
    text: "row_changes follows edits made after set_diff_base without it being called again, and a cfg(test) counter shows one diff per buffer version however often row_changes is called"
    required: true
    manual: false
  - id: "C4"
    text: "A base or text over 5 MiB gives no changes"
    required: true
    manual: false
---

# Diff an editor's text against a base text by row in fe-editor, as added, modified and deleted row ranges that follow every edit (Mac T374)

Filed on helium from Mac T374 (~/coordination/helium-cards/T374.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-a.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user asked on 2026-10-11 for "subtle highlighting of the changed rows in files" as part of git
support. The editor has no notion of a base text to compare against. Diagnostics are the model for this:
`EditorModel` (crates/fe-editor/src/lib.rs) holds a `DiagnosticSet` and `sync_display` keeps it current
with edits, and element.rs reads `view.model.diagnostics()` to paint. Changed rows need the same: a
GPUI-free, headless-testable answer to "which rows of the current text differ from the base", current
with every unsaved edit, not only with what's on disk.

## Change

Add `crates/fe-editor/src/row_changes.rs` and hook it into `EditorModel` with a few lines in
crates/fe-editor/src/lib.rs (a field, `pub mod row_changes;`, two methods):

- `EditorModel::set_diff_base(&mut self, base: Option<Arc<str>>)`: the text to compare against (the
  host passes the file's HEAD text; `None` turns highlighting off). Setting the same text again is a
  no-op.
- `EditorModel::row_changes(&self) -> Arc<[RowChange]>`: the current text's changes against the base,
  sorted by row, computed lazily and cached by buffer version in a `RefCell`, so many edits between two
  frames cost one diff. `RowChange { rows: Range<u32>, kind: RowChangeKind }`,
  `RowChangeKind::{Added, Modified, Deleted}`; rows are buffer rows of the current text. A `Deleted`
  change has an empty range at the row that now follows the removed rows (`rows.start == rows.end`; at
  the end of the text, the row count).
- The diff is a row diff with `imara-diff` (already a workspace dependency, used by
  crates/fe-buffer/src/proposed.rs; add it to fe-editor's Cargo.toml). Rows are compared by their
  content without the line terminator, so a CRLF buffer against an LF base, or the reverse, shows no
  change. Split the base into rows the same way `fe_text::Text` does for the buffer. A hunk with no old
  rows is `Added`, with no new rows `Deleted`, else `Modified` over its new rows.
- Limit: when the base or the text is over 5 MiB, `row_changes` is empty. Say so in the doc comment.

Tests in a new `crates/fe-editor/src/row_changes_tests.rs`, included from row_changes.rs with
`#[cfg(test)] #[path = "row_changes_tests.rs"] mod tests;` (not crates/fe-editor/src/tests/mod.rs).

## Out of scope

Painting (next card), reading git, word-level diffs, staging hunks.
