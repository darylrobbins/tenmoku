---
task_uid: "01m41xnbjwa2v51gkfk713r4j9"
task: "T40"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T15

Findings of the review of T15 that don't block the merge (below merge.block_severity):

- F1 medium vendor/tree-sitter-yaml/src/scanner.c:171: The widened fields are serialized through int32_t pointers into a byte buffer. blk_imp_row occupies byte offset 6, so its store and corresponding load are misaligned even when the buffer starts properly aligned. This introduces undefined behavior on ordinary YAML parses and can fault on alignment-sensitive targets or abort under alignment sanitizers; passing parsing tests does not establish safety.
  fix: Use memcpy for both widened fields in serialize and deserialize, preserving their byte layout without requiring buffer alignment. Document the additional safety change and verify serialization round trips with alignment sanitization.
  instances: vendor/tree-sitter-yaml/src/scanner.c:167, vendor/tree-sitter-yaml/src/scanner.c:171, vendor/tree-sitter-yaml/src/scanner.c:201, vendor/tree-sitter-yaml/src/scanner.c:205

## Implementation notes

- F1: `serialize`/`deserialize` in `vendor/tree-sitter-yaml/src/scanner.c` now
  `memcpy` `row` and `blk_imp_row`; the serialized layout is unchanged. The
  vendor README records it as a change on top of PR #35.
- Verified by hand, not in CI: a C harness built with clang
  `-fsanitize=alignment,undefined -fno-sanitize-recover=all` against the
  tree-sitter 0.27.0 runtime. It round-trips a hand-built state with rows past
  the int16 range byte for byte, then parses a 40k-entry nested YAML document
  and reparses it after an edit, both without errors. The pre-fix scanner
  aborts in the same harness with "load of misaligned address ... for type
  'int32_t'" at the `blk_imp_row` load.
