---
task_uid: "01m4rbvtetg2cnjxz6yzsf5sep"
task: "T48"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "occurrences_tests: occurrence_target gives the word for a caret in or just after it and for a selected whole word, and None for punctuation, whitespace, several carets, a multi-row selection and a partial word"
    required: true
    manual: false
  - id: "C2"
    text: "occurrences_tests: decorations lists the whole-word, case-sensitive matches overlapping the range, the target's own included, as Highlight with Tint::Muted at priority -50; foo matches neither foobar nor _foo; a match straddling the range's end is included"
    required: true
    manual: false
  - id: "C3"
    text: "occurrences_tests: set_target returns whether the target changed; after an edit inside the target word decorations yields nothing, and after an edit elsewhere the matches follow the text"
    required: true
    manual: false
  - id: "C4"
    text: "occurrences_tests: a range holding 5,000 matches yields at most 1,000, and a word that occurs only outside the queried range (beyond its length) yields nothing"
    required: true
    manual: false
---

# Highlight the other uses of the word at the caret: an OccurrenceHighlights decoration provider in fe-editor, headless (Mac T492)

Filed on argon from Mac T492 (card ~/coordination/argon-cards/T492.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12 note l (relayed by the user).md). Mac task uid 01m4rbgwp1kt3k4wsae2k4xr5v. Expected: one out-of-scope notice for the `pub mod occurrences;` line in crates/fe-editor/src/lib.rs (deliberately left out of scope by the Mac lead); that is its only change there. Title the host commit "... (Mac T492)".

## Problem

Users browse code more than they edit it (~/.claude/plans/glittery-foraging-graham.md, Context). Seeing every
other use of the identifier under the caret in view is the cheapest navigation aid an editor has, and
fastedit has none: the only word matching is multi-cursor's select-next and select-all occurrences
(crates/fe-editor/src/multi_cursor.rs:124, :168) and vim's search highlight. The decoration seam
(`DecorationProvider`, crates/fe-editor/src/decorations.rs:118, painted since ac01dc63) can show it without
any fe-gpui painting change.

## Change

Add `OccurrenceHighlights`, a `DecorationProvider`, in a new crates/fe-editor/src/occurrences.rs. Its only
line in crates/fe-editor/src/lib.rs is the `pub mod occurrences;`, left out of the scope on purpose so this card doesn't block the other fe-editor cards that own lib.rs: expect one out-of-scope notice for it. Registering it, updating it on caret moves
and the on/off setting are a later Mac card in fe-gpui; this card is headless.

- `EditorModel::occurrence_target(&self) -> Option<Range<usize>>`, in an `impl EditorModel` block in
  occurrences.rs: with exactly one selection, a caret's `word_range_at` when that range is a run of word chars
  (alphanumeric or `_`), or a non-empty selection on one row that equals `word_range_at` of its start; else
  `None` (several carets, punctuation, whitespace, a multi-row or partial-word selection).
- `OccurrenceHighlights::set_target(&mut self, snapshot: &BufferSnapshot, target: Option<Range<usize>>)`
  keeps the word's text and its range as anchors. Words over 256 bytes are no target. Returns whether the
  target changed, so the host repaints only then.
- `decorations(snapshot, range, out)`: every whole-word occurrence of the target text overlapping `range`,
  case-sensitive, found with multi_cursor.rs's `each_match` and `is_whole_word` (made `pub(crate)`, unchanged
  otherwise), as `DecorationKind::Highlight` with `Tint::Muted` at priority -50, the target's own occurrence
  included. At most 1,000 per call. Nothing when the target's anchored range no longer holds the word's text
  (it was edited) or the target is `None`.
- Search only `range` widened by the word's length on each side, clipped to the text, so cost follows the
  painted range, never the file.

Tests go in a new crates/fe-editor/src/occurrences_tests.rs, included from occurrences.rs with
`#[cfg(test)] #[path = "occurrences_tests.rs"] mod tests;`.
