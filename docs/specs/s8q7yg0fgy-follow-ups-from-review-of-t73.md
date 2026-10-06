---
task_uid: "01m47qvmqpm38s3gs8q7yg0fgy"
task: "T95"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T73

Findings of the review of T73 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/keymap.rs:93: Pending input has no window or focus owner. Pressing cmd-k in editor A and cmd-0 in editor B completes the shared matcher and folds B. Likewise, changing focus before a timeout dispatches the prefix command or replays swallowed text into the newly focused editor. Closing the originating window also leaves matcher state pending because the timer's failed window update is ignored.
  fix: Associate pending input with its originating window and focus generation. Invalidate it when that owner changes or disappears, and verify ownership before continuation, timeout dispatch, and replay. Add simulated-clock tests covering two windows, focus changes, and owner closure.
  instances: crates/fe-gpui/src/keymap.rs:93, crates/fe-gpui/src/keymap.rs:118, crates/fe-gpui/src/keymap.rs:132, crates/fe-gpui/src/keymap.rs:143, crates/fe-gpui/src/keymap.rs:161

- F2 medium crates/fe-gpui/src/keymap.rs:81: An unsupported key returns before cancelling the timer or ending pending input. For example, cmd-k, f21, cmd-0 still folds all, and an exact cmd-k prefix still dispatches on timeout after f21. This violates the requirement that a new keystroke cancels the timer and lets chords span intervening unmatched keys.
  fix: Handle unsupported non-modifier keys as failed chord continuations: cancel the timeout, clear pending matching without dispatching its exact prefix, replay swallowed character input, and propagate the current key. Add tests for an unsupported key between chord strokes and before timeout.
  instances: crates/fe-gpui/src/keymap.rs:81, docs/specs/sqss9kgtqc-drive-key-input-through-fe-keymap-a.md:157

## Implementation notes

- **Owner.** A pending chord records its owner: the window handle and a weak handle to the
  element focused when its first keystroke came in (`Owner` in `fe-gpui/src/keymap.rs`). A chord
  that is abandoned is dropped whole: nothing in it runs, and none of its swallowed keystrokes
  are re-delivered, since the element they were typed into no longer has focus.
- **Focus generation.** Comparing focus ids alone would let focus leave and come back (A, B, A)
  without ending the chord. The chord instead holds a `Window::on_focus_out` subscription on its
  element and is abandoned when GPUI reports focus leaving it, at the next frame. GPUI reports
  focus changes only in the active window, which a window receiving keystrokes is. The
  subscription is dropped with the chord, so it never fires for a later one. GPUI also reports
  focus out when the window stops being active, so switching away mid-chord abandons it. In addition, every
  keystroke, and the timeout, checks the owner before continuing, running or re-delivering: a
  keystroke from another window or element abandons the chord and is matched as a fresh one.
- **Window closure.** An `App::on_window_closed` observer abandons a chord whose window closed.
  The timer now runs on the app, not the window, and does nothing if its window is gone.
- **Unsupported keys.** A key `fe-keymap` can't name (such as `f21`) fails a pending chord: the
  timer is cancelled, the matcher's chord is ended without dispatching its exact prefix
  (`Matcher::flush`, outcome dropped), swallowed character input is re-delivered, and the key
  propagates. Standalone modifiers (GPUI's `shift`, `control`, `alt`, `platform`, `function`
  keystrokes, sent when a modifier is pressed and released alone) still leave the chord be. This
  supersedes the "Keys the adapter ignores" note in
  `docs/specs/sqss9kgtqc-drive-key-input-through-fe-keymap-a.md`, which is left as written.
- **Tests** (`fe-gpui/src/keymap_tests.rs`, simulated clock): `a_chord_belongs_to_its_window`
  (two windows, then closing the owner), `a_chord_belongs_to_its_focused_element` (two editors in
  one window, including focus leaving and returning), `unsupported_keys_fail_a_chord`. A
  test-only `keymap::chord_pending` asserts that closure and focus-out clear the chord directly,
  not only through the next keystroke's owner check.
