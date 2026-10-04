---
task_uid: "01m4228yhzp262759vtfg0sqv4"
task: "T45"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T20

Findings of the review of T20 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/queries/kotlin.scm:14: Appending the broad SCREAMING_CASE captures after the crate query overrides its specialized function captures. Kotlin's built-in TODO() becomes constant instead of function.builtin, and uppercase direct or qualified function calls become constants. The samples do not exercise these collisions.
  fix: Restrict the constant patterns or restore specialized captures after them, preserving function and built-in function highlighting. Add regression cases for TODO(), uppercase direct and qualified calls, and actual constants.
  instances: crates/fe-syntax/queries/kotlin.scm:14, crates/fe-syntax/queries/kotlin.scm:17

- F2 medium crates/fe-syntax/queries/sql.scm:11: The numeric overrides recognize only plain integers and decimals, although the selected grammar accepts exponents, radix literals and digit separators. Valid literals such as 1e3, 1.5e-2, 0xFF and 1_000 therefore retain the crate's fallback string highlight. The golden sample covers only the supported subset.
  fix: Match the numeric forms accepted by the selected grammar and add regression coverage for exponent, radix and separated literals while ensuring quoted strings remain strings.
  instances: crates/fe-syntax/queries/sql.scm:11, crates/fe-syntax/queries/sql.scm:14
