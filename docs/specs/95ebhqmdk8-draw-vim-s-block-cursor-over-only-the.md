---
task_uid: "01m5gfj8s8w3epk695ebhqmdk8"
task: "T275"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Regression test block_starts_after_the_hint_at_the_caret_offset (new crates/fe-display/src/layout/hint_block_caret_tests.rs, Linux-OK, fake Mono measurer): for a row with a parameter hint at offset o, the block span for a caret at o starts at the hint's right edge and ends at the next grapheme's x, so its width is exactly one character; it fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Same file: a caret at an offset with no hint keeps today's block (start = caret x); a caret at o with two hints at o starts after both; a type hint after the character (e.g. 'batch: u64' with the caret on 'h') is not covered by the block on 'h'; a caret at end of row after a hint draws the one-cell block after the hint"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui element.rs block_rect (~:2130-2151) starts the block at RowLayout's new x_after_hints(offset) instead of caret_bounds(head).x, so vim normal and visual block cursors, the replace-mode underline/composition block and jump labels all sit on the character, not the hint; the bar caret, clicks and the_caret_never_lands_inside_a_hint (inlay_hints_tests.rs) are unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-display and -p fe-gpui pass"
    required: true
    manual: false
---

# Draw vim's block cursor over only the character after an inlay hint, not the hint and the character (Mac T1081)

User report 2026-10-21: "Vim cursor movement for inlay hints is a little odd. It should probaly just skip over the hint." Screenshot (main 2277f74c, vim NORMAL, rust-analyzer hints on): line 107 `let batch: u64 = GRAMMAR_BATCH.fetch_add(val: 1, order: Ordering::SeqCst) + 1;`, status Ln 107 Col 52 (caret on the `O` of `Ordering`). The block cursor covers the whole `order: ` hint AND the `O`. VS Code + VSCodeVim and Neovim draw the block on the `O` only; the hint stays drawn as a hint beside it.

## Problem
Paint only. Motions are fine: fe-vim never sees hints (goal column is buffer columns, crates/fe-vim/src/line_motions.rs:659-715, curswant state.rs:102), so h/l/w/b/j/k already step over hints.
crates/fe-gpui/src/element.rs `block_rect` (:2130-2151), used by `block_carets` (:2115) for normal and visual block cursors, the replace-mode underline/composition block (:1131-1132, :2175) and jump labels (:2565), starts the block at `frame.caret_bounds(head, ..).x` (:2139). That is `RowLayout::x_for_offset` (crates/fe-display/src/layout.rs:326-348, via :552-565), the x BEFORE any hint at that offset, which is right for the bar caret and clicks (pinned by `the_caret_never_lands_inside_a_hint`, crates/fe-display/src/layout/inlay_hints_tests.rs ~:168). The block's end (:2145) is x_for_offset(next grapheme), after the hint and the char, so the block spans hint + char.

## Fix
- Add `RowLayout::x_after_hints(offset)` in fe-display layout.rs: the x where the offset's own character starts, after every inlay/hint anchored at that offset (equal to x_for_offset when none).
- In block_rect, start the block at x_after_hints(head) and keep the end at the next grapheme's x. Leave caret_bounds, the bar caret and offset_for_x alone.

## Tests
New crates/fe-display/src/layout/hint_block_caret_tests.rs, wired with a #[path] mod beside inlay_hints_tests (Linux-OK): block_starts_after_the_hint_at_the_caret_offset (regression) plus the cases in the criteria. No screenshot goldens change.
