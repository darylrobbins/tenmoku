---
task_uid: "01m6ktjw7jq1ebngxxc16baaqj"
task: "T446"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T254

Findings of the review of T254 that don't block the merge (below merge.block_severity):

- F5 medium crates/fe-vim/src/feature/exchange.rs:165: Tracking linewise regions only by their boundary rows loses surviving marked text. On "a\nb\nc\nd", `3cxxddGcxx` swaps only c, producing "b\nd\nc" instead of "d\nb\nc"; deleting the last boundary has the symmetric defect, and deleting both boundaries clears the mark despite surviving interior rows. Anchoring both places at row starts also loses split-off text: on "ab\nc", `cxxli<CR><Esc>Gcxx` produces "c\nb\na" instead of "c\na\nb".
  fix: Track the surviving region extent through boundary deletions and row splits, while retaining the O/o distinction. Add regressions for deleting either or both boundaries of a region with interior rows and inserting a newline inside its last row.
  instances: crates/fe-vim/src/feature/exchange.rs:165, crates/fe-vim/src/feature/exchange.rs:166, crates/fe-vim/src/feature/exchange.rs:188
