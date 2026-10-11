---
task_uid: "01m4pghkqd9c0z4b2h2bt0m71z"
task: "T24"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T22

Findings of the review of T22 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/src/mouse.rs:367: Missed-release recovery still conflates an unsupported held button with no held button. MouseButton::Navigate(_) maps to None, so movement identifying a navigation button while Left remains held clears the selection drag and releases every reported button prematurely. This leaves the requested preservation for pressed_button: Some incomplete.
  fix: Trigger missed-release recovery only when event.pressed_button.is_none(), independently of conversion to a reportable button. Add an overlapping navigation-button movement regression test.
  instances: crates/fe-terminal-view/src/mouse.rs:367

- F2 medium crates/fe-terminal/src/session.rs:344: The new input path fixes the offset race structurally, but the requested verification during continued output is missing. The existing reports_keep_selection_and_scrollback test only asserts display_offset >= offset after a reply; the old snapshot-and-restore implementation also satisfies that assertion while losing offset increments and moving the viewport.
  fix: Add a regression test that sends reports while PTY output adds history and verifies that visible history remains stationary, with display_offset increasing by the corresponding history growth. Retain verification that keyboard input returns to the bottom.
  instances: crates/fe-terminal/src/session.rs:344

- F3 low crates/fe-terminal-view/tests/mouse.rs:695: The autoscroll regression test waits 200 ms before recording the post-release offset. With only ten lines remaining and scrolling one line per frame, a stranded drag can reach offset zero during that wait; the subsequent equality then passes because scrolling has hit the boundary, even though release failed to stop it.
  fix: Record the offset immediately at release and assert it stays unchanged across subsequent frames, while asserting there is still room to scroll toward the bottom.
  instances: crates/fe-terminal-view/tests/mouse.rs:695
