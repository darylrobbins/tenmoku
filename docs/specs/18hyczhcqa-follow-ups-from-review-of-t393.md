---
task_uid: "01m4p54t8qxk7c2a18hyczhcqa"
task: "T438"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T393

Findings of the review of T393 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/visual/operate.rs:179: Block replay loses its saved columns when extent_from's clamped endpoints are passed to area, which reconstructs the block using endpoint grapheme widths. From (0,0), <C-v>jll<C-a>jj. on "111\n111\n1234\n1" consequently changes the third row to "2234" instead of Neovim's "1244". A short endpoint can also move the left boundary: from (0,2), <C-v>jl<C-a>jj. or <C-v>j$<C-a>jj. on "a 11\na 11\n9 12\nx" changes the third row's 9 instead of producing "9 13", and lands at column 0 instead of 2. Conversely, a tab endpoint widens a finite replay. The added short-row test uses single-digit numbers and cannot detect this geometry loss.
  fix: Construct replay blocks directly from the starting caret's display column and the saved width or end-of-line extent, independently of the final row's clamped endpoint. Use those bounds for both row windows and landing. Add corpus cases covering shrinking, leftward movement, and widening with short, empty, and tab endpoint rows.
  instances: crates/fe-vim/src/visual/operate.rs:179
