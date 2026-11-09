---
task_uid: "01m6w2wezexkcnw896teffcnax"
task: "T320"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_dismissed_list_picker_does_not_open_its_row passes and its toggle-off case asserts focus equals the focus captured before the picker was shown"
    required: true
    manual: false
---

# Harpoon: assert toggling the list picker off keeps the pre-picker focus (T1422 F1) (Mac T1488)

Follow-up to T1422 (review round 2, F1 medium, carried by the lead under the round-2 rule).

`a_dismissed_list_picker_does_not_open_its_row` in crates/fe-workbench/tests/harpoon_supersede.rs (~169, ~180) checks the Escape case against the focus captured before the Harpoon list picker was shown, but the toggle-off case (toggling the list off after Enter) derives its expected focus from a post-dismissal control value, so it would accept a focus move.

Change the toggle-off case to capture focus before showing the picker and assert that, after toggle-off and parking, focus equals that pre-picker focus (the same check as Escape). If the assertion fails on current main, the bug is in crates/fe-workbench/src/harpoon.rs's dismissal path: fix it there so toggle-off restores the pre-picker focus.

Tests: edit only the existing test file; keep the existing Escape assertions unchanged.
