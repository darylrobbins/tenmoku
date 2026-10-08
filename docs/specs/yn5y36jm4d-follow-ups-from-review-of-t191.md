---
task_uid: "01m4e1ca6gdvwv46yn5y36jm4d"
task: "T195"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T191

Findings of the review of T191 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/references_panel.rs:355: The run-wide destination list still mistakes coincident offsets for actual panel landings. For example, open reference A, open B, manually move back to A's end, then open C: A remains in `opened`, so its manually chosen position is omitted from Back history. A superseded pending read is also added before it lands and remains eligible forever, even when its reveal was cancelled. Entries carry no destination-pane identity. The new regression only covers destinations never requested, leaving the spec's requirement to distinguish actual landings incomplete.
  fix: Track pending destinations and current landings with their destination panes and lifecycle, invalidating superseded requests and stale landings when the user moves the caret. Preserve existing pending-read, pane-switch, clamping, and Back/Forward behavior. Add regressions for manually revisiting an earlier opened end and moving to the end of a superseded destination that never landed.
  instances: crates/fe-workbench/src/references_panel.rs:78, crates/fe-workbench/src/references_panel.rs:330, crates/fe-workbench/src/references_panel.rs:355, crates/fe-workbench/src/references_panel.rs:358, crates/fe-workbench/src/references_panel.rs:359
