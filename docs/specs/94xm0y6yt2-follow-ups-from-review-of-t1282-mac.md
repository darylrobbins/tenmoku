---
task_uid: "01m7q2j073ad7gtw94xm0y6yt2"
task: "T423"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T1282 (Mac T1882)

Findings of the review of T1282 that don't block the merge (below merge.block_severity):

- F1 medium vendor/tree-sitter-gitignore/bindings/rust/lib.rs:23: The new can_load_grammar test references tree_sitter, but the vendored crate declares no tree-sitter dependency or dev-dependency. Running its own cargo test therefore fails to compile. The fe-syntax gate does not catch this because dependency unit tests are not run.
  fix: Add tree-sitter 0.27 as a dev-dependency in the vendored manifest and verify its can_load_grammar test passes.
  instances: vendor/tree-sitter-gitignore/bindings/rust/lib.rs:23
