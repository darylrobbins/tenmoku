---
task_uid: "01m5wvj9k0gsxfxg34sxtnehjk"
task: "T365"
rev: 2
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T57

Findings of the review of T57 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/external_drop.rs:103: The new open_serial check detects another open, but tab activation and cycling only increment Pane::navigations. If the awaited file lands and the user selects another tab before the batch resumes, membership remains true and the serial remains unchanged, so the batch overrides that navigation with its next file. It can also send Escape to the user's selected editor. The requested controlled navigation and overlapping-drop tests remain absent.
  fix: Track navigation changes independently of read completion and competing opens; cancel the batch before normalization or further opens when navigation supersedes it. Add controlled delayed-read tests for navigation around completion and overlapping drops sharing a path.
  instances: crates/fe-workbench/src/external_drop.rs:103, crates/fe-workbench/src/external_drop.rs:107, crates/fe-workbench/src/external_drop.rs:110, crates/fe-workbench/src/external_drop.rs:117, crates/fe-workbench/src/external_drop.rs:124

- F2 medium crates/fe-workbench/src/external_drop.rs:98: Pane::open still spawns an independent read whose landing can activate and focus an editor after the destination pane is removed. The batch retains the pane, and its asynchronous membership check cannot prevent a ready read from landing before the removal notification is processed. No controlled pending-read pane-closure test was added.
  fix: Synchronously invalidate pending drop reads when the pane leaves the workbench, or guard their landing and focus with pane membership lifecycle state. Add a controlled test proving completion after removal cannot steal focus.
  instances: crates/fe-workbench/src/external_drop.rs:84, crates/fe-workbench/src/external_drop.rs:98, crates/fe-workbench/src/external_drop.rs:113
