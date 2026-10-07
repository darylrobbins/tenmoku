//! Vim mode for fastedit's editor, headless. Must never depend on GPUI.
//!
//! Keys go through a pure reducer, [`step`], which keeps the whole
//! incomplete command (count, `"{reg}`, operator, a prefix such as `g`, `f`
//! or `r`) in one [`Pending`] value and never sees the buffer. A complete
//! command comes out as an [`Invocation`](fe_command::Invocation) of one of
//! vim's commands (`vim.move`, `vim.operate`, `vim.put`, `vim.insert`,
//! `vim.join`, `vim.replace_char`, `vim.toggle_case`, `vim.undo`, `vim.redo`
//! and `vim.repeat`), dispatched through [`registry`] against a
//! [`VimTarget`], which carries the host's shared [`Registers`].
//! [`VimState`] holds the mode, the pending value, the goal columns, the
//! last character search, the open insert session, the last change for `.`
//! and the carets before each change for `u`, and publishes `vim.mode`
//! (plus `vim.pending` and `vim.pending_kind` while a command is incomplete)
//! on the editor's own context frame, so keymap bindings can be scoped to a
//! mode. With vim off nothing stamps the frame and `vim.mode` is never set.
//! The design is `docs/decisions/vim-engine.md` (§1, §4, §9 and §10).
//!
//! An insert session (`i a I A o O`, or the change operator `c`) is one
//! `fe_buffer` transaction with the actor's origin, opened by the command
//! that enters insert mode. In insert mode keys aren't vim's: typed text
//! reaches the editor as ordinary input (`EditorModel::insert`) and joins
//! the transaction, and `vim.normal_mode` (bound to `Esc` and `ctrl-[`)
//! ends it. `.` replays the last change, and the text its session typed, as
//! one transaction.
//!
//! # Clean room
//!
//! This crate is written from Vim's documented behaviour, never from
//! source. Don't open Vim's or Neovim's source (C, Lua or Vimscript
//! runtime), hjkl's crates or corpus (any `hjkl-*`), Zed's `vim` crate or
//! Helix while working on it; see `CLEAN_ROOM.md`. Learn behaviour by
//! running `nvim --clean` (0.12.4) and from `:help`. The corpus under
//! `tests/corpus/` is the spec: when unsure, add a case and record what nvim
//! does with the ignored `record_corpus_with_neovim` test. Tables nvim
//! computes per code point are recorded the same way: `src/chars.txt`
//! (word classes and widths) and `src/case.txt` (what `~` does).
//!
//! # Cards
//!
//! M7 builds this crate in three cards:
//!
//! 1. The keys, the whole pending-input grammar, `vim.mode`, `vim.move` and
//!    its motions (`h j k l w b e W B E 0 ^ $ gg G f t F T`), and the Neovim
//!    oracle with its recorder.
//! 2. `vim.operate` with `d y > <`, text objects, `%`, `;` and `,`,
//!    registers, and `x X D p P`.
//! 3. This one: insert sessions as one transaction, `c C s S i a I A o O`,
//!    `J r ~`, `u <C-r>` and `.`.
//!
//! Later cards add the visual modes, macros, marks, search and ex, and the
//! GPUI wiring of `vim-engine.md` §10: key routing (the keymap first, then
//! [`step`] in normal mode, text input in insert mode), cursor shapes, the
//! `Esc` and `ctrl-[` bindings and the `vim.enabled` setting. That wiring
//! also ends a session when a command other than typing runs in insert mode
//! (cursor keys are macOS editor commands there), standing in for Vim's
//! `<C-g>u`, and types `Enter` in insert mode through
//! [`VimState::line_break`], as `HeadlessVim` does, so the indentation a
//! line break copies is 'autoindent'.
//!
//! # Known limits
//!
//! - Undo and redo put the carets back from fe-vim's own record of where
//!   each change began, as (row, byte column) pairs. Another origin's edits
//!   between a change and its undo can shift the rows, and then the carets
//!   land on the old rows, clamped to the text.
//! - `fe_buffer` drops a transaction that changed nothing, so a change that
//!   changed no text (`~` on punctuation, `<<` on an unindented row, a `c`
//!   over nothing with nothing typed, `3J` on the last row) leaves no undo
//!   step: `u` then undoes the change before it, where nvim undoes the
//!   no-op and only moves the caret.
//! - After undoing or redoing a `c` over several rows, nvim's caret goes to
//!   the second row at the column its motion reached; fe-vim uses the
//!   caret's own column, which differs when the motion moved it sideways.
//! - `vim.undo` and `vim.redo` step through the model's own history, the
//!   user's, whatever the actor.
//! - Typed `<CR>` and `<BS>` are the editor's (`EditorModel::newline` and
//!   `backspace`), not Vim's 'autoindent' and 'smarttab': a line break
//!   copies the row's indentation as it is (tabs and any other white space
//!   too) and keeps the blanks after the caret, and a backspace in an
//!   indent deletes one blank. On `Esc`, fe-vim drops the indentation the
//!   session made that went unused, as 'autoindent' does: what `o`, `O` or
//!   a linewise `c` opened a row with or kept, and what a line break typed
//!   in insert mode copied, on a row holding only white space, if nothing
//!   was typed on that row since it was made (typing deleted since counts;
//!   a backspace or a line break doesn't). Which edits are typed line
//!   breaks only the host knows: it types `Enter` in insert mode through
//!   [`VimState::line_break`], as `HeadlessVim`'s `<CR>` does. Any other
//!   input, `EditorModel::newline` called directly included, is typed text,
//!   whatever it holds, so a paste of a line break and blanks keeps its
//!   blanks. It follows those bytes through every edit
//!   while the session is open, so it never drops another origin's text or
//!   blanks that were already there, and it leaves indentation another
//!   origin changed (deleted some of, or wrote inside) as it is.
//! - A session's typing is its origin's edits, as its transaction is
//!   (`fe_buffer` groups edits by origin): another view of the buffer (a
//!   split) typing with the same origin while it's open types in it. What
//!   that view types is typed text to the session, line breaks included,
//!   unless typed through this state's [`VimState::line_break`] (or a
//!   clone's). The GPUI wiring can end a session when its editor loses
//!   focus.
//! - `fe_buffer` keeps one open transaction for the whole buffer. While a
//!   session's is open, a vim change through another view's state that
//!   opens its own (an insert session, `c` or `.`) for another origin
//!   nests into the session's (`fe_buffer` asserts against that in debug
//!   builds), so the two aren't undone apart. fe-vim can't see the open
//!   transaction's origin to refuse it.
//! - Another origin's undo or redo while a session is open closes the
//!   session's transaction (`fe_buffer` closes whichever transaction is
//!   open), so what's typed after it, and `Esc`'s cleanup, are undone
//!   apart from it.
//! - `.` and a count type the session's text again as the buffer holds it,
//!   not the keys: a line break in it brings the indentation it copied
//!   then, not the row's own; backspaces over text before the session's
//!   start aren't repeated; and another origin's text written among the
//!   typed characters is typed again with them, as the text `.` keeps is
//!   what lies between the session's start and the caret.
//! - `"{reg}.` ignores the register; a repeated change keeps its own.
//! - `r<CR>` cancels.
//! - A caret another origin's edit left inside a grapheme (or past a row's
//!   last one) is on that grapheme, as nvim's cursor is: `a`, `x`, `cw` and
//!   `p` act on all of it. `i` and `P` put text before it, where nvim,
//!   whose cursor only its API can leave there, puts it at that byte.
//! - Measuring text as the model catches up (`EditorModel`'s char widths)
//!   may stamp the context, or even end the session, through a clone of
//!   the state. A clone ending a session while another is ending it
//!   returns to normal mode without the session's outcome (its last change
//!   for `.`).

mod args;
mod commands;
mod edit;
mod headless;
mod history;
mod insert;
mod key;
mod motion;
mod operator;
mod reducer;
mod register;
mod repeat;
mod state;
mod text_object;

pub use args::{
    CountArgs, InsertArgs, InsertAt, Motion, MoveArgs, OperateArgs, Operator, PutArgs, RangeClass,
    ReplaceCharArgs, Target, TextObject,
};
pub use commands::{VimCommands, VimTarget, registry};
pub use headless::{FeedError, HeadlessVim};
pub use key::{KeyError, VimKey, parse_keys};
pub use reducer::{Pending, PendingKind, Step, step};
pub use register::{Register, RegisterError, Registers, Write};
pub use repeat::{Change, LastChange};
pub use state::{Mode, VimState, context};

#[cfg(test)]
mod tests;
