---
task_uid: "01m49dyfz3yfqf61xq8tkb4n7n"
task: "T134"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T122

Findings of the review of T122 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/outline.rs:676: F4's function exclusion still misses TypeScript/TSX instantiation_expression wrappers. Valid code such as `const f = (function <T>(x: T) { return x; })<string>;` produces a Variable because bound_to_function stops at that wrapper and returns false. Generic arrow and generator expressions have the same problem, including exported declarations. The regression test covers other type-only wrappers but misses this one.
  fix: Traverse instantiation_expression through its expression child before classifying the initializer, and add exported and non-exported TS/TSX regressions for generic function, arrow and generator expressions while preserving outline Function items.
  instances: crates/fe-syntax/src/outline.rs:676, crates/fe-syntax/queries/definitions/javascript.scm:29, crates/fe-syntax/queries/definitions/javascript.scm:39


Scope notes (triage, main 1e99299): still true; `bound_to_function` in outline.rs unwraps parenthesized/as/satisfies/non_null/type_assertion but not `instantiation_expression`. The fix is in outline.rs only (the definitions queries reach it through `#non-function?`). Put the TS and TSX regressions in the new integration test `crates/fe-syntax/tests/ts_instantiation.rs` (cfg-gated on the typescript feature like `symbols_followups.rs`); leave existing tests and goldens alone.
