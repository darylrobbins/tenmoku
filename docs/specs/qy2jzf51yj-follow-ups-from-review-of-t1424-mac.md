---
task_uid: "01m7y01j374r34drqy2jzf51yj"
task: "T730"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The max_line_length ruler test lives in crates/fe-workbench/tests/editorconfig_max_line_length.rs (fe-gpui cannot build a Workbench, so the old fe-gpui file with set_max_line_length is removed): it sets the limit only through a real .editorconfig and EditorSettings, observes the painted rulers (coexistence with editor.rulers, order, settings reload, matching-column dedup, clearing), and its assertion fails if the max_line_length ruler is not drawn (shown by temporarily disabling the ruler, noted in the finish note)."
    required: true
    manual: false
---

# Follow-ups from review of T1424 (Mac T2077)

Findings of the review of T1424 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/element/max_line_length_ruler_tests.rs:46: The revised test observes painted rulers, but still violates the requirement to use only APIs present at base: set_max_line_length is introduced by this change. Transplanting this test to base fails compilation rather than demonstrating the missing behavior through a failing assertion.
  fix: Use base-compatible setup for the regression test while retaining the painted-ruler observation, and verify that the test compiles at base and fails its behavioral assertion.
  instances: crates/fe-gpui/src/element/max_line_length_ruler_tests.rs:46, crates/fe-gpui/src/element/max_line_length_ruler_tests.rs:55, crates/fe-gpui/src/element/max_line_length_ruler_tests.rs:59
