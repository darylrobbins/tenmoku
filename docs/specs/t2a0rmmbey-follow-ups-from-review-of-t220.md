---
task_uid: "01m4j2849vnphsxrt2a0rmmbey"
task: "T313"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T220

Findings of the review of T220 that don't block the merge (below merge.block_severity):

- F9 medium crates/fe-gpui/src/vim/search_tests.rs:205: The regression now compares ordered positions without omissions, but the boundary match remains offscreen: long-row shaping starts approximately 32 ems left of the viewport. Aligning the slice start to an "a" therefore places its following combining-mark match outside the text bounds, where visible_ranges excludes it. Skipping that precise match can still pass.
  fix: Construct a boundary case whose affected match reaches the text area, explicitly assert that match is included in the expected ranges, and compare the complete ordered rectangle geometry, including widths.
  instances: crates/fe-gpui/src/vim/search_tests.rs:201, crates/fe-gpui/src/vim/search_tests.rs:205, crates/fe-gpui/src/vim/search_tests.rs:211

- F10 medium crates/fe-gpui/src/element.rs:1239: Each frame still enumerates and allocates every match in each encountered buffer line using usize::MAX before applying the rectangle cap. A huge line can therefore stall rendering despite painting only 1,000 rectangles; CANDIDATES measures subsequent filtering and does not capture this enumeration work.
  fix: Bound or cache whole-line enumeration while preserving regex context and visible coverage, and measure enumeration work separately from rectangle candidate checks.
  instances: crates/fe-gpui/src/element.rs:1239

- F4 medium : Required criterion C3 remains unfulfilled: the supplied evidence explicitly states that no running-app check occurred, and the headless tests do not exercise N or establish actual GUI appearance.
  fix: Check this candidate in the running app with Vim enabled and record visible search highlighting, n and N navigation, and :noh clearing before merging.
  instances: (general)
