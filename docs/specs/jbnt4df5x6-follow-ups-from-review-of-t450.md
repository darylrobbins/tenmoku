---
task_uid: "01m6pw52hsjfzdatjbnt4df5x6"
task: "T461"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T450

Findings of the review of T450 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F1 medium crates/fe-syntax/src/injection.rs:437: Scanner spans are never clipped to self.range. Highlighting a small portion of a format string returns placeholders and escapes elsewhere in that string, including complete directives that cross the requested boundaries. The previous highlight_query_finished path clipped its output, and neither overlay nor checked_highlights clips these new spans afterward.
  fix: Clip scanner output to the requested highlight range before overlaying it. Add partial-range tests covering off-range directives and ranges starting or ending inside a directive.
  instances: crates/fe-syntax/src/injection.rs:437

- F2 medium crates/fe-syntax/src/injection.rs:466: The new path copies and scans every byte of an overlapping format-string content range without checking the highlight deadline, then unconditionally reports finished=true. A large raw format string therefore causes unbounded work and allocation on each uncached viewport request, even when its parsed injection layer is already cached. This bypasses the time budget enforced by the replaced query path.
  fix: Make collection and scanning interruptible in bounded chunks, propagate incomplete status when the deadline expires, and avoid collecting off-screen spans. Add coverage for deadline exhaustion on a cached large format-string layer.
  instances: crates/fe-syntax/src/injection.rs:437, crates/fe-syntax/src/injection.rs:466, crates/fe-syntax/src/verbs.rs:20
