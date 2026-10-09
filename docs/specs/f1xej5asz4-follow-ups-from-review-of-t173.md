---
task_uid: "01m4c7nyph8yrfd3f1xej5asz4"
task: "T182"
rev: 5
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F11: v$o$iw/iW at a row end after trailing blanks (spaces and tabs, with and without a count) selects the blank run and grows forward as Neovim does; \"abc   \\ndef\" at (0,0) with v$o$iwd leaves \"abc\\ndef\""
    required: true
    manual: false
  - id: "C2"
    text: "F12: backward iw/iW growth keeps the visual kind Neovim keeps after the cursor moves or reaches the start, in both the ordinary and the row-end branch (Vh2iWy and V$o$3iWy yank linewise)"
    required: true
    manual: false
  - id: "C3"
    text: "F13: backward iw/iW onto an initial whitespace-only row stops where Neovim does (\"   \\nabc\" at (1,1), vhiWd leaves \" c\"), for iw and iW, including a second count that reaches column zero"
    required: true
    manual: false
  - id: "C4"
    text: "Every case is recorded against Neovim in the new tests/corpus/visual_words area and checked by tests/visual_words_oracle.rs; the existing visual corpus still passes"
    required: true
    manual: false
---

# Follow-ups from review of T173

Findings of the review of T173 that don't block the merge (below merge.block_severity):

- F11 medium crates/fe-vim/src/visual.rs:560: The new row-end branch always grows iw/iW backwards, including after trailing whitespace, where Neovim selects the whitespace run and grows forward. On "abc   \ndef" at (0,0), v$o$iwd now deletes the final blank and newline, producing "abc  def"; Neovim deletes the three blanks and leaves "abc\ndef". Counted variants also select the wrong extent.
  fix: Distinguish row ends following whitespace from those following nonblank text, preserving the whitespace object's anchor and forward growth; record iw/iW regressions with trailing spaces and tabs, with and without counts.
  instances: crates/fe-vim/src/visual.rs:560, crates/fe-vim/src/visual.rs:563

- F12 medium crates/fe-vim/src/visual.rs:585: Backward growth only preserves the visual kind when the cursor does not move. When an iw/iW reaches or exhausts the beginning after moving, the helper returns an ordinary offset and the default charwise kind wins. On "abc" at (0,2), Vh2iWy yanks charwise "abc", whereas Neovim retains linewise mode and yanks "abc\n"; the row-end branch has the same problem with V$o$3iWy.
  fix: Return enough boundary/completion information from backward growth to preserve the original kind when Neovim does, including after movement; record both ordinary backward and row-end cases through a yank or delete.
  instances: crates/fe-vim/src/visual.rs:563, crates/fe-vim/src/visual.rs:585, crates/fe-vim/src/visual.rs:587

- F13 medium crates/fe-vim/src/visual.rs:683: Backward iw/iW consumes an extra blank when crossing onto an initial whitespace-only row. On "   \nabc" at (1,1), vhiWd produces "c", whereas Neovim leaves " c". This divergence also existed in the previous implementation, so it is a follow-up.
  fix: Implement Neovim's first-row whitespace boundary behavior and record both iw and iW cases, including a second count that legitimately reaches column zero.
  instances: crates/fe-vim/src/visual.rs:683
