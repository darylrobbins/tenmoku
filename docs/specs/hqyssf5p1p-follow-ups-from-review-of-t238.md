---
task_uid: "01m4f80dek5xzr63hqyssf5p1p"
task: "T243"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T238

Findings of the review of T238 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-config/src/theme.rs:310: Replacing bundled captures in place loses the required file order. For example, a user file listing keyword, new_capture, comment produces those entries in the order comment, keyword, new_capture. Sorting the parsed entries does not help because replacements retain their bundled positions; the tests only check ordering of new captures.
  fix: Keep fallback entries for captures absent from the user file, then append all valid user entries in file order, including replacements. Add a regression test interleaving reordered bundled captures and new captures.
  instances: crates/fe-config/src/theme.rs:310
