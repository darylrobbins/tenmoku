---
task_uid: "01m7y01m4d15349z5c7v1ty9q3"
task: "T731"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-workbench/tests/harpoon_supersede.rs compares the focus after a toggle-off dismissal against the focus from before the picker opened (not against the control dismissal), so the test fails if either dismissal loses focus; fix harpoon.rs only if the stricter test shows a real focus loss."
    required: true
    manual: false
---

# Follow-ups from review of T1422 (Mac T2078)

Findings of the review of T1422 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/tests/harpoon_supersede.rs:169: Toggle-off still uses unchecked control-dismissal focus as its expected value, so both dismissals can lose focus and the assertion passes. Revised C2 defers this coverage to T1488, but the review instructions explicitly keep unresolved previous findings blocking.
  fix: Compare toggle-off focus against the original focus as previously requested, or explicitly withdraw F1 from this review's blocking findings to reflect the T1488 deferral.
  instances: crates/fe-workbench/tests/harpoon_supersede.rs:169, crates/fe-workbench/tests/harpoon_supersede.rs:180
