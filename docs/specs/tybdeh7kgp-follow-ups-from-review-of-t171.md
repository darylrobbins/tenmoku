---
task_uid: "01m4br9ffn82cqhwtybdeh7kgp"
task: "T178"
rev: 4
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T171

Findings of the review of T171 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/insert.rs:115: Closing outcomes are adopted only before repeat and history commands. After the outer shutdown publishes Ended, a retained clone can execute a newer change such as `rZ` before `.`; adoption then unconditionally replaces that newer last_change with the old insert, so dot repeats the wrong change. Starting another insert/change session also overwrites Carets.pending before the old completion is adopted, permanently losing the old transaction's caret record. If adoption occurs while the newer session is pending, close() can discard that newer record too. The regression covers only immediate dot after shutdown.
  fix: Adopt published completions before every command that can replace last_change or modify pending caret records, preferably through a common command boundary. Preserve transaction-specific pending records if commands can proceed while completion is still unpublished. Extend the retained-clone regression with an intervening normal edit and a second insert/change session, verifying dot and undo/redo caret positions for both transactions.
  instances: crates/fe-vim/src/insert.rs:115, crates/fe-vim/src/insert.rs:117, crates/fe-vim/src/repeat.rs:97, crates/fe-vim/src/history.rs:127
