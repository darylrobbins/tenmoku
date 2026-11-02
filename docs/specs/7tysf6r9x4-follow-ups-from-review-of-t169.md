---
task_uid: "01m6byhx9m4x2f497tysf6r9x4"
task: "T392"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T169

Findings of the review of T169 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-vim/src/insert.rs:2263: Automatic indentation still enters the last-insert register after dot repeat. Starting with `  abc\nz`, run `A<CR>x<Esc>G.`: replay types LastChange::inserted (`\n  x`) as ordinary typing, so its indentation is absent from the indent ledger and this capture publishes `\n  x`. Neovim retains `\nx`. The added multiline tests cover initial insertion but miss this repeat path.
  fix: Preserve the separate register text through repeat, or preserve indentation provenance when replaying, and add a multiline dot-repeat regression asserting the register text and subsequent put.
  instances: crates/fe-vim/src/insert.rs:2263
