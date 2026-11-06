---
task_uid: "01m6pw50sysj7ckrjt361ysycs"
task: "T456"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T445

Findings of the review of T445 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F1 medium crates/fe-vim/src/replace.rs:1086: Multiline characterwise plain <C-r> and <C-r><C-r> now create stackless entries with empty replay text, while register-key recording excludes both forms. Consequently, counts and dot replay omit the insertion entirely. For example, on "ab\ncd\nxyz tail", vjyj02R<C-r>0<Esc> inserts only once; Neovim produces "ab\nab\ncab\nccd\nxyz tail". This regresses replay and leaves the requested shared model incomplete.
  fix: Retain replayable insertion data for both multiline forms, preserving their insertion semantics and subsequent backspaces. Add count, dot, and consecutive-dot oracle coverage.
  instances: crates/fe-vim/src/replace.rs:1086, crates/fe-vim/src/insert/keys.rs:942

- F2 medium crates/fe-vim/src/insert/keys.rs:1518: Count replay clears the restoration stack after inserting a register and creates no stackless boundary. A following backspace therefore neither restores the preceding overwritten character nor performs the required movement or row join. On "abc\nxyz uvw", yiwj2RQ<C-r><C-o>0<BS><Esc> produces "abc\nQabQabcyz uvw" through this path, whereas Neovim produces "abc\nQabQabxyz uvw". C-p and linewise register insertions have the same defect.
  fix: Use the replace-stack transitions during count replay, retaining prior overwritten entries across register insertions and supporting empty-stack movement and row joins. Add characterwise and linewise counted-backspace oracle cases for both modifiers.
  instances: crates/fe-vim/src/insert/keys.rs:1518

- F3 medium crates/fe-vim/src/replace.rs:1079: Stack tracking is abandoned whenever insertion coalesces carets. Linewise C-o/C-p explicitly deduplicate carets on the same row, so this is a supported transition: the insertion succeeds, but ends.len() differs from heads.len() and no surviving caret receives a stackless entry. Immediate backspace then cannot find its boundary and becomes a no-op.
  fix: Map original caret records through insertion and coalescing, attaching a valid stackless boundary to each surviving caret instead of abandoning tracking. Add a regression with two carets on one row and a linewise insertion followed by backspace.
  instances: crates/fe-vim/src/replace.rs:1079

- F4 medium crates/fe-vim/src/visual.rs:180: The temporary anchor_before bias is never reset after register insertion. Later insertions above that row therefore leave visual marks on the newly inserted row. On "abc\ndef", viwyR<C-r><C-o>0<Esc>0yyP should leave '< and '> on the second row, as Neovim does; these anchors remain on the first row, so gv selects the wrong text. C-p follows the same path.
  fix: Reanchor the visual positions at their resulting row starts with normal anchor_after bias immediately after the protected insertion. Add a regression that inserts a register and subsequently puts a line above the marked row.
  instances: crates/fe-vim/src/visual.rs:180
