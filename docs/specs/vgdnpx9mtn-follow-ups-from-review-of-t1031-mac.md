---
task_uid: "01m6z299t81t8pmbvgdnpx9mtn"
task: "T538"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "sticky_scroll_settings_tests: in the million-row fixture with a closed fold, enclosing scopes found in one visible run survive into the next; after fractional scrolling both headers pin at their asserted vertical positions, and discovery work stays bounded (review F2)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Follow-ups from review of T1031 (Mac T1493)

Findings of the review of T1031 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-gpui/src/element/sticky_scroll.rs:193: Independent indentation queries truncate enclosing scopes at a closed fold. In the new million-row fixture, the first run returns outer as 0..=2, while the second cannot rediscover it within INDENT_SCAN_ROWS. Row 2 maps to the folded header, so scrolling to line_height + 1 pushes outer to y = -1 and prevents b from pinning. The previous contiguous query correctly pinned outer at y = 0 and b one line below. The new assertion expecting only [0] codifies this regression.
  fix: Preserve enclosing indentation context across visible runs using fold-aware discovery or indexed summaries while keeping discovery work bounded. Update the regression test to assert both pinned headers and their vertical positions after fractional scrolling.
  instances: crates/fe-gpui/src/element/sticky_scroll.rs:193, crates/fe-gpui/src/element/sticky_scroll_settings_tests.rs:329
