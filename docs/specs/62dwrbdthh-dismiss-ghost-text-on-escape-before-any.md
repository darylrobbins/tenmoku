---
task_uid: "01m4sjp1wjpg8s9c62dwrbdthh"
task: "T84"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::ghost_text_key_tests::blur_repaints_without_another_draw: after blur drops a painted suggestion, the window redraws without the test forcing a draw, painting no ghost text (F1)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::ghost_text_key_tests::escape_dismisses_before_bindings: with escape bound to ghost_text.accept in a user keymap, escape drops the suggestion with one Dismissed and no edit, and in vim insert mode the same escape also enters normal mode (F3)"
    required: true
    manual: false
  - id: "C3"
    text: "view::ghost_text_key_tests pass unchanged; cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Dismiss ghost text on escape before any key binding runs, and repaint once blur drops it (T53 review F1, F3) (Mac T391 follow-up)

Follow-up from T53 (Mac T391, ghost text accept/dismiss, landed as f7aae89), filed on helium. Review 01m4sh38nqjmfmrwem6gzepvk9
(`rgt review show T53`), findings F1 and F3. F2, accepting from the command palette, is a separate card that
depends on this one. This card depends on the T69 F1 card, which also edits `crates/fe-gpui/src/vim.rs`.

## Problem

- **F1 (`crates/fe-gpui/src/view/ghost_text.rs` :133).** The blur listener in
  `dismiss_ghost_text_on_escape_and_blur` calls `dismiss_ghost_text`, which calls `cx.notify()`. Blur listeners run
  in GPUI's `DrawPhase::Focus`, which ignores frame invalidation. So the painted suggestion can stay on screen until
  something else redraws the window. `view::ghost_text_key_tests::escape_blur_and_accept_word` masks this by
  drawing after the blur. The hover/composition blur listener in `crates/fe-gpui/src/view.rs` (about :1244) already
  handles this with `cx.defer_in(window, |_, _, cx| cx.notify())`.
- **F3 (view/ghost_text.rs :128).** The escape interceptor is per view, registered on first render, so it runs after
  kiln-platform's keymap interceptor (`kiln_platform::keys::init`, installed by `crate::keymap::init`). GPUI runs
  interceptors in registration order. If a user binds `escape` to `ghost_text.accept`, escape accepts and edits the
  buffer before the dismissal runs. The spec says escape drops the suggestion "before any binding takes the key".

## Change

- **Blur (F1).** The blur listener drops the suggestion and emits `GhostTextEvent::Dismissed` as now, but defers the
  redraw with `cx.defer_in(window, |_, _, cx| cx.notify())`, as the hover listener does.
- **Escape (F3).** Move escape dismissal into one app-wide keystroke interceptor installed in
  `crate::keymap::init` (`crates/fe-gpui/src/keymap.rs`) before `keys::init`, so it runs before matching. Install it
  once, holding its `Subscription` in a global so a second `init` doesn't add another. On an unmodified `escape`, it
  finds the focused editor with `crate::vim::focused_editor` (`crates/fe-gpui/src/vim.rs` :273, made `pub(crate)`; that
  one-word change is this card's only edit in vim.rs) and calls
  `dismiss_ghost_text`. The keystroke then goes on to matching as before, so in vim's insert mode one escape still
  drops the suggestion and enters normal mode. Remove the per-view escape interceptor from
  `dismiss_ghost_text_on_escape_and_blur` and keep only its blur listener.

Existing `view::ghost_text_key_tests` pass unchanged.

## Tests

Add the tests to the existing `crates/fe-gpui/src/view/ghost_text_key_tests.rs` (already declared from view.rs at
base), next to `escape_blur_and_accept_word`, leaving its existing tests unchanged. Not a new file: Regatta's
regression check applies only test files to base, so a test module declared in view/ghost_text.rs can't run there.

- `blur_repaints_without_another_draw`: after a frame painted the suggestion, blur. With no explicit
  `window.draw`, the next frame GPUI runs paints no ghost text, or the window is invalidated after the focus phase.
- `escape_dismisses_before_bindings`: with a user keymap binding `escape` to `ghost_text.accept` in the
  `ghost_text` context, escape drops the suggestion with one `Dismissed`, the buffer unchanged. With vim on in
  insert mode, the same escape also enters normal mode.
