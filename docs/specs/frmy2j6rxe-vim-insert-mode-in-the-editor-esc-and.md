---
task_uid: "01m4a5aq1vyb9z5bfrmy2j6rxe"
task: "T162"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::insert_tests::insert_session_round_trip: with vim on, i then simulate_input(\"hello\") then escape inserts hello at the caret, EditorView::vim_mode() is normal again and the caret is on the o; u then restores the original text in one step; ctrl-[ ends a session as escape does; 3 i x escape inserts xxx; IME text marked and then committed in insert mode lands in the same session"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::insert_tests::insert_mode_keeps_the_macos_keys: in insert mode alt-backspace deletes the word left, enter inserts a line break, cmd-z undoes, and typed j and k insert text; the input handler is registered in insert mode and not again after escape (simulate_input then types nothing)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::insert_tests::other_commands_split_the_session: i a b left c d escape, then u, and then ., each leave the text and caret nvim 0.12.4 leaves for iab\u003cLeft\u003ecd\u003cEsc\u003e, u and . (recorded by hand and cited in the test); a mouse click inside the session splits it the same way; the session stays in insert mode after each split"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test vim::insert_tests::escape_bindings: the default keymap binds escape and ctrl-[ to vim.normal_mode in editor \u0026\u0026 vim.mode == insert and nothing else in that context; the pinned binding counts in command_tests.rs and keymap_tests.rs rise by exactly 2; with vim off, escape in an editor changes nothing"
    required: true
    manual: false
---

# Vim insert mode in the editor: Esc and ctrl-[ back to normal, typed text as one undo step, other editor commands splitting the session

# Vim insert mode in the editor: Esc and ctrl-[ back to normal, typed text as one undo step, and other editor commands splitting the session

Milestone M7 "Vim", GPUI wiring, card B of 2. Card A wired `fe-vim` into `EditorView` for normal mode.
T156 added insert mode to `fe-vim`: `Mode::Insert`, `vim.insert`, `c`, the insert session as one
`fe_buffer` transaction, `vim.normal_mode`, undo/redo and `.`. Its spec leaves two things to this card:
binding `Esc`/`ctrl-[` to `vim.normal_mode`, and ending the session when a non-insert command runs
(§9.8's `<C-g>u` rule, since in the app the cursor keys are macOS editor commands).

Read first: `docs/decisions/vim-engine.md` §9 items 5, 7 and 8 and §10 ("Insert mode keeps the macOS
text keys. `Esc` and `ctrl-[` return to normal mode."); T156's and card A's specs (`docs/specs/`).

## Clean room

As every M7 card (`CLEAN_ROOM.md`): no Vim/Neovim source, no hjkl, no Zed `vim` (or `editor`) crate, no
Helix. Behaviour comes from running `nvim --clean` (0.12.4) and `:help`.

## Change

### Entering and leaving

- `i a I A o O c C s S` already commit through card A's step 2; after them the editor's live frame says
  `vim.mode == insert`, and card A's rule registers the input handler again. Check it end to end; fix
  only what's missing.
- In insert mode the reducer is never called (§10 step 3, T156): an unmatched key propagates as text.
- The vim layer of `crates/fe-keymap/keymaps/default-macos.toml` gains `escape` and `ctrl-[`, each
  `vim.normal_mode`, context `editor && vim.mode == insert`. The pinned counts in fe-gpui's
  `command_tests.rs` and `keymap_tests.rs` rise by exactly 2.
- Every other `editor` binding works unchanged in insert mode: `alt-backspace`, `enter`, option-arrows,
  `cmd-z` (§10). No vim `ctrl-` key is bound in insert mode.

### Typed text joins the session

Typing and IME text reach `EditorModel` through the view (`type_text`, `replace_text_in_range`,
`replace_and_mark_text_in_range`). Check that they join T156's open transaction (its origin), so
`ihello<Esc>u` undoes in one step; fix the origin in fe-gpui if not.

### Splitting the session

While an insert session is open, any editor command other than text input (`editor.insert`,
`editor.newline`, `editor.backspace`, `editor.delete`, `editor.delete_word_left`, `editor.tab`, and
the IME paths) ends the session's transaction before it runs, and the next typed text starts a new one,
still in insert mode. A mouse click that moves the caret counts as such a command. Follow nvim for what
`.` and `u` see after a split: record `ab<Left>cd<Esc>` then `u` and then `.` in `nvim --clean` and cite
the result in the test. Use T156's public API for this; if it has no way to split a session, add one
small public method in `crates/fe-vim/src/insert.rs`, documented, and nothing else in `fe-vim`.

### Cursor

Insert mode paints the 2px bar (card A's block is for `normal` and `operator_pending` only). The status
bar shows `INSERT` through card A's item; nothing to add.

## Tests (new files)

`crates/fe-gpui/src/vim/insert_tests.rs`, declared from `vim.rs`, `#[gpui::test]`s in a headless window
as card A's `vim/tests.rs`.

## Out of scope

- Visual modes, replace mode, macros, marks, search, ex, the underline cursor (§9.7).
- Vim's insert-mode `ctrl-` keys (`ctrl-w`, `ctrl-u`, `ctrl-r`): not bound by default (§10).
- `jk`-style escapes in the default keymap (a user can bind them, §10).
- Key-to-key remaps: not designed yet.
- Any `fe-vim` change beyond the one method above.

## Implementation notes

- **Entering, typing, IME.** `i a I A o O c …` already left the live frame at `vim.mode == insert` and
  re-registered the input handler; nothing was missing. Typed text (`editor.insert` from the
  keyboard, `Actor::User`) and the IME paths (`EditorModel::insert` directly) both edit with
  `Origin::User`, the origin `vim.insert` opens the session with, so they join its transaction
  unchanged.
- **`Enter` in insert mode** runs through `VimState::line_break` (as T156's spec says the GPUI wiring
  would), acting as the invocation's actor, instead of the editor registry's `editor.newline`. So
  `A<CR><Esc>` on an indented row drops the copied indentation, as nvim's 'autoindent' does.
- **The split** is `VimState::split_session(model, edits, command)`, the one public method added to
  `fe-vim` (`src/insert.rs`). It takes the command as a closure, and whether it may edit (the
  registry's `mutates_buffer`). A command that edits (undo, paste, cut, …) runs after the session
  has ended, as the spec says, so its edits are their own undo step. A command that doesn't (the
  cursor keys, a click or drag, copy, fold) runs inside the session, which ends after it only if
  it changed the selections, ending the typed text at the primary caret as it was before. Then a
  new session opens at the carets left, as `i`, keeping any selection. This follows nvim, where a
  cursor key that can't move (`<Right>` at the end) doesn't split, and where the unused
  indentation goes when the cursor leaves its row (`o<Up>`, in the same undo step) but stays while
  it moves along it (`o<Left>`); the caret keeps its column and goal. Its private helpers live in
  the same file: `normal_mode` became `end(model, state, Ending)` (a split skips the count's copies
  and the step back), `enter`'s session construction moved to `open`, `resume` opens the new
  session, and `drop_unused_indent` can keep the rows the carets are on and moves selections
  rather than collapsing them. A `resumed` flag on the session keeps `.`'s last change when a
  resumed session changes nothing. Sessions not made by a split behave as before.
- **What the split follows from nvim** (`nvim --clean` 0.12.4, recorded by hand, cited in
  `other_commands_split_the_session`): the session before the split is its own undo step; the
  count is dropped (`3iab<Left>cd<Esc>` gives `acdb`); `.` repeats what was typed after the split
  as `i` (a backspace alone repeats as nothing typed, as nvim's `i<BS>` at a row's start); a
  split followed by no change leaves no undo step and `.` the change before it
  (`iab<Left><Esc>u.`, and `3aab<Left><Esc>u.` repeats the count).
- **Which commands split**: in `run_command`, any command the editor registry knows, other than
  `editor.insert`, `editor.backspace`, `editor.delete`, `editor.delete_word_left`, `editor.tab`
  and `editor.newline` (`vim::TEXT_INPUT`, `vim::NEWLINE`). Commands the editor doesn't know, which
  propagate to the workbench or the app, don't. Mouse clicks and drags split as commands that
  don't edit.
- **Known limits**: paste and cut split the session (they aren't in the spec's text-input list),
  so `.` after `ifoo` cmd-v `bar` repeats `bar`, where nvim's `<C-r>` paste stays in the session.
  A non-editing command runs with the session's transaction open, so one that edited anyway
  despite `mutates_buffer = false` would join it; none does.
