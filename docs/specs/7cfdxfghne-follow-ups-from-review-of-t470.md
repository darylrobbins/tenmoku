---
task_uid: "01m6qvqqf9y5qh2b7cfdxfghne"
task: "T472"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T470

# Follow-ups from review of T470

A finding of the review of T470 that doesn't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106e), for helium batch 107. The Mac lead asked for this finding only; T468 F1/F2 and T471 F1 were skipped.

- F1 medium crates/fe-vim/src/feature/exchange.rs:212: The deletion check includes edits ending exactly at `at`, although such edits leave the tail's newline intact. After deleting the original last marked row, clearing the preceding row's text therefore drops that surviving empty row from the exchange. On "a\nb\nc\nd\ne", `3cxxjjddkDGcxx` produces "e\n\nd\na" instead of "e\nd\na\n": deleting `c` maps the tail from 3 to 2, then the new subtraction incorrectly moves it to 1.
  fix: Distinguish deletion of the tail boundary from deletion ending immediately before it; preserve the mapped boundary when its newline survives. Add a regression covering clearing the surviving tail row's text after deleting the original last row.

Keep T465's and T470's existing regressions in crates/fe-vim/tests/exchange.rs passing.
