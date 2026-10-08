---
task_uid: "01m4bvnm4jjcxf56smyn1bfb1s"
task: "T180"
rev: 5
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T162

Findings of the review of T162 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/insert.rs:246: Non-mutating commands split only when selections change. Copy, save, wrap toggle, and fold/unfold commands that leave carets unchanged therefore merge subsequent typing into the previous undo step. For example, i, type ab, alt-z, type cd, Esc, u removes abcd instead of just cd. The spec requires these non-text editor commands to split.
  fix: Distinguish caret-motion commands from other non-text commands and split unconditionally for the latter. Add undo-grouping tests for copy, save, wrap toggle, and folds without caret movement.
  instances: crates/fe-vim/src/insert.rs:246, crates/fe-gpui/src/commands.rs:274

- F2 medium crates/fe-vim/src/insert.rs:1564: Indentation cleanup calls set_selections when deletions shift caret offsets, which clears EditorModel's vertical goal columns. From "    a\nb\nabcdef" at (0,4), o down down Esc consequently lands at (3,0), whereas Neovim 0.12.4 retains goal column 4 and lands at (3,3). Thus ordinary vertical movement changes behavior after a split that removes indentation before the caret.
  fix: Preserve vertical goals during cleanup, using the model's anchored selections where possible instead of resetting them. Add the two-down-arrow regression case.
  instances: crates/fe-vim/src/insert.rs:1564

- F3 medium crates/fe-gpui/src/view.rs:287: The new mouse split wrapper covers text-area clicks, but on_mouse_down returns earlier for fold-chevron clicks. Folding a region containing the caret moves it to the header through fold_rows without splitting the insert transaction. Typing before and after that mouse-induced movement remains one undo step, contrary to the mouse-click requirement.
  fix: Route fold-click mutations through the session-splitting path too. Test typing inside a foldable body, clicking its chevron, typing on the header, and independently undoing the latter typing while insert mode remains active.
  instances: crates/fe-gpui/src/view.rs:287

- F4 medium crates/fe-gpui/src/commands.rs:270: Insert-mode interception occurs before registry validation. The newline special case bypasses validation altogether, so editor.newline with nonempty arguments inserts a newline instead of returning BadArgs. Other intercepted mutating commands end the session before dispatch rejects bad arguments; this can remove unused indentation and change undo grouping even though the command never runs.
  fix: Validate the invocation before any newline handling or session splitting. Add tests showing malformed newline and paste/cut invocations leave text, selections, mode, and undo grouping unchanged.
  instances: crates/fe-gpui/src/commands.rs:270, crates/fe-gpui/src/commands.rs:274, crates/fe-gpui/src/vim.rs:254
