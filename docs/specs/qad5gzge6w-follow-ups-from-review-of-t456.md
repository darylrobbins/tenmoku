---
task_uid: "01m6qajs94y3p908qad5gzge6w"
task: "T464"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T456

Findings of the review of T456 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106a), for helium batch 105:

- F1 medium crates/fe-vim/src/insert/keys.rs:951: Recording both plain multiline forms as LiteralInsert::Plain routes subsequent replay through unconditional insertion, even when the register now contains single-line text that should overwrite. On "ab\ncd\nxyz tail", `vjyj0R<C-r>0<Esc>G0"0yiw.` inserts another `xyz`, producing `xyzxyz tail` on the last row; Neovim leaves `xyz tail`. `<C-r><C-r>` has the same defect. The new tests keep register contents unchanged and miss this transition.
  fix: Replay plain register operations using the current register's replace-mode semantics, including restoration-stack tracking, rather than always inserting stackless text. Cover register changes between dots for both plain forms and subsequent counted replay.
  instances: crates/fe-vim/src/insert/keys.rs:951, crates/fe-vim/src/insert/keys.rs:635, crates/fe-vim/src/insert/keys.rs:1550
