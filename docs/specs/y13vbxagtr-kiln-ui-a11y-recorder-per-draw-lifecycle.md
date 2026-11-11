---
task_uid: "01m749dw4gnr8kmjy13vbxagtr"
task: "T576"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "An announcement submitted after deactivation by a view rendering before the Announcer is discarded, not replayed when the window reactivates; a new test in kiln/crates/kiln-ui/src/tests/a11y_lifecycle.rs covers deactivation, that announcement, then reactivation (F3)"
    required: true
    manual: false
  - id: "C2"
    text: "Acceptance is checked against the receiving window's current accessibility state, never the Announcer's previous listening flag, including when the announcement is submitted while that window is mid-update; a test in a11y_lifecycle.rs covers it (F3)"
    required: true
    manual: false
  - id: "C3"
    text: "The existing tests in kiln/crates/kiln-ui/src/tests/a11y.rs and crates/fe-workbench/tests/a11y_announcer.rs still pass"
    required: true
    manual: false
---

# kiln-ui a11y: Announcer acceptance against the live window state, discarding inactive announcements (F3)

Follow-up to T572 (Mac T1668, A11Y-2 kiln-ui a11y kit), from its round-3 review (review rd71ft, candidate cpxg7c). T572's C1 was narrowed to name F1, F3 and F4 as known limits.

This card fixes **F3** only. F1 (recorder lifecycle) and F4 (namespaced recorder identity) moved to their own card on 2026-11-11: with gpui-pre 0.3.9, nothing runs after a draw that records no node (`Window::defer` is armed only by a record, `on_next_frame` runs at frame-request start, `Window::draw` has no post-draw hook, and the frame internals are pub(crate)), and a namespace-qualified id needs `node()`/`node_with()` to take `&mut Window`, which changes callers outside this scope. Both need a design choice first.

**F3, Announcer acceptance mid-draw** (kiln/crates/kiln-ui/src/a11y.rs:251, :302). When the receiving window is already being updated (including during render), GPUI rejects the nested update and the lookup falls back to the Announcer's previous listening flag. After deactivation, a view rendering before the Announcer can submit an inactive announcement, which render keeps and replays on activation. Check or defer acceptance against the receiving window's current accessibility state, never a stale flag, and discard inactive messages.

Put the new tests in a new file, kiln/crates/kiln-ui/src/tests/a11y_lifecycle.rs.
