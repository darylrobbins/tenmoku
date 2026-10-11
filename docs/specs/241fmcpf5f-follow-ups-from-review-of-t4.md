---
task_uid: "01m4pav3m75yte0e241fmcpf5f"
task: "T15"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T4

Findings of the review of T4 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/src/mouse.rs:265: Mouse reports use the keyboard-input write path, which clears selection and scrolls to the bottom. With any-motion reporting enabled, a Shift-created selection disappears on the first ordinary pointer movement after releasing Shift; Shift-scrolled history also snaps back to the bottom.
  fix: Send mouse reports through a PTY write path that preserves selection and display_offset. Keep selection clearing and scroll-to-bottom behavior for keyboard input, and test selection and scrollback preservation under any-motion reporting.
  instances: crates/fe-terminal-view/src/mouse.rs:265, crates/fe-terminal-view/src/mouse.rs:411

- F2 medium crates/fe-terminal-view/src/mouse.rs:229: Active gesture moves and releases are handled only during bubbling. Another control can stop propagation before these listeners run, leaving a selecting drag active after release and continuing autoscroll, or leaving a reporting program without its button release.
  fix: Handle active-gesture movement and release during capture, with appropriate event ownership. Add a window test that releases an out-of-bounds drag over a sibling that stops propagation and verifies that scrolling stops and the reporting release arrives.
  instances: crates/fe-terminal-view/src/mouse.rs:220, crates/fe-terminal-view/src/mouse.rs:229

- F3 medium crates/fe-terminal-view/src/mouse.rs:285: The single drag slot silently discards outstanding reported presses. For example, left-down, right-down, left-up, right-up emits both presses but only the right release. Starting a selection can likewise overwrite a reported button, and the missed-release recovery clears Report state without releasing it to the program.
  fix: Track outstanding reported buttons separately from the selection gesture, preserve their matching releases, and explicitly reconcile a reported button when movement proves its release was missed. Test overlapping buttons and missed-release recovery.
  instances: crates/fe-terminal-view/src/mouse.rs:285, crates/fe-terminal-view/src/mouse.rs:312, crates/fe-terminal-view/src/mouse.rs:327

- F4 medium crates/fe-terminal-view/src/mouse.rs:339: Shift bypass is checked for fresh presses and idle motion, but not for an existing Report gesture. Pressing normally and then holding Shift while dragging still sends application drag events; its release is also reported without considering the bypass state.
  fix: Apply Shift bypass consistently to active reporting gestures, with an explicit cancellation and release policy for previously reported presses. Test modifier changes between mouse down, movement and release.
  instances: crates/fe-terminal-view/src/mouse.rs:339, crates/fe-terminal-view/src/mouse.rs:360

- F5 medium crates/fe-terminal-view/src/mouse.rs:412: On an alternate screen with reporting and alternate_scroll enabled, holding Shift makes report false and falls through to sending arrow keys. The spec permits this fallback only when mouse reporting is off; the bypass currently still changes the application's state and clears any selection.
  fix: Require mouse reporting to be disabled before using the alternate-scroll arrow fallback. Test Shift-wheel with alternate screen, alternate_scroll and reporting all enabled, asserting that no PTY bytes are written.
  instances: crates/fe-terminal-view/src/mouse.rs:412

- F6 low crates/fe-terminal-view/src/mouse.rs:388: The wheel remainder survives changes between reporting and local scrolling, although their consumption units differ. After accumulating 2.5 lines toward a report, a Shift-held half-line event scrolls three local lines, transferring motion intended for the program into scrollback.
  fix: Reset or separate wheel accumulators when the effective wheel destination changes, including Shift bypass and terminal mode changes. Add a fractional-delta test across those transitions.
  instances: crates/fe-terminal-view/src/mouse.rs:388
