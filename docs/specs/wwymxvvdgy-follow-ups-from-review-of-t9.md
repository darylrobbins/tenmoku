---
task_uid: "01m4pa4txw1t3vnxwwymxvvdgy"
task: "T13"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T9

Findings of the review of T9 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:787: Falling back to the plain scroll position reintroduces the wide-inlay caret bug. With the test's geometry, head 60, insertion offset 40, and a 100-character inlay, plain scrolling produces 327.5 and includes the inlay. Shifted scrolling produces 1327.5 and excludes it, so this branch chooses 327.5 again; the laid-out caret is then at x=1600 and remains offscreen. The regression's visibility assertion misses this because its selections always put the actual cursor at offset 0.
  fix: Resolve slice inclusion and autoscroll together so the final laid-out primary caret is visible even when the inlay changes inclusion between proposed positions. Add a wide-inlay regression and make the helper's selection head match its primary_head argument.
  instances: crates/fe-display/src/layout.rs:787, crates/fe-display/src/layout/inlay_tests.rs:122, crates/fe-display/src/layout/inlay_tests.rs:197

- F2 medium crates/fe-display/src/layout.rs:809: Scrolling uses predicted inlay geometry rather than the final laid-out insertion. Inclusion is checked before scroll clamping and slice widening, and width comes from shaping the inlay alone rather than its measured span in the row. For example, on the 9000-character monospace row, an excessive incoming scroll and an inlay at offset 8990 initially exclude the inlay; clamping then produces a slice that includes it, but the maximum scroll still omits its width. Contextual shaping can also make the standalone width differ from RowLayout.inlay_width, leaving autoscroll and mappings inconsistent.
  fix: Calculate autoscroll and maximum scroll from the final shaped slice's measured inlay displacement, reconciling geometry after clamping or widening changes inclusion. Add regressions for an excessive incoming scroll with an insertion near the row end and for contextual inlay measurement.
  instances: crates/fe-display/src/layout.rs:724, crates/fe-display/src/layout.rs:739, crates/fe-display/src/layout.rs:761, crates/fe-display/src/layout.rs:809
