---
task_uid: "01m46tzw42tb3sjammcqfn18hv"
task: "T75"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test tests::shared_buffer::split_shares_text_history_and_dirty_state: after a.split(), shares_buffer_with is true both ways (false for an unrelated model); an insert in a shows in b.text() after b.sync_display(); both report is_dirty; undo in b undoes the edit a made; saving a leaves both clean"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test tests::shared_buffer::the_other_models_carets_follow_edits: with b carets after a word, inserting 10 chars before them through a and then calling b.insert(x) without an explicit sync puts x at the moved caret positions (b catches up before editing), checked with render()"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test tests::shared_buffer::display_and_folds_stay_per_model: folding rows in a leaves b unfolded; after a inserts 5 lines above the fold, a fold still covers the same text and b.display() row count equals the new line count after b.sync_display()"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test tests::shared_buffer::a_quiet_model_holds_no_events: after 1,000 edits through a and one b.sync_display(), b has no undrained events and its cached snapshot version equals the buffer version"
    required: true
    manual: false
  - id: "C5"
    text: "All existing fe-editor, fe-gpui and fastedit tests pass; no file outside crates/fe-editor changes except this task's own spec file under docs/specs/; scripts/check-no-gpui.sh prints ok: fe-editor"
    required: true
    manual: false
---

# Let two EditorModels edit one shared Buffer

# Let two EditorModels edit one shared Buffer

Milestone M3 "Browse", third batch. This card is the core half of the splits card: the workbench half
(panes, layout, commands) depends on it. The plan's M3 scope includes "splits". A split that shows the
same file twice has to edit **one** buffer, with one undo history and one dirty state. Each side keeps
its own selections, folds, wrap and scroll position.

## Problem

`fe_editor::EditorModel` owns its `Buffer` by value (`crates/fe-editor/src/lib.rs`). Opening the same
file in a second pane today would load a second, independent buffer. The two copies would then diverge,
and saving one would overwrite the other's edits.

## Change

All of it is in `crates/fe-editor` (GPUI-free).

- `EditorModel` holds the buffer as `Rc<RefCell<Buffer>>`. `EditorModel` is already not `Send`
  (`Box<dyn CharWidth>`), so nothing changes there. It also keeps its own cached `BufferSnapshot`
  (cheap to clone) as of its last catch-up.
- `EditorModel::new(buffer, path)` and `EditorModel::open(path)` are unchanged. Each makes a new,
  unshared buffer.
- New `pub fn split(&self) -> EditorModel`: a second model over the **same** buffer. It copies the path,
  tab size, soft wrap and wrap width, the char widths (`Monospace` until the view sets its own), and
  the selections. It builds its own `DisplayMap`, starting with no folds, and its own `Subscription`.
- New `pub fn shares_buffer_with(&self, other: &EditorModel) -> bool` (`Rc::ptr_eq`).
- Accessors:
  - `buffer()` returns `Ref<'_, Buffer>`. `buffer_mut()` returns a `BufferMut<'_>` guard that derefs
    (mutably) to `Buffer` and catches the model up when dropped, so a read after an edit through it
    sees the edit. Callers that only call methods through them compile unchanged.
  - `snapshot()` and `text()` keep their signatures and return the model's cached snapshot.
- **Catch-up:** another model's edits reach this one when it catches up. Catching up:
  - refreshes the cached snapshot
  - syncs the display map
  - drains its subscription (so the subscription can't grow without bound while only the other model
    edits)
  - leaves the selections, which are anchored, wherever the edits moved them

  `sync_display()` (already called by `fe-gpui` every frame) catches up. Every method that reads or
  edits through the selections with `&mut self` also catches up first: movement, `insert`, `paste`,
  `newline`, `tab`, the deletes, `cut`, `undo` and `redo`. A model therefore never computes an edit
  from a stale snapshot. A model that is already current skips the catch-up (a version compare).
  The `&self` reads (`selections()`, `primary_selection()`, `copy_text()`, `text()`, `display()`)
  keep `&self` and show the state as of the last catch-up, consistent with each other; `fe-gpui`
  catches up every frame before it reads them.
- `undo`/`redo` act on the shared history: undo in either model undoes the latest `User` transaction,
  whichever model made it. `is_dirty()` and `save()` are shared through the buffer. Saving from one
  model makes both clean.
- No `RefCell` borrow is held across a call that might borrow again. Each method borrows once,
  briefly.

Tests go in a new file `crates/fe-editor/src/tests/shared_buffer.rs`, declared in
`crates/fe-editor/src/tests/mod.rs` (one `mod` line). They can use the existing `parse`/`render`
helpers.

Nothing outside `crates/fe-editor` changes. `fe-gpui` and `fastedit` must build and pass unchanged.
If T63 has landed and `fe-workbench` holds a `&Buffer` across calls, so that it no longer compiles,
block and say so rather than editing `fe-workbench`.

## Out of scope

- The workbench side: panes, layout, split commands. That's the splits card.
- Sharing one syntax tree between two views of a buffer. Each `EditorView` keeps its own
  `SyntaxDriver`, so a file open in two panes is parsed twice.
- Redrawing the other view. `fe-workbench` notifies sibling views. This crate has no notion of views.

## Decided (lead defaults; the user may revisit)

- Undo history is shared across views of a buffer (VS Code and Zed both do this). It isn't scoped per
  view.
- A split starts with no folds and copies the selections.
- Two views of one file parse it twice for now. Sharing `Syntax` is a later optimisation if the
  memory or CPU shows up.
- (Review round 2, findings F1/F6/F4.) `&self` reads show the last catch-up rather than taking
  `&mut self`, so `fe-gpui` stays unchanged and every read agrees with `text()`. `buffer_mut()`
  returns a guard that catches up on drop instead of a bare `RefMut`. The task's own spec file is
  exempt from C5's "no file outside crates/fe-editor" rule.
