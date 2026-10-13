---
task_uid: "01m4skj80e2faf5pecfsb0gw9b"
task: "T90"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test full_accept_then_undo_is_not_reoffered: accept, and accept-word or accept-line that consume the rest, clear the driver's suggestion, so an undo after them does not show it again. A partial accept keeps it (F1)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test hand_undo_redo_updates_touched_rows: undo of an accepted hunk and of a streamed region, made through the view with no further script step, removes their touched-row marks, and redo puts them back. The observer settles after one extra notify (F2)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test hand_accepted_hunk_still_marked: a hunk accepted through EditorView::accept_hunk is still marked with the version observer installed (F2)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test ghost_steps_report_outcome: the ghost step runner returns Report::Accepted(transaction) or Report::Dismissed when the command acts, and StepError::NoSuggestion for a stale or missing suggestion (F3)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo build -p fe-gpui --example ai_demo, cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass. No Cargo.toml, deny.toml or screenshot golden changes"
    required: true
    manual: false
---

# Stop ai_demo reoffering accepted suggestions, keep touched-row marks in step with hand undo and redo, and report each scripted ghost step's outcome (T55 F1-F3, Mac T502 follow-up)

## Problem

T55 (Mac T502) landed on main as 1f4f321, "Add an ai_demo example that plays a scripted fake AI in a
window" (spec `docs/specs/*5qjntgx74b-add-an-ai-demo-example-that-plays-a.md`, host guide
`docs/ai-host.md`). It was approved with three findings open, all in
`crates/fe-gpui/examples/ai_demo.rs`:

- **F1 (medium), ai_demo.rs:151, also :346.** The `GhostTextEvent::Accepted` subscriber only prints. It
  never clears the driver's suggestion (`ScriptedAi::ghost_text`), so after `suggest foo; accept; undo`
  the old suggestion is live again and the "shown again" branch in `run` (:346) reoffers a suggestion
  that was accepted in full. The same happens when `accept-word` or `accept-line` consumes the
  remainder, and when the user accepts by hand (`tab`).
- **F2 (medium), ai_demo.rs:255, 264, 284, 287, 358, 362.** The driver follows undos and redos of
  touched transactions only inside `ScriptedAi::step` and `ScriptedAi::note_accepted` (its private
  `catch_up` drains its buffer subscription). Keyboard undo/redo during a `wait`, while stdin is idle,
  or in hand play after the script ends (both `script done` loops, and after stdin EOF, where `play`
  returns) never services those pending buffer events. Undoing an accepted hunk by hand leaves its
  touched-row mark, and a redo can't restore a mark whose anchors collapsed.
- **F3 (low), ai_demo.rs:313.** For `accept`, `accept-word`, `accept-line` and `dismiss`, `run` checks
  `view.ghost_text().is_some()`, dispatches the `ghost_text.*` command and prints only the event,
  never the step's `Report` as every other step does. `is_some()` doesn't establish liveness, so a
  stale accept can silently do nothing with no `StepError`.

## Change

Example-only; no change to fe-editor, fe-gpui's library, Cargo manifests or the screenshot goldens.

1. **Extract the host glue so it is testable headless.** Move the view/driver wiring out of `main` into
   `crates/fe-gpui/examples/ai_demo/host.rs`, included from `ai_demo.rs` with
   `#[path = "ai_demo/host.rs"] mod host;` (a directory without `main.rs` isn't auto-discovered as an
   example). It holds: one function that installs the `GhostTextEvent`, `ProposalEvent` and
   buffer-version subscriptions for an `Entity<EditorView>` and an `Rc<RefCell<ScriptedAi>>`, and one
   that runs a scripted ghost step (`accept`, `accept-word`, `accept-line`, `dismiss`) in a window and
   returns `Result<Report, StepError>`. `ai_demo.rs` keeps argument parsing, the stdin thread, pacing
   and printing. The new test file `crates/fe-gpui/tests/ai_demo_host.rs` includes the same module with
   `#[path = "../examples/ai_demo/host.rs"] mod host;` and drives it with `gpui::TestAppContext`
   (already a dev-dependency, as in `src/pairs_host_tests.rs`).
2. **F1.** On `GhostTextEvent::Accepted { whole: true, .. }`, clear the driver's suggestion without
   touching the text: `ai.step(model, &Step::Dismiss)` (it only takes the suggestion). Keep it on
   `whole: false`. This covers the scripted steps and acceptance by hand alike, so the "shown again"
   branch can never reoffer a fully accepted suggestion.
3. **F2.** Observe the view (`cx.observe`) for the window's lifetime, independent of `play`, so it
   keeps running after `script done` and after stdin EOF. When the buffer's version differs from the
   last one serviced, service the driver with a silent `ai.step(model, &Step::Wait(Duration::ZERO))`
   (nothing printed), record the version, and `cx.notify()` so the marks repaint. The version check is
   the loop guard: the notify re-fires the observer once, which then sees no change. The servicing must
   not swallow the accept's `Edited` event before `note_accepted` runs on `ProposalEvent::Accepted`
   (gpui flushes notify and emit effects in order): have the `ProposalEvent::Accepted` handler mark the
   hunk first, or skip servicing while an accept is pending, so a hand-accepted hunk is still marked.
4. **F3.** Track each scripted ghost command's outcome: clear a shared `Option<Report>` cell, dispatch
   the command, and have the `GhostTextEvent` subscriber fill it (`Accepted { transaction, .. }` →
   `Report::Accepted(transaction)`, `Dismissed` → `Report::Dismissed`). Print that report, or
   `StepError::NoSuggestion` when no event came. Drop the `ghost_text().is_some()` pre-check. Keep
   printing each `GhostTextEvent` as its own `event:` line.
5. Update `ai_demo.rs`'s module docs and the "Marking what the AI touched" section of
   `docs/ai-host.md`: the host services the driver's undo/redo bookkeeping on every buffer-version
   change, not only at the next step.

## Implementation notes

- `examples/ai_demo/host.rs` holds `install` (the `GhostTextEvent`, `ProposalEvent` and version
  subscriptions, all detached so they last as long as the view, past stdin EOF), `run_ghost_step` and
  `show_again` (the "shown again" branch, so the tests can check it). `ai_demo.rs` keeps its own
  printing subscribers, registered first so each `event:` line precedes what the host does.
- F2's ordering: the observer `cx.defer`s its servicing, which gpui runs after every effect already
  queued, so a `ProposalEvent::Accepted` queued behind the notify reaches `note_accepted` first. The
  C3 test notifies before `accept_hunk` in the same update (as a click focusing the view would) so the
  observer does run first; servicing without the defer fails it.
- F3: a ghost accept of a stale suggestion makes the view drop it with `Dismissed`, so the runner
  matches the outcome to the step: only `Accepted` answers an accept, only `Dismissed` a dismiss.
- The C2 stream undo and redo are the host's own, on the view's model (`buffer_mut().undo(Origin::is_ai)`
  then a notify): the view has no undo-ai command. The hunk's go through the view's `editor.undo` and
  `editor.redo` commands, as their keys do.
- Seen while checking by hand, not changed: quitting the demo with `q` panics with gpui's leaked-handle
  check (test-support is on for the example). Base 1963efd does the same.
