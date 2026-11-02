---
task_uid: "01m6dmdfqem455vr38pkr49xgv"
task: "T409"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T224

Findings of the review of T224 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-markdown-view/src/view.rs:299: Overlapping parent regions prevent gap interpolation and make the two mappings inconsistent. A table separator line belongs only to the whole-table region, so scrolling to it interpolates through the entire table, while reading back selects a header or body row. With equal-height rows, advancing from separator line 1.9 to body line 2 can actually scroll backward. Blank lines between nested quote/list blocks have the same problem because their parent wins before gap interpolation runs.
  fix: Build a consistent piecewise mapping that gives nested regions precedence and interpolates uncovered source or pixel gaps between neighboring nested regions. Test table separators and gaps inside quotes and loose list items for round-trip accuracy and monotonicity.
  instances: crates/fe-markdown-view/src/view.rs:262, crates/fe-markdown-view/src/view.rs:299

- F3 medium crates/fe-markdown-view/src/view.rs:226: The deferred wheel callback reads an unsettled offset: GPUI adds wheel deltas immediately but clamps them during the next prepaint. A large downward wheel therefore syncs the editor to a line beyond the preview's eventual visible top, potentially EOF, and no correction follows the clamp. The baseline also remains stale after scroll_to_heading, which records the old offset before GPUI applies the heading scroll; a subsequent zero-delta wheel can incorrectly report that programmatic movement as a user scroll.
  fix: Compare settled offsets before and after user scrolling, accounting for GPUI's clamp, and update the baseline after deferred programmatic scrolling settles. Add tests for overscroll at both boundaries and a zero-delta wheel after heading navigation.
  instances: crates/fe-markdown-view/src/view.rs:174, crates/fe-markdown-view/src/view.rs:226, crates/fe-markdown-view/src/view.rs:519
