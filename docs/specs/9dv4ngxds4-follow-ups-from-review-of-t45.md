---
task_uid: "01m43zfpyczsge879dv4ngxds4"
task: "T54"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T45

Findings of the review of T45 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/queries/sql.scm:12: Both numeric predicates still reject valid signed literals with whitespace or comments between the sign and digits, such as `- 1e3`, `+ 1.5e-2`, and `- /* comment */ 0xFF`. The selected grammar defines the optional sign separately from the numeric token and permits extras between them, so these complete literal nodes retain the fallback string highlight. The new tests exercise only unsigned literals.
  fix: Handle grammar-permitted extras between signs and numeric tokens in both predicates while preserving quoted-string highlighting and nested comment captures. Add regressions for signed integers, radix literals, and decimals with whitespace and comments.
  instances: crates/fe-syntax/queries/sql.scm:12, crates/fe-syntax/queries/sql.scm:15
