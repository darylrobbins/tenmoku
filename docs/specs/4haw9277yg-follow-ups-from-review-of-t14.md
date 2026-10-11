---
task_uid: "01m4pcrht71658364haw9277yg"
task: "T19"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T14

Findings of the review of T14 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/element.rs:639: The fallback retains and shifts the entire original frame, including other long rows whose slices were shaped for the original scroll position. With two visible unwrapped long rows and the wide composition from the new regression on the first, the shift moves the second row's shaped slice completely off-screen, leaving it blank despite text existing at the displayed columns. Keeping the original scroll repeats this on subsequent frames.
  fix: Preserve the composition row's geometry while laying out other long rows for the final displayed scroll position, or make slicing account for the composition without retaining stale slices for other rows. Add a regression with another visible long row and verify its text covers the shifted viewport across frames.
  instances: crates/fe-gpui/src/element.rs:639

- F2 medium crates/fe-gpui/src/view/ime_tests.rs:513: The requested no-op movement regression never executes a movement command: it calls only end_composition_for. Moreover, move_right at offset 100 in a 2000-character row would move the caret. This does not verify cancellation and caret visibility through actual no-op command dispatch.
  fix: Compose at a boundary where movement does nothing, dispatch the corresponding movement through RunCommand, and assert that the buffer caret remains unchanged, the composition ends, and the caret returns to view after drawing.
  instances: crates/fe-gpui/src/view/ime_tests.rs:513
