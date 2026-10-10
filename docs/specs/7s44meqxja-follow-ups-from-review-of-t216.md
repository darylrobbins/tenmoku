---
task_uid: "01m4hgyva37za2357s44meqxja"
task: "T307"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T216

Findings of the review of T216 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/ex.rs:182: A double quote is consumed as an empty register argument instead of terminating the command. Consequently, :d " 3 deletes three lines, whereas Neovim 0.12.4 deletes only the current line because the remainder is a comment.
  fix: Treat a command-argument double quote as a comment terminator, including after a register or count, and add oracle cases covering numeric and textual comments.
  instances: crates/fe-vim/src/ex.rs:182, crates/fe-vim/src/ex.rs:215

- F2 medium crates/fe-vim/src/ex.rs:355: Summing offsets before validation hides intermediate overflow. From line 1, :1+2147483647-2147483647d resolves to line 1 and deletes it; Neovim 0.12.4 rejects the command with E1247 and leaves the buffer unchanged.
  fix: Preserve individual offset operations and apply Neovim-compatible overflow checks while evaluating them; add cases where a later offset cancels an earlier overflow.
  instances: crates/fe-vim/src/ex.rs:355, crates/fe-vim/src/ex.rs:491

- F3 medium crates/fe-vim/src/ex.rs:310: Address parsing accepts only contiguous searches followed by offsets. Valid ranges such as :1 /x/d, :1+1/x/d and :/x/+1/x/d are rejected, although Neovim accepts them. Separating searches from offsets also prevents preserving their evaluation order.
  fix: Parse and resolve an ordered sequence of searches and offsets, allowing whitespace between address components, and record cases for these combinations.
  instances: crates/fe-vim/src/ex.rs:310, crates/fe-vim/src/ex.rs:335, crates/fe-vim/src/ex.rs:488

- F4 medium crates/fe-vim/src/ex.rs:454: Negative addresses are rejected before later addresses can replace them and before command validation. From line 1, :-2,1,2d incorrectly returns E16 instead of deleting lines 1–2; :-2frob returns E16 instead of E492, and :-2noh returns E16 instead of E481.
  fix: Separate address-resolution failures from final range validation, validate retained endpoints at the appropriate command stage, and preserve semicolon side effects; add oracle and message assertions for these cases.
  instances: crates/fe-vim/src/ex.rs:454

- F5 medium crates/fe-vim/src/ex.rs:601: Post-delete cursor placement uses the original numeric row after display synchronization, overwriting the anchored cursor that operator::operate already established. If another model inserts a row above the deletion during measurement, :2d can land on the preceding line instead of the surviving replacement line.
  fix: Normalize the first non-blank column on the operator's anchored landing row, rather than recomputing the row from first, and add a Measuring test with another-origin insertion above the deleted range.
  instances: crates/fe-vim/src/ex.rs:601

- F6 medium crates/fe-vim/src/ex.rs:586: Deleting an already empty buffer reaches the linewise operator, which writes a newline into the delete registers despite removing no text. Neovim preserves existing registers in this case, so :%d followed by :d incorrectly overwrites the previously deleted text and rotates the numbered registers.
  fix: Make ex deletion of an empty buffer a no-op after validating the command and range, preserving registers and history; add empty-buffer and repeated-delete corpus cases.
  instances: crates/fe-vim/src/ex.rs:586
