---
task_uid: "01m4p6c3q0s3hf621hsyckmwae"
task: "T14"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test viewer_words::word_moves_match_the_editor: over random texts from tests/viewer.rs's alphabet, editor.move_word_left/right, editor.select_word_left/right and editor.move with word motions and counts leave a ViewerModel with the same selection as EditorModel over the same text, from every offset"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test viewer_words::reads_only_around_the_caret: on a 64 MiB single-row text behind a TextSource that counts the bytes chunks_in_range yields, a word move from the middle reads under 1 MiB"
    required: true
    manual: false
  - id: "C3"
    text: "In crates/fe-editor/tests/viewer.rs the four word commands are out of the unsupported list and among the differential moves; crates/fe-editor/src/lib.rs is unchanged"
    required: true
    manual: false
---

# Move by word in the read-only file viewer, reading only the text around the caret (Mac T396)

Filed on helium from Mac T396 (~/coordination/helium-cards/T396.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-c.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

`ViewerModel` (`crates/fe-editor/src/viewer.rs`, T273), the read-only model behind the large-file
viewer, doesn't move by word: `move_cursor` returns early for `WordLeft`/`WordRight` (viewer.rs:144-149,
"Word moves aren't supported yet") and `run` answers `Outcome::Unsupported` for them (:197, :211).
alt-left/alt-right and their shift forms do nothing in the viewer, though they work in the editor.
`crates/fe-editor/tests/viewer.rs:22` and :222-236 record the gap.

`EditorModel::next_word`/`prev_word` (`crates/fe-editor/src/lib.rs:805-853`) copy the whole row
(`row_at`). That's not acceptable here: a file the viewer shows can be one 4 GB row.

## Change

In `viewer.rs`, implement `Movement::WordLeft`/`WordRight` in `moved`, and let `run` take
`editor.move` with word motions and the word movement commands. Match `EditorModel`'s rules
exactly: skip whitespace, then a run of one `CharClass` (the crate root's private `CharClass`,
reachable from the `viewer` child module without changing `lib.rs`); at a row's end move past the
newline; at a row's start move back over it. Read only the text the move needs, through
`TextSource::chunks_in_range` from the caret towards the row's end (or start) in chunks, stopping
when the run ends; never materialize the row.

Tests: move the four word commands out of the unsupported list in `crates/fe-editor/tests/viewer.rs`
and into its differential moves, and add the new file `crates/fe-editor/tests/viewer_words.rs`.
