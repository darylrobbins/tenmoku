---
task_uid: "01m4ewtxmv14nfcwe67djkhktw"
task: "T219"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::vim_line::search_through_the_line: with vim on and a focused editor over \"one\nfoo\nbar foo\", typing / shows a focused line whose prefix is /; typing foo then enter moves the editor's caret to (1,0) and focus returns to the editor; n moves to (2,4); typing /zzz then enter leaves the caret and shows E486 on the status bar's left until the next key; d / foo enter deletes up to the match in one undo step; escape in the line and backspace on an empty line cancel with no change; with the status bar hidden the line still shows while open"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::vim_line::ex_through_the_line: : then 2 enter moves to row 2; v j : shows the prefill '\u003c,'\u003e and s/o/0/g enter changes only those rows; :frobnicate shows E492; :w leaves the buffer dirty and shows no error (06c runs it); the line's text input carries no vim.mode in its context, so typing j or d in it only types; clicking the editor while the line is open cancels it"
    required: true
    manual: false
  - id: "C3"
    text: "The default keymap gains exactly two bindings, enter to text_input.submit in vim_line \u0026\u0026 text_input and escape to vim_line.cancel in vim_line; vim_line.cancel is in fe_workbench::command_catalog(); fe-workbench test tests::outline_panel::the_default_keymap_binds_outline_show passes unchanged; the counts and context lists pinned in fe-gpui's command_tests.rs and keymap_tests.rs change only by those two bindings and the vim_line context; the screenshot golden images are unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "Scope: nothing under crates/fe-vim or crates/fe-gpui/src changes other than: the two pinned test files; in crates/fe-vim/src/state.rs one public method on VimState, pub fn clear_message(\u0026mut self) (about 4 lines, with a fe-vim unit test in that file's tests showing it clears the message and nothing else); in crates/fe-gpui/src/vim.rs one read-only accessor on EditorView (such as pub fn vim_state(\u0026self) -\u003e Option\u003c\u0026fe_vim::VimState\u003e, about 4 lines) and, in live_stack, a call to clear_message for each keystroke in the focused editor before the keymap or reducer runs, notifying if it cleared one (about 3 lines), nothing else in that file; and the two contexts vim_line \u0026\u0026 text_input and vim_line added to the allowed-context list in crates/fe-gpui/src/vim/tests.rs (nothing else in that file), apart from the task's own spec under docs/specs/ and its decision record. The status bar holds no key interceptor or dismissal state of its own for messages. crates/fe-workbench/src/lib.rs doesn't change; default-linux.toml gains the same two bindings as default-macos.toml; tests::search_panel::the_default_keymap_binds_the_search_commands changes only to expect text_input.submit's second binding; T161's vim::tests (apart from those two contexts), T162's vim::insert_tests, T175's vim::visual_tests, fe-vim's existing tests and the status bar's vim tests pass unchanged"
    required: true
    manual: false
---

# Vim command line in the workbench: / ? and : typed at the bottom of the window, submitted to fe-vim, with its error messages in the status bar

# Vim command line in the workbench: / ? and : typed at the bottom of the window, with fe-vim's messages

Milestone M7 "Vim", second batch (resized), card 06a: the line part of the old card 6, the GPUI
wiring of 04b–05c. Read their specs, T161's, T162's and T175's, and `docs/decisions/vim-engine.md`
§9 item 12 and §10 ("The palette, search and picker inputs have their own contexts and never carry
`vim.mode`, so they always type").

**Clean room** as every M7 card: no Vim/Neovim source, no hjkl, no Zed, no Helix.

## What exists

- No command palette and no in-editor find. The only single-line input is `fe-ui`'s `TextInput`
  (append-only: no caret movement, no selection, no IME), used by the pickers and the search panel,
  with commands `text_input.backspace`, `text_input.delete_word_left`, `text_input.clear`,
  `text_input.paste` and `text_input.submit`.
- `fe-gpui` doesn't depend on `fe-ui`, so the line lives in `fe-workbench`.

## Change

### The line (new `crates/fe-workbench/src/vim_line.rs`, `status_bar.rs`, `workbench.rs`)

`vim_line.rs` is a child module of `status_bar.rs` (`#[path = "vim_line.rs"] mod vim_line;`), so
`lib.rs` (held by LSP and nav cards) doesn't change. `vim_line.cancel` is registered in
`status_bar::registry()`, so it is in `fe_workbench::command_catalog()`.

- When the focused editor's `vim_mode()` becomes `cmdline`, the workbench shows a `TextInput` in
  the status bar's place, prefixed by `/`, `?` or `:` (from `vim.cmdline_kind`), holding fe-vim's
  prefill (`'<,'>`, `.,.+2`), and focuses it. If the status bar is hidden it shows anyway, in the
  same place, while the line is open (the one change in `workbench.rs`'s render).
- Enter (`text_input.submit` in context `vim_line && text_input`) dispatches
  `vim.command_line_submit {"text"}` to that editor; Escape (`vim_line.cancel`, context
  `vim_line`) dispatches `vim.command_line_cancel`; backspace on an empty line cancels. Either way
  focus goes back to the editor. Losing focus to anything else (a click elsewhere) cancels.
- Keys typed in the line never reach the reducer (the line's context carries no `vim.mode`).
- After a submit, `VimState::message()` (E486, E492, an unsupported pattern…) shows on the left of
  the status bar until the next key in that editor.
- A `HostCommand` left by a submit is ignored here (06c runs them).

### Keymap (`crates/fe-keymap/keymaps/default-macos.toml`)

Two bindings: `enter` → `text_input.submit` in `vim_line && text_input`, `escape` →
`vim_line.cancel` in `vim_line`. The pinned counts and context lists in fe-gpui's
`command_tests.rs` and `keymap_tests.rs` change by exactly those two bindings and the `vim_line`
context.

## Tests (new files)

`crates/fe-workbench/src/tests/vim_line.rs`, declared in `crates/fe-workbench/src/tests/mod.rs`
(the lead's rule: not a `#[path]` test file under `src/`), as the other workbench tests drive a
test window.

## Size

About 300 non-test lines.

## Out of scope

- Search highlights (06b), host commands (06c), 'incsearch', history, completion, caret movement
  inside the line (TextInput can't yet), `q:`.
- Any fe-vim change (if 04b–05c's API lacks something, block).

## Decided (lead defaults; the user may revisit)

- **The line replaces the status bar while open**, workbench-wide, like Vim's single command line;
  not a line at the bottom of each pane, and not the (not yet built) command palette.

## Implementation notes

- `StatusBar::set_editor` takes the window now, so the bar observes its editor with
  `observe_in`: the line opens and takes focus in the same effect flush as the `/`, `?` or `:`
  that opened it, not a frame later. Its one caller, `follow_active_editor`, takes
  `&mut Window` for it.
- The line ends in the editor through `FocusHandle::dispatch_action` on the editor's handle,
  which reaches only an editor in the frame last drawn. So a line is cancelled: in `set_editor`
  when the workbench follows another editor (during its render, while the last frame still
  holds the editor a tab switch or close takes out); in the capture phase of a mouse-down
  outside the line (`on_mouse_down_out`), before the editor takes the click, so the click acts in
  the mode the line came from; and on a blur, for focus moved by a key or command. A key that
  ends it focuses the editor first.
- A blur from the window going inactive (switching apps) doesn't cancel the line: focus stays in
  it for when the window comes back.
- `StatusItem::VIM_MESSAGE` (`vim_message`, left, priority 10) shows `VimState::message()`
  whenever it's set, so `n` and `*`'s E486 show too, not only a submitted line's. fe-vim clears
  it on each key it's fed, but keys the keymap binds (arrows, Delete, `cmd-c`, a user's binding)
  never reach the reducer, so fe-gpui's `live_stack`, which runs for every keystroke in the
  focused editor before the keymap or the reducer, clears it with `VimState::clear_message` and
  notifies. A command that key runs can set a new one; a key in another window or editor leaves
  it alone. The status bar only shows what the state holds.
- The input pads its text by 12 px; the line pulls it back so the text follows the prefix.
