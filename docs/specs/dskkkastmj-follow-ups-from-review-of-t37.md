---
task_uid: "01m4nvzrh6pg0dkxdskkkastmj"
task: "T38"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T37

Findings of the review of T37 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-text/src/encoding.rs:603: The EOF decoder fix and full-traversal regressions address the panic, but the spec also requests search matches ending at EOF. Both new assertions exercise only for_each_unit; they do not verify decode_legacy_line's pending EOF offsets, converted match ranges, or preview.
  fix: Add search conversion regressions for ISO-2022-JP lines ending in ESC '$' and ESC '(', with match ranges ending at EOF. Assert successful completion, decoded ranges, and preview text.
  instances: crates/fe-text/src/encoding.rs:603
