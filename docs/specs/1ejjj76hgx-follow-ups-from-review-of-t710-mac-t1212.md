---
task_uid: "01m69aev0x5dka2r1ejjj76hgx"
task: "T386"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Follow-ups from review of T710 (Mac T1212)

Findings of the review of T710 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-nav/tests/references_encoding.rs:150: The watchdog's completion flag is set only on normal return. If request creation or the search panics during the baseline call with limit=None, the watchdog loops forever and thread::scope waits forever while unwinding. A test failure therefore hangs the test binary instead of reporting the failure.
  fix: Install a completion guard inside the scope that sets done on drop, including during unwinding, before executing request creation and the search.
  instances: crates/fe-nav/tests/references_encoding.rs:150
