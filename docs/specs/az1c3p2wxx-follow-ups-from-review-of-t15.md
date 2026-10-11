---
task_uid: "01m4pey3asgj97tyaz1c3p2wxx"
task: "T22"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T15

Findings of the review of T15 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/src/mouse.rs:376: A move event identifies one pressed button, not every held button. After left-down and right-down, movement identifying either button prematurely releases the other and removes its outstanding press; its actual mouse-up is then ignored. The overlap test contains no movement and misses this regression.
  fix: Preserve other outstanding buttons when pressed_button is Some; reconcile missed releases only when the available event information proves them. Add overlapping-button tests with movement before and between releases.
  instances: crates/fe-terminal-view/src/mouse.rs:376

- F2 medium crates/fe-terminal-view/src/mouse.rs:284: The report path still calls Terminal::input, which scrolls to the bottom, and then reconstructs the offset. Snapshot, input and restoration acquire separate locks. Concurrent PTY output between these operations loses the offset increments that normally keep scrolled history stationary, causing the viewport to jump during mouse reporting.
  fix: Provide a PTY write method in fe-terminal that sends bytes without changing display_offset, and use it for mouse reports. Keep the existing keyboard-input behavior and verify preservation while output continues.
  instances: crates/fe-terminal-view/src/mouse.rs:284

- F3 medium crates/fe-terminal-view/src/mouse.rs:421: Shift cancellation is applied only after movement reconciliation. A normal press followed by a Shift-held release without intervening movement still sends an ordinary Shift-modified release; missed-release recovery with Shift also sends Shift-modified releases before reaching the cancellation branch. This differs from the explicit unshifted cancellation policy implemented for held-button movement and leaves the requested release edge case unresolved.
  fix: Apply one explicit Shift cancellation policy before all release and recovery paths, including mouse_up, and consistently normalize cancellation modifiers. Test Shift changes immediately before release and during missed-release recovery.
  instances: crates/fe-terminal-view/src/mouse.rs:373, crates/fe-terminal-view/src/mouse.rs:376, crates/fe-terminal-view/src/mouse.rs:421

- F4 low crates/fe-terminal-view/tests/mouse.rs:596: The sibling propagation test exercises only a reporting gesture. It never starts a selection drag, observes autoscroll, or verifies that scrolling stops after release, although that regression test is explicitly requested by the task.
  fix: Add a selection gesture with scrollback, move beyond the terminal over the swallowing sibling, verify autoscroll occurs, release there, and verify display_offset remains stable across subsequent frames.
  instances: crates/fe-terminal-view/tests/mouse.rs:596
