---
task_uid: "01m67g1gpr2ytq9g8chedc3y59"
task: "T378"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test diff_view_words_tests::one_word: a modified row pair differing in one word gets the word's range painted with the word tint on both sides, the rest of the row with the row tint"
    required: true
    manual: false
  - id: "C2"
    text: "diff_view_words_tests::pure_rows: an added row's whole text is word-tinted on the right, a removed row's on the left (VS Code does), and the filler rows opposite carry the hatch fill"
    required: true
    manual: false
  - id: "C3"
    text: "diff_view_words_tests::inline_mode: in inline mode the removed rows block shows the word tint on its changed words and the added row below it likewise"
    required: true
    manual: false
  - id: "C4"
    text: "tints come from named theme colours (added/removed row, added/removed word, filler hatch) in both dark and light themes; none of them is blue"
    required: true
    manual: false
  - id: "C5"
    text: "existing diff_view tests (compare, inline, navigation, language) pass; cargo test -p fe-gpui passes; clippy -D warnings clean"
    required: true
    manual: false
---

# Tint the changed words in a DiffView's modified rows darker than the row, and hatch the filler rows of the empty side, instead of one flat whole-row colour (Mac T1218)

## Problem
The user: "do a better job of showing line level changes beyond just highlighting the whole thing in blue". diff_view.rs `tinted_rows` gives Added/Removed/Modified row tints only. After T1216 (TextDiff::changed_spans) the exact characters are known. Filler rows are plain blank blocks. VS Code hatches the empty side diagonally. If GPUI can't draw a pattern cheaply, use a muted fill and say so in the finish notes.

## Implementation notes

- Five new `[editor]` theme keys, set in the three bundled fastedit themes
  (the others fall back to fastedit Dark or Light by appearance):
  `diff_added_row`, `diff_removed_row`, `diff_added_word`,
  `diff_removed_word`, `diff_filler`. They are VS Code's
  diffEditor.inserted/removedLineBackground, inserted/removedTextBackground
  and diagonalFill, and they are painted as the theme gives them, alpha
  included, not faded again.
- Two new tints, `Tint::DiffAdded` and `Tint::DiffRemoved`. As a row
  background they resolve to the `*_row` colour and as a highlight to the
  `*_word` colour (`element::decorations::diff_color`). Any other kind
  falls back to `change_added` / `change_deleted`. The left side is
  tinted removed and the right side added, modified rows included, so
  there's no blue (`change_modified`) left in a diff view.
- Words: within a modified hunk's row pairs these are
  `InlineDiff::changed_spans`. Rows with no partner (added and removed
  hunks, and a modified hunk's rows past the shorter side) are highlighted
  whole. A pair over the inline caps keeps its row tint alone. They are
  worked out with the diff, off the main thread when it is.
- Inline: `HostBlockKind::Lines` gained `words`, byte ranges per line,
  painted over the band in the tint's word colour.
- Fillers already had a diagonal hatch (GPUI strokes paths cheaply); it is
  now drawn in `diff_filler` rather than the text colour at 12%.
- Scope was widened by respec to the theme, tint, host-block and painter
  files this needs.
