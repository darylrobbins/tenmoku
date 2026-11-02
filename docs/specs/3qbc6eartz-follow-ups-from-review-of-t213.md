---
task_uid: "01m6d5pc0rk5rbc03qbc6eartz"
task: "T407"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T213

Findings of the review of T213 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/feature/line_undo.rs:121: Undo/redo events are treated as fresh edits because their EditKind is discarded. With buffer "zero\nabc\ndef\nlast", cursor [1,0], and keys "xjxuuU", Neovim preserves the restored buffer and lands on row 2; this implementation starts a new run when undo restores row 1, then U replaces "abc" with "bc".
  fix: Distinguish history restoration from fresh edits when updating the saved U line, matching Neovim's behavior. Add oracle cases covering undo and redo across different rows.
  instances: crates/fe-vim/src/feature/line_undo.rs:121

- F2 medium crates/fe-vim/src/feature/line_undo.rs:159: Whitespace and newline shapes do not identify the operations that clear Neovim's U line. On "zero\nabc\n\nlast" at [1,0], "xjddU" incorrectly restores "abc" although Neovim leaves "bc", while "xjyyGpU" incorrectly clears the saved line although Neovim restores "abc". On "zero\nabc\ndef\nlast", "xjvjdU" also incorrectly clears the saved line for a characterwise multiline deletion.
  fix: Track the relevant operation semantics explicitly instead of inferring linewise deletion, opening, and putting solely from whitespace content. Add oracle coverage for blank-line deletion and putting, plus characterwise multiline deletion.
  instances: crates/fe-vim/src/feature/line_undo.rs:159, crates/fe-vim/src/feature/line_undo.rs:160

- F3 medium crates/fe-vim/src/feature/line_undo.rs:65: Cloned Vim states share a destructively drained subscription but retain independent text and line snapshots. A command through one clone removes events the other needs, leaving its snapshot inconsistent with subsequent edit ranges. For example, clone the state after x, execute U through the clone, then U through the original: the original can save the restored text instead of toggling back to the deleted text.
  fix: Give each clone an independent replay position over retained events, following the existing marks Feed pattern, or share the complete tracker consistently. Add a regression test alternating commands between cloned states.
  instances: crates/fe-vim/src/feature/line_undo.rs:65
