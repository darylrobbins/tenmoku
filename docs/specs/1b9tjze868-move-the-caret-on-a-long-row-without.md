---
task_uid: "01m4pp8rmmqk2s131b9tjze868"
task: "T20"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor tests in tests/long_row_moves.rs pass: left, right, word-left, word-right, line-start, word_range_at and newline's indent on rows over 1 MiB match a whole-row unicode-segmentation oracle at positions near the 1 KiB window edges, with ZWJ, combining-mark, regional-indicator and CRLF cases straddling an edge, a cluster over 2 KiB and a word run longer than the window"
    required: true
    manual: false
  - id: "C2"
    text: "EditorModel::row_at is gone and no movement, word_range_at or newline path in crates/fe-editor/src/lib.rs calls Text::row_to_string"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing fe-editor test passes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "PERF.md's Typing latency section records move/long-row's dispatch p50/p99 before and after, compared by fastest sample with base and candidate bench binaries copied out and run alternately"
    required: true
    manual: false
---

# Move the caret on a long row without copying the whole row on each arrow key (Mac T407)

Filed on helium from Mac T407 (~/coordination/helium-cards/T407.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-f.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note (Mac lead, note f): any numbers you record in PERF.md are measured on helium (Linux): label them so, next to the Mac rows, and never overwrite or remove Mac numbers.

## Problem

`EditorModel::row_at` (`crates/fe-editor/src/lib.rs:770`) returns the whole text of the caret's row as a
`String` (`Text::row_to_string`). Several paths call it:

- `prev_grapheme` and `next_grapheme`, for every left or right arrow;
- `prev_word` and `next_word`;
- the `LineStart` move (`lib.rs:715`);
- `word_range_at` (`lib.rs:627`), for double-click;
- `newline` (`lib.rs:890`), which copies the row only to read its indent.

On a 10 MB minified one-line file, each arrow key therefore copies 10 MB. The long-row slices decision
(`docs/decisions/ra1pz1qex0-long-row-slices.md:132`) calls this "the known follow-up". The keystroke bench
case `move/long-row` (added by the card this depends on) measures it.

## Change

Make each caller read only the part of the row it needs. `row_at` goes away.

- **Grapheme moves:** read a window of the row around the offset with `Text::slice_to_string`, 1 KiB on
  each side, clamped to the row and snapped to char boundaries. Find the boundary in that window. If the
  boundary found is at the window's edge (a grapheme cluster could continue past it), double the window and
  retry. A cluster longer than the whole row ends at the row as before.
- **Word moves and `word_range_at`:** scan with `Text::chunks_in_range` from the offset outwards and stop at
  the first class change. Don't copy the row.
- **`LineStart` and `newline`:** read only the row's leading whitespace, chunk by chunk from `row_start`.
- Behaviour is unchanged. For every caret position, every move returns the same offset as before.

New tests go in `crates/fe-editor/tests/long_row_moves.rs`. They compare each move with an oracle in the
test that segments the whole row with `unicode-segmentation`, as the old code did. Cover:

- positions within 4 bytes of the 1 KiB window edges;
- a ZWJ family emoji, a run of combining marks, and regional-indicator pairs that straddle an edge;
- a CRLF row ending;
- a grapheme cluster longer than 2 KiB;
- a word run longer than the window.
