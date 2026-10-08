---
task_uid: "01m493ctp82swfjd4w2q7wwt9r"
task: "T129"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::toolbar_history::buttons_follow_the_history: with an empty history both buttons are disabled; after an f12 jump back is enabled and forward disabled; clicking back returns to the jump's origin as ctrl-minus does and enables forward; clicking forward returns to the target and disables it; a new jump after going back disables forward again"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::toolbar_history::every_pane_shows_one_history: with two panes, both panes' buttons show the same state after a jump; clicking back in the inactive pane activates it and opens the history's target there; each click records one nav.back or nav.forward invocation with source Mouse"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::toolbar_history::crumb_picks_push_history: choosing sibling a from the b symbol crumb, then clicking back, puts the caret where it was inside b; choosing another file from an entry crumb's picker, then clicking back, reopens the first file at its old caret"
    required: true
    manual: false
  - id: "C4"
    text: "No new commands or bindings; the button state is pushed when NavHistory changes, not computed per frame (a test-only counter of NavButtons pushes stays flat across redraws with no history change); existing tests::nav and tests::pane_toolbar tests pass unchanged"
    required: true
    manual: false
---

# Enable the pane toolbar's back and forward buttons from the navigation history

# Enable the pane toolbar's back and forward buttons from the navigation history

A follow-up to the pane toolbar card (T127), which draws back and forward disabled because the
navigation history arrives with T107, after T81's golden screenshot. It builds on:

- T127's `NavButtons { back, forward }` seam, which the workbench pushes into each pane (always both
  `false` so far), and its buttons, which dispatch `nav.back` / `nav.forward` as a `RunCommand` with
  `Source::Mouse` when enabled
- T127's toolbar picker, whose entry and symbol picks move the caret or open a file
- T107's `NavHistory` (at most 100 `(path, offset)` entries; a jump pushes where the caret was and
  clears the forward stack), its `nav.back` / `nav.forward` commands (ctrl-minus, ctrl-shift-minus), and
  its `pub(crate)` `push_history(cx)` seam in `crates/fe-workbench/src/nav.rs`
- T112, which also edits `nav.rs` and `workbench.rs`

## Change

- **State from the history.** The workbench pushes `NavButtons { back: history can go back, forward:
  history can go forward }` into every pane whenever the history changes: a push, a back, a forward, or
  the forward stack being cleared. It's derived from `NavHistory` when it changes, not on every frame.
  `nav.rs` gains whatever small `pub(crate)` accessor or change notification that needs.
- **One history, every pane.** The history is the workbench's, so every pane's buttons show the same
  state. Clicking back in an inactive pane activates that pane (T79's mouse-down), then runs `nav.back`,
  which opens the target in the active pane as T107 defines.
- **Breadcrumb jumps are jumps.** Choosing a symbol from a symbol crumb's picker, or a file from an entry
  picker, first pushes the caret's position with T107's `push_history`, so back returns there, as an
  F12 jump does.
- Clicks and keys run the same handler: a button only dispatches the command.

`crates/fe-workbench/src/workbench.rs`, `crates/fe-workbench/src/pane_toolbar.rs` and
`crates/fe-workbench/src/toolbar_picker.rs` change only as this wiring needs.

### Tests

New file `crates/fe-workbench/src/tests/toolbar_history.rs`, declared in `tests/mod.rs`: headless GPUI
over a `tempfile` Rust fixture, clicks at the toolbar buttons' laid-out bounds.

## Out of scope

- Per-pane histories.
- A history dropdown (long-press or right-click on back).
- Mouse back/forward buttons and trackpad swipes.

## Decided (lead defaults; the user may revisit)

- One workbench-wide history drives every pane's buttons, as in VS Code.
- Breadcrumb picks push history; plain caret moves don't.
