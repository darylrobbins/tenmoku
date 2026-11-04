---
task_uid: "01m6j7xkpwhr67fs52pfwyv2p1"
task: "T439"
rev: 4
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T238

Findings of the review of T238 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-vim/src/feature/comment.rs:179: Operator endpoints remain incorrect in uncovered cases. For `gc/foo<CR>` on `a\nb\nfoo\nc`, the raw motion endpoint sets `']` to zero-based (2,0), while Neovim sets (1,0); backward searches and gc} have the same exclusive-range problem. Empty objects return without setting Neovim's reversed endpoints. Also, select_rows defaults an absent goal to column zero: gcgc from (1,2) on `x\n# aaaa\n# bbbb\ny` sets `']` to (2,0), rather than Neovim's (2,2). Subsequent mark motions therefore target different text.
  fix: Apply exclusive-range adjustments when deriving endpoints, record reversed endpoints for empty objects, and derive an absent selection goal from the original caret. Add mark assertions for these cases.
  instances: crates/fe-vim/src/feature/comment.rs:179, crates/fe-vim/src/operator.rs:1009, crates/fe-vim/src/operator.rs:974, crates/fe-vim/src/visual.rs:535

## Implementation notes

- F4: `gc` over an empty object now sets `'[` at the object and `']` on the character before it; `select_rows` aims each end at the caret's own column when no goal is held. The search and `gc}` end marks already matched nvim; corpus cases for them are added.
- Carried T445 F4 (visual.rs typing bias reset) needs a reanchor call after the register insertion in insert/keys.rs, which would pull in its carried F2 (count replay); left for its own card.
