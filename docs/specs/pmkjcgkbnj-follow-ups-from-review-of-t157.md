---
task_uid: "01m69qwxndy5babapmkjcgkbnj"
task: "T388"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T157

Findings of the review of T157 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/replace.rs:1153: Count replay falls back to R's copies whenever the recorded keys contain both a newline and backspace, so surviving breaks insert rows instead of replacing their remainder and advancing. For example, 2gRx<CR><BS>y<CR>z<Esc> on abcdef\nghijkl\nmnopqr from (0,0) produces xy\nzxy\nzkl\nmnopqr instead of xy\nzxy\nznopqr.
  fix: Support backspaces in virtual replay with the same row replacement and restoration semantics as interactive typing. Add oracle cases for counts and counted dot containing both backspaces and surviving breaks.
  instances: crates/fe-vim/src/replace.rs:1153, crates/fe-vim/src/replace.rs:1174

- F2 medium crates/fe-vim/src/replace.rs:1213: virtual_copies starts with unused indentation set to zero and cannot edit before head, losing the pending indentation from the interactive copy. Consequently, 2gR<CR><Esc> on "  abc\ndef\nghi" from (0,2) leaves two spaces on the middle row instead of producing "  \n\n  i".
  fix: Carry pending break indentation into count replay and allow its first break to remove those bytes before head. Add coverage for counts whose keys begin and end with a break.
  instances: crates/fe-vim/src/replace.rs:1213, crates/fe-vim/src/replace.rs:1277

- F3 medium crates/fe-vim/src/replace.rs:603: The added flag classifies the entire multicaret batch as an added line break when any caret reaches EOF. Existing-row indentation then enters the cleanup ledger and can disappear at Esc. With carets at (0,2) and (2,2) in "  abc\nxx\n  def", gR<CR><Esc> incorrectly removes the two spaces replacing xx.
  fix: Track added-row indentation per caret or edit so only EOF additions enter the cleanup ledger. Add a multicaret regression mixing an existing-row break with an EOF break.
  instances: crates/fe-vim/src/replace.rs:603
