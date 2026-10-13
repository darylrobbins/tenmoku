# Driving fastedit's AI seams from a host

An embedding host owns the AI (a model, an agent, a network call); fastedit
owns the buffer, the view and undo. The host reaches each seam through public
API of fe-buffer, fe-editor and fe-gpui. `crates/fe-gpui/examples/ai_demo.rs`
makes every call below against a scripted fake AI (`fe_editor::scripted_ai`,
whose module docs give the script grammar), and is the reference to copy.

## Ghost text

- Make a suggestion at the caret with `GhostText::new(model.snapshot(), at,
  text, origin)`, `origin` an `Origin::Ai { agent, session }`.
- Show it with `EditorView::set_ghost_text(Some(ghost), cx)`; `None` clears
  it. Typing what it suggests consumes it; a caret moved away ends it.
- The user takes it with the `ghost_text.*` commands (`tab` is
  `ghost_text.accept`; also `accept_word`, `accept_line`, `dismiss`).
  Subscribe to `GhostTextEvent`: `Accepted { transaction, text, whole }` or
  `Dismissed`. The host's own `set_ghost_text` raises neither.
- On `Accepted { whole: true, .. }`, drop the host's copy of the suggestion:
  an undo puts the caret back where it would be live again, and it was
  taken. `ai_demo` keeps one accepted in part, and one the caret left.

## A streamed edit

- `EditStream::begin(&view.model().buffer(), range, origin)`, then
  `stream.push(&mut view.model_mut().buffer_mut(), chunk)` per chunk and
  `stream.finish(...)` at the end, `cx.notify()` after each. The user keeps
  typing around it: the region moves with their edits.
- Undo AI is `view.model_mut().buffer_mut().undo(Origin::is_ai)` (and
  `redo`).

## A proposed change

- `ProposedChange::from_text(snapshot, range, new_text, origin)`, held as
  `Rc<RefCell<_>>`, shown as diff blocks with Accept and Reject buttons by
  `EditorView::set_proposals(vec![proposal], cx)`.
- Resolve a hunk from the host with `EditorView::accept_hunk(id, cx)` or
  `reject_hunk(id, cx)`, as the buttons and the `proposal.*` keys do. Hunk ids
  come from `proposal.borrow().hunks(view.model().snapshot())`.
- Subscribe to `ProposalEvent`: `Accepted { hunk, transaction }` or
  `Rejected { hunk }`, whichever way the user resolved it.

## Marking what the AI touched

Register a provider once with `EditorModel::add_decoration_provider(set)`,
`set` an `Rc<RefCell<DecorationSet>>`, and `splice` row backgrounds
(`DecorationKind::RowBackground`) into it as the AI writes. `ai_demo` marks
streamed regions and, on `ProposalEvent::Accepted`, the accepted hunk
(`ScriptedAi::note_accepted`).

Anchors can't follow text deleted and restored, so an undo of a marked edit
has to take its mark away and a redo has to mark where it lands again. The
driver does that bookkeeping from the buffer's edit events, whenever it's
serviced. `ai_demo`'s host (`examples/ai_demo/host.rs`) services it on every
change of the buffer's version, not only at the next step: it observes the
view for the window's lifetime and, when the version differs from the last
it serviced, runs a silent zero `wait` step and notifies the view so the marks
repaint. So an undo or redo by key, during a `wait`, or after the script ends
updates the marks as it happens. The version check stops the loop its own
notify would start. The servicing is deferred until the view's events have
gone out, so `note_accepted` still sees the accept as the buffer's last edit.

## The undo rule

Accepting a suggestion or a hunk is the **user's** edit: one `Origin::User`
transaction that cmd-z (`editor.undo`, `EditorModel::undo`) takes back. A
stream is the **AI's**: one AI-origin transaction, however many chunks, that
the user's undo never touches and one undo AI reverts whole, typing beside it
kept.

## Running the demo

```text
cargo run -p fe-gpui --example ai_demo                       # DEMO_SCRIPT on DEMO_TEXT
cargo run -p fe-gpui --example ai_demo -- <file> <script>    # your file and script
cargo run -p fe-gpui --example ai_demo -- <file> -           # steps typed on stdin
```

It prints each step and what it did, and each `GhostTextEvent` and
`ProposalEvent`, then `script done`; the window stays open for hand play until
closed, or `q` on stdin when the script isn't stdin. A bad file or script
exits with status 2. On Linux, gpui's X11 and Wayland backends are features
only the app turns on: add
`--features gpui_platform/x11,gpui_platform/wayland` to the `cargo run`.
