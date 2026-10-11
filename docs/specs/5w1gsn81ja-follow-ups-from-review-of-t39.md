---
task_uid: "01m4p1bskenxkgvf5w1gsn81ja"
task: "T47"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T39

Findings of the review of T39 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-lsp/src/tests/pull_diagnostics.rs:613: The new retirement-race test waits for any historical pull reply, and earlier steps have already recorded many. This predicate therefore succeeds before the raced request is answered. Releasing the blocked actor can let it process Close/Open before that reply arrives, allowing the original faulty implementation to pass.
  fix: Capture the raced request's ID and launch, wait specifically for its reply, and ensure reply delivery precedes actor processing of Close/Open. Verify that removing the active-identity checks makes this regression fail reliably.
  instances: crates/fe-lsp/src/tests/pull_diagnostics.rs:613

- F4 medium crates/fe-lsp/src/document.rs:640: The pushed-diagnostics path still accepts publications using only the actor's local incarnation. A publication processed after manager-side retirement but before the queued Close can emit diagnostics for a retired identity. The UI identity guard limits the impact, but the sink still receives a stale event; this pre-existing sibling of F1 remains.
  fix: Check the shared active identity when accepting pushed diagnostics and cover publication delivery during the manager-retirement/actor-close interval.
  instances: crates/fe-lsp/src/document.rs:640
