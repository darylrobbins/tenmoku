---
task_uid: "01m6pb6697gcqz0a6f673y6gpb"
task: "T454"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T279

Findings of the review of T279 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-plugin/src/net.rs:139: Plugin-controlled headers are inserted with HeaderMap::append, which panics when the map exceeds its capacity. Thousands of distinct valid headers fit within the plugin's memory limit, so an otherwise valid fetch can unwind host code instead of returning an import error. Near-capacity maps can also panic when fe-net inserts Host and Connection.
  fix: Bound accepted header counts with room for fe-net's generated headers, use fallible insertion, and return InvalidArgument on overflow. Add a regression test with excessive valid headers.
  instances: crates/fe-plugin/src/net.rs:139
