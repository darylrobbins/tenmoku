---
task_uid: "01m5nytzf643cqvwkkc53vgnpq"
task: "T305"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The tab marks conflicts in the key-context frames fe-gpui/fe-vim expose, including visual, pending-operator and cmdline frames, with no mark for a frame that cannot occur; tests for each frame Limit (Known limits, review round 2): an editor frame goes under another view only where a binding names both or a term needs an editor key; a condition that only rules out editor keys under an editor-holding view such as DiffView (for example DiffView \u0026\u0026 !vim.enabled against an editor binding) is not reported. The mark is advisory. Follow-up card F3 exposes the editor-holding key contexts. Tests for each frame"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Report keyboard-shortcut conflicts in exactly the key-context frames an editor can have, from fe-gpui and fe-vim (follow-up to Mac T1100)

## Problem
T299 (Mac T1100) narrowed C3 at review round 3: the Keyboard Shortcuts tab finds conflicts by witnesses built from predicate atoms, the editor feature flags and vim.enabled. A clash that depends on a full vim-stamp combination (visual only in visual mode, pending_kind only while pending, cmdline kind) may go unmarked.

## Change
fe-gpui / fe-vim expose the set of key-context frames an editor can have (the stamps VimState produces); the tab's witness generation reports in exactly those frames instead of mirroring them in fe-workbench.

## Tests
Conflicts that exist only in visual, pending-operator and cmdline frames are marked; no false marks for a frame that cannot occur. Headless, Linux-OK.

## Implementation notes

- `fe_vim::frames(&frame)` lists every frame `VimState::stamp` makes of an
  editor's frame: each mode with what can be pending in it (as
  `reducer::step` and the jump and surround features take keys), with and
  without `vim.replace_with_register`, `vim.insert_pending` (not in insert or
  replace mode) and `vim.recording` for each register `a`–`z`.
  `frames_tests::frames_are_exactly_the_stamps` drives a `HeadlessVim`
  through key sequences for each mode and pending kind, from each start
  (recording or not, after `<C-o>` or not, `gr` ReplaceWithRegister's or
  not), and checks the stamps reached are exactly the listed frames
  (recording into `a`); a 20 000-key walk checks no stamp falls outside them.
- `EditorView::key_context_frames()` (fe-gpui, in `vim.rs` since fe-gpui's
  `lib.rs` is out of scope) adds the editor's own flags: the completion menu
  and a suggestion only with vim off or in insert and replace mode (the only
  modes that accept text and so keep them), a proposal in any.
- The tab's `clashes` no longer mirrors editor flags or `vim.enabled`: it
  projects the editor frames onto the flags and key values the keymap's
  contexts name (a value no context names becomes the first such value seen,
  which every predicate reads the same way), so the frames reported in stay
  few, and adds each projected frame, innermost, to a witness of a term that
  needs one of the editor's flags or keys (or rules some out with no other
  view named, as `!vim.enabled`): `project_panel && !vim.enabled` holds in
  the panel alone, with no editor in it (review F1). Which keys are the
  editor's comes from the whole frame list, before projection, so
  `vim.recording` used as a flag is still known for the editor's key, which
  never holds as one (review F2). Contexts where the binding's predicate
  fails are dropped as before.

## Known limits

- An editor frame goes under another view only where a binding's term
  names both or needs one of the editor's flags or keys. A condition that
  only rules out editor keys under a view that holds editors, such as
  `DiffView && !vim.enabled` against an `editor` binding, is not reported:
  no crate yet says which key contexts an `EditorView` can sit in (review
  round 2, F3). The mark is advisory. A follow-up card exposes the
  editor-holding key contexts so `clashes` can attach editor frames under
  exactly those.
