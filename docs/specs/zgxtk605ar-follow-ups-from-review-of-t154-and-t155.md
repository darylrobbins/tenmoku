---
task_uid: "01m4abnbk7611s5pzgxtk605ar"
task: "T165"
rev: 2
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T154 and T155

Findings of the review of T155 that don't block the merge (below merge.block_severity):

- F7 medium crates/fe-vim/src/motion.rs:788: Overlapping-marker classification remains incorrect: !after_star suppresses a // line comment following a stray */. On "**// /* */" or "*//x /* */", with the caret on either byte of the final */, nvim 0.12.4 cancels %, d% and y%, but comment_open returns column 5. Consequently d% deletes text and changes registers instead of leaving everything unchanged.
  fix: Resolve overlapping // and */ markers using the recorded behavior rather than the preceding-byte shortcut, preserving linear complexity; record %, d% and y% regressions at both closing-marker bytes for both examples.
  instances: crates/fe-vim/src/motion.rs:788

- F10 medium crates/fe-vim/src/motion.rs:783: The new earliest-opener condition incorrectly disqualifies an opener whose own star participates in an overlapping */. On "/*//x/* */" at [0,8] or [0,9], nvim matches column 0, while this code selects column 5 because last_close is 1. This regresses the previous implementation: d% at column 8 leaves "/*//x/" instead of "/", and y% captures the wrong range.
  fix: Distinguish closers overlapping an opener from closers occurring after its opening marker when selecting the earliest still-open candidate; add motion, delete and yank regressions at both closing-marker bytes while retaining linear scans.
  instances: crates/fe-vim/src/motion.rs:783


Folded in from T159 (abandoned in favour of this task; same file):

- F4 medium crates/fe-vim/src/motion.rs:240: The new VS16 rule incorrectly widens every grapheme containing U+FE0F. Neovim 0.12.4 keeps keycaps, non-emoji bases and some intervening-combining-mark sequences one column wide. For buffer "x1️⃣y\nabcdef", cursor (0,8), and j, Neovim lands at (1,2), while this code computes (1,3). This regression affects goal columns and destination selection for j, k, gg and G.
  fix: Widen only sequences Neovim actually treats as two columns, using recorded sequence behavior to determine eligible bases and selector placement. Add recorded cases moving from and onto keycaps, non-emoji VS16 sequences and selectors following another combining mark.
  instances: crates/fe-vim/src/motion.rs:240
