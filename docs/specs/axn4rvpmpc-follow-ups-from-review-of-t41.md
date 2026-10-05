---
task_uid: "01m452sxm2zw6a5yaxn4rvpmpc"
task: "T59"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T41

Findings of the review of T41 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-display/src/long_row.rs:424: The new proportional-ASCII walker returns when the base character's width crosses the target, before checking whether it starts a narrower grapheme cluster. For a 9,000-byte row of repeated e+U+0301, with ASCII widths 10 except i=3 and cluster width 2, offset_for_x(2) returns 0 instead of 3, and slice(0..5, 0) ends at byte 3 (x=2), leaving its requested range uncovered. The previous segmented path handles this correctly.
  fix: Before returning for an X target, establish that the ASCII character is a complete cluster; otherwise resume segmentation from its start. Add proportional-width regression coverage where a combining cluster is narrower than its base, checking boundary round trips and slice coverage while retaining the scan-count assertions.
  instances: crates/fe-display/src/long_row.rs:424

## Implementation notes

- F3: when a proportional printable-ASCII char crosses an x target, `walk_ascii` now checks whether the char after it extends its cluster (as a combining mark does). If so it returns `Err` at the char's start, and `walk` segments from there, measuring the cluster whole. Monospace runs are unaffected: for an x target `stop` is the walk's bound, a cluster boundary, so the crossing char is always followed by printable ASCII or that boundary. Scan counts are unchanged; the one-char look-ahead isn't counted, as the existing look-ahead at `stop` isn't.
- `proportional_ascii_finds_narrower_clusters` covers the reported case (`offset_for_x(2.)` is 3, `slice(0..5, 0)` covers it) and checks round trips at every boundary and slice coverage across a row of e+U+0301 runs mixed with narrow and wide ASCII, with clusters 2 wide against bases of 3 and 10. It fails without the fix. The existing scan-count assertions are kept as they were.
