---
task_uid: "01m6qk54eaxd86njz5ek5et76z"
task: "T469"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T463

Findings of the review of T463 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106c), for helium batch 106:

- F1 medium crates/fe-plugin/src/decorations.rs:286: Refresh rebases display anchors but retains the watch's original version; late sets also retain their source version. Once compaction passes that version, is_taken always returns false even while the display anchors remain valid. For example, set a highlight, insert unrelated text, refresh, compact to the refreshed version, then replace the entire highlight: the deleted record remains visible and consumes capacity. Decorations and gutter entries both suffer this regression.
  fix: Preserve deletion tracking across rebasing and compaction, or discard records whose lifetime can no longer be determined. Add held and late set regressions covering refresh, compaction, subsequent replacement, and capacity accounting.
  instances: crates/fe-plugin/src/decorations.rs:178, crates/fe-plugin/src/decorations.rs:286, crates/fe-plugin/src/decorations.rs:689

- F2 medium crates/fe-plugin/src/decorations.rs:171: An empty decoration created at a boundary still has no watched bytes after moving into the text. It disappears only if one edit strictly surrounds its point, so deleting its surroundings in separate edits retains it forever. In "abcdef", set 0..0, insert "X" at zero, delete 1..7, then delete 0..1: the buffer is empty but the decoration survives. Held and late sets both retain this record through refresh and capacity accounting.
  fix: Track surrounding-text deletion after a boundary decoration enters the text, including deletion across multiple edits, rather than relying solely on a single strict point collapse. Add held and late regressions for this sequence.
  instances: crates/fe-plugin/src/decorations.rs:171, crates/fe-plugin/src/decorations.rs:183, crates/fe-buffer/src/log.rs:281
