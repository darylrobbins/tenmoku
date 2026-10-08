---
task_uid: "01m4c5xkx0x2s9bx1jzxmqgcrs"
task: "T181"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T168

Findings of the review of T168 that don't block the merge (below merge.block_severity):

- F7 medium crates/fe-syntax/src/imports.rs:279: Argument filtering remains quadratic when comments precede the arguments. For require(/* repeated N comments */ 'm', ...N string arguments...), each string capture walks all N comments before reaching two non-comment arguments. This performs O(N²) work and delays cancellation; the new test contains no comments.
  fix: Validate each arguments node once and cache its result, or avoid emitting a capture for every string argument. Add an adversarial regression with many leading comments and many string arguments.
  instances: crates/fe-syntax/queries/imports/javascript.scm:6, crates/fe-syntax/src/imports.rs:279

- F8 medium crates/fe-syntax/src/imports.rs:133: The recovery cap silently loses real imports after the seventeenth affected JSX element. A valid TSX file containing seventeen declarations with distinct names, each initialized with <A extends="/" />, followed by import a from './a', exhausts sixteen reparses and queries the still-misparsed remaining tree, omitting ./a.
  fix: Recover these elements without a fixed occurrence limit, preferably by correcting their parsing while preserving context and bounded work. Add a regression with more than sixteen occurrences and a final real import.
  instances: crates/fe-syntax/src/imports.rs:133, crates/fe-syntax/src/imports.rs:160
