---
task_uid: "01m4t1gj6w6chwrs2pqexxq19r"
task: "T98"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "view::ghost_text_key_tests::host_edits_end_the_suggestion_blur_dropped: after blur drops a suggestion, an edit to another row through model_mut() (caret unmoved) and, separately, through a split() view of the shared buffer each make a later palette ghost_text.accept insert nothing and emit no Accepted event"
    required: true
    manual: false
  - id: "C2"
    text: "the suggestion blur dropped is restored only when the buffer version is unchanged since the blur; view::ghost_text_tests::the_palette_takes_back_what_blur_dropped, view::ghost_text_tests::only_a_palette_accept_takes_back_what_blur_dropped and fe-workbench palette_accepts_the_suggestion still pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui -p fe-workbench and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Drop the suggestion blur saved on any buffer edit, so a palette accept after a formatter, workspace edit or reload inserts nothing (T88 review F1) (Mac T391 follow-up)

## Problem

T88 (07e8e25) keeps the suggestion that losing focus drops in `EditorView::blurred_ghost_text`
(`crates/fe-gpui/src/view.rs:136`) so a palette pick of Accept Suggestion can take it back. Its spec says an
intervening edit clears it. The only edit path that clears it is `EditorView::changed()`
(`crates/fe-gpui/src/view.rs:491`), which the view's own commands call.

T88 review finding F1 (medium): edits through the public `EditorView::model_mut()` (view.rs:475) never reach
`changed()`. Formatter replies (`crates/fe-workbench/src/lsp_format.rs:122`), workspace edits
(`crates/fe-workbench/src/workspace_edit.rs:100`) and disk reloads (`crates/fe-workbench/src/disk_sync.rs:490`)
all edit through `model_mut()` and then only notify. An edit through another view of the same buffer
(`EditorModel::split`) misses it the same way. If such an edit lands while the palette is open and changes
text away from the caret, `GhostText::is_live` still holds at the restore in `run_ghost_text_command`
(`crates/fe-gpui/src/view/ghost_text.rs:191`), so the palette accept inserts the saved suggestion into text
that has changed since it was offered.

## Change

Record the buffer version along with the dropped suggestion, and restore only if the buffer hasn't changed
since.

- In `crates/fe-gpui/src/view.rs`, make the field `blurred_ghost_text: Option<(GhostText, fe_buffer::Version)>`
  (update its doc comment). `changed()` keeps clearing it.
- In `crates/fe-gpui/src/view/ghost_text.rs`, the blur listener in `dismiss_ghost_text_on_blur` stores
  `(ghost, view.model.snapshot().version())`.
- `run_ghost_text_command` restores the saved suggestion only when `self.model.snapshot().version()` equals
  the saved version, after `sync_display`, and still checks `accepts_text` and `is_live`. Otherwise it drops
  the saved suggestion, and the accept inserts nothing and emits no event.
- Leave `set_ghost_text`, `forget_blurred_ghost_text_if_focused` and `forget_blurred_ghost_text_unless_taken`
  as they are, apart from the field's new type.

Regression test `host_edits_end_the_suggestion_blur_dropped`, in the existing
`crates/fe-gpui/src/view/ghost_text_key_tests.rs` (declared at base from view.rs; leave `ghost_text_tests.rs`
alone, since T97 owns it). Using that file's `editor(false, …)` and `suggest` helpers:

1. Focus the view, suggest `bar()` at `CARET`, draw, then `window.blur` and run until parked, so the
   suggestion is dropped and saved.
2. Edit row 1 (`next`) through `view.model_mut().buffer_mut().edit(…, Origin::User)` and only `cx.notify()`,
   as the formatter does. The caret stays at `CARET`.
3. Run `ghost_text.accept` through `run_command` with `fe_command::Source::Palette`, in a context stamped
   with `super::ghost_text::stamp(…, true)`, the way `ghost_text_tests::run` does it.
4. Assert that the text equals the edited text without `bar()`, that no suggestion shows, and that no
   `Accepted` event was emitted.
5. Do it again with the edit made through `view.model().split()` (another view of the shared buffer)
   instead of `model_mut()`.

At base, each palette accept inserts `bar()`, so the test fails.
