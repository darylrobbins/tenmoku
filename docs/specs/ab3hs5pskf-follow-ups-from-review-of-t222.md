---
task_uid: "01m4m87msmq32554ab3hs5pskf"
task: "T339"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T222

Findings of the review of T222 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/register.rs:153: Kind matching compares normalized clipboard text with the unnormalized cached register. A charwise Register::new("ab\r\n", false) written through Registers::write loses its kind on an unchanged clipboard read, becoming linewise. Conversely, after writing charwise "ab\n", externally replacing the clipboard with "ab\r\n" incorrectly retains the cached kind despite different text. The new tests cover neither case.
  fix: Compare the raw clipboard text with the cached text before newline normalization; normalize the fallback text afterward. Add regression tests for unchanged CRLF-containing writes and externally replaced text that only matches after normalization.
  instances: crates/fe-vim/src/register.rs:153
