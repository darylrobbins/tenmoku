---
task_uid: "01m4crte8q4cd0ca7pprdfqg7s"
task: "T190"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T181

Findings of the review of T181 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/imports.rs:140: Removing MAX_REPARSES makes JSX recovery quadratic without bounding its total work. For N declarations like the new regression, each recovery constructs a fresh parser and parses the entire remaining suffix, totaling Θ(N²) input processing. Strictly increasing `from` proves termination, not a reasonable work bound. Large valid TSX files can now stall import lookup; the navigation caller polls cancellation but supplies no deadline here. The 40-element correctness test does not exercise this scaling problem.
  fix: Recover affected JSX elements while preserving context with bounded total work, such as correcting all affected spans before a single recovery parse. Add an adversarial scaling regression with substantially more occurrences and a final real import, plus cancellation coverage during recovery; do not restore a cap that silently loses imports.
  instances: crates/fe-syntax/src/imports.rs:140
