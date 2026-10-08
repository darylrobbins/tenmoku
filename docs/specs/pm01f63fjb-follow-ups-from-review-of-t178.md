---
task_uid: "01m4ep81htk0c5h5pm01f63fjb"
task: "T207"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T178

Findings of the review of T178 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/insert.rs:433: Unpublished completions still overwrite newer changes. In the measuring callback, after the clone dispatches normal_mode, dispatch rZ before returning: adopt_closing sees Ending and returns, rZ updates last_change, and the outer shutdown subsequently publishes Ended. The clone's next repeat then unconditionally adopts the old insert, repeating the wrong change. The new regression dispatches rZ only after the outer shutdown returns, so it misses this transition.
  fix: Track change ordering so an older completion cannot replace a newer last_change, or defer editing commands until the closing session has finished. Add a regression that dispatches the intervening edit inside the measuring callback and verifies subsequent dot behavior.
  instances: crates/fe-vim/src/insert.rs:433

- F2 medium crates/fe-vim/src/history.rs:82: Moving the original pending carets into Closing does not preserve first-command carets when commands proceed before publication. A same-origin rZ inside the measuring callback joins the still-open original transaction and records newer carets in entries; settle then calls record, which refuses to replace that entry with the original carets. Starting another insert in that callback also reuses the original transaction ID through nesting: the outer close returns None, so settle discards the original record, and the later session records its newer position when the transaction finally closes. Undo/redo therefore restore the wrong caret.
  fix: Preserve the earliest caret record by transaction ID across detached completions, ordinary edits, and nested sessions, retaining it until the transaction actually closes; alternatively defer these commands until closure. Test both an intervening normal edit and a second insert started while End is Ending, checking undo/redo caret positions.
  instances: crates/fe-vim/src/history.rs:80, crates/fe-vim/src/history.rs:82, crates/fe-vim/src/insert.rs:1563
