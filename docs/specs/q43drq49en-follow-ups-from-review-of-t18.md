---
task_uid: "01m420bhfqb9fs1rq43drq49en"
task: "T43"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T18

Findings of the review of T18 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-syntax/src/tests/query_overrides.rs:78: The accepted depth limit is documented, but its quoted-key boundary remains untested. The new boundary test generates only bare keys, so it never exercises the deepest quoted_key patterns for pairs, tables, or table arrays, nor their documented string fallback.
  fix: Extend the boundary test with basic and literal quoted components at ten dotted_key ancestors in pairs and both header forms; also verify their documented fallback beyond that limit. Arbitrary-depth support is no longer required.
  instances: crates/fe-syntax/src/tests/query_overrides.rs:82, crates/fe-syntax/src/tests/query_overrides.rs:83, crates/fe-syntax/src/tests/query_overrides.rs:84, crates/fe-syntax/src/tests/query_overrides.rs:94
