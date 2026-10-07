---
task_uid: "01m4bjab3zbhefxx70841xj1xv"
task: "T171"
rev: 4
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T156

Findings of the review of T156 that don't block the merge (below merge.block_severity):

- F12 medium crates/fe-vim/src/insert.rs:976: A clone exiting while the shared session is Ending permanently loses the closing result. Its session was already taken, and this branch returns without updating last_change or promoting the pending undo caret record. When the outer shutdown publishes Ended, the clone has no remaining link to consume it. In the new 3ix reentrancy regression, dot through that clone subsequently does nothing; with a preceding change, it repeats the wrong change. The test checks only the clone's mode and the original state's outcome.
  fix: Retain a shared completion handle for reentrant exits and adopt the published last change and caret record before subsequent normal-mode commands. Extend the regression to retain the clone after outer shutdown and verify its dot-repeat and undo/redo caret restoration.
  instances: crates/fe-vim/src/insert.rs:976


Added by the lead from the push security review of 0be0c71 (T156):

- F-actor low crates/fe-vim/src/commands.rs: vim's command handlers ignore the invoking Actor (`_: &Actor`), so every vim edit, insert, put, undo and redo runs as the user. fe-editor's commands run through `EditorModel::run_as(origin(actor), …)` (crates/fe-editor/src/commands.rs `run_as`), so an edit dispatched as `Actor::Ai` or `Actor::Lsp` gets its own origin. Through vim, such an edit would land in the user's undo history, and an AI-dispatched `vim.undo` would undo the user's edits.
  fix: run vim's mutating handlers and vim.undo/vim.redo as the invoking actor, the way fe-editor's run_as does, and drop the lib.rs known-limit note that undo acts on "the user's, whatever the actor". Test (in crates/fe-vim/src/tests/commands.rs): a vim.operate dispatched as Actor::Ai is undone by an Ai vim.undo but not by a User vim.undo.
