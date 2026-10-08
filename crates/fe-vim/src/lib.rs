//! Vim mode for fastedit's editor, headless. Must never depend on GPUI.
//!
//! Keys go through a pure reducer, [`step`], which keeps the whole
//! incomplete command (count, `"{reg}`, operator, a prefix such as `g`, `f`
//! or `r`) in one [`Pending`] value and never sees the buffer. A complete
//! command comes out as an [`Invocation`](fe_command::Invocation) of one of
//! vim's commands (`vim.move`, `vim.operate`, `vim.put`, `vim.insert`,
//! `vim.join`, `vim.replace_char`, `vim.toggle_case`, `vim.undo`, `vim.redo`,
//! `vim.repeat`, `vim.visual`, `vim.visual_exit`, `vim.visual_swap`,
//! `vim.reselect` and `vim.select`), dispatched through [`registry`] against
//! a [`VimTarget`], which carries the host's shared [`Registers`].
//! [`VimState`] holds the mode, the pending value, the goal columns, the
//! last character search, the open insert session, the visual selection and
//! the last one (`gv`, `'<` and `'>`), the last change for `.` and the
//! carets before each change for `u`, and publishes `vim.mode` (plus
//! `vim.pending` and `vim.pending_kind` while a command is incomplete, and
//! the flag `vim.visual` in a visual mode) on the editor's own context
//! frame, so keymap bindings can be scoped to a mode. With vim off nothing
//! stamps the frame and `vim.mode` is never set. The design is
//! `docs/decisions/vim-engine.md` (§1, §4, §9 and §10).
//!
//! An insert session (`i a I A o O`, or the change operator `c`) is one
//! `fe_buffer` transaction with the actor's origin, opened by the command
//! that enters insert mode. In insert mode keys aren't vim's: typed text
//! reaches the editor as ordinary input (`EditorModel::insert`) and joins
//! the transaction, and `vim.normal_mode` (bound to `Esc` and `ctrl-[`)
//! ends it. `.` replays the last change, and the text its session typed, as
//! one transaction.
//!
//! In the visual modes (`v` charwise, `V` linewise; `vim.mode` is `visual`
//! or `visual_line`) keys still go to [`step`], which [`VimState`] tells the
//! mode is on, so an operator acts on the selection at once
//! (`Target::Selection`, or `Target::SelectedLines` for `X D Y C S R`), as do
//! `~ u U J r{char}`, and `i`/`a` select text objects (`vim.select`). The vim
//! cursor and the anchor are anchors on grapheme starts (or a row's end,
//! after `$`), and the model's one selection mirrors them, taking the
//! grapheme under the cursor, so the editor paints and copies what vim
//! would take; hosts paint the block cursor at
//! [`VimState::visual_cursor`] and drive a mouse drag through
//! [`VimState::select_visual`]. Leaving a visual mode by any path keeps its
//! extent for `gv` and as `'<` and `'>` ([`VimState::mark`]). `.` repeats a
//! change over a selection on as much text from the caret.
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
//! (word classes and widths), `src/case.txt` (what `~` does), and
//! `src/lower.txt` and `src/upper.txt` (what visual `u` and `U` do).
//!
//! # Cards
//!
//! M7 built this crate in three cards:
//!
//! 1. The keys, the whole pending-input grammar, `vim.mode`, `vim.move` and
//!    its motions (`h j k l w b e W B E 0 ^ $ gg G f t F T`), and the Neovim
//!    oracle with its recorder.
//! 2. `vim.operate` with `d y > <`, text objects, `%`, `;` and `,`,
//!    registers, and `x X D p P`.
//! 3. Insert sessions as one transaction, `c C s S i a I A o O`, `J r ~`,
//!    `u <C-r>` and `.`.
//!
//! The GPUI wiring of `vim-engine.md` §10 came next: key routing (the
//! keymap first, then [`step`] in normal mode, text input in insert mode),
//! cursor shapes, the `Esc` and `ctrl-[` bindings and the `vim.enabled`
//! setting. It also ends a session when a command other than typing runs in
//! insert mode ([`VimState::split_session`], standing in for Vim's
//! `<C-g>u`), and types `Enter` in insert mode through
//! [`VimState::line_break`], as `HeadlessVim` does, so the indentation a
//! line break copies is 'autoindent'.
//!
//! The second batch starts with this card, charwise and linewise visual
//! mode: `v V`, operators over the selection, `o`, `gv`, `'<` and `'>`, and
//! `.` after a visual change. Later cards add blockwise visual mode (`<C-v>`)
//! with `p` and `P` over a selection and blockwise registers; search; ex
//! commands (`:'<,'>` among them); the command line; the clipboard; marks
//! and macros; replace mode, increment and folds; scrolling; and the GPUI
//! wiring of the visual modes (their key bindings, painting the selection
//! and the block cursor, mouse drags and the status bar).
//!
//! # Known limits
//!
//! - `'<` and `'>` keep a byte column on a row anchored at its start, as
//!   nvim's marks do: rows put in or taken out above move them, edits
//!   within their row don't. nvim also moves marks on rows that a change
//!   joins or replaces; fe-vim does so for the changes over a selection
//!   (their own `'<` and `'>`, or through `.` earlier ones): rows a
//!   charwise `d`, `c` or `J` joins to the first, rows a linewise `c`
//!   replaced, and rows deleted at the end of the text, where the marks
//!   stay past the new end as nvim's do. Other joins (`J`, `x` at a row's
//!   end in normal mode) leave a mark on the joined row at its old column,
//!   and text with line breaks put in at the start of a mark's row moves
//!   the mark down, as a row opened above does, where nvim keeps it for a
//!   charwise put.
//! - A count before `.` after a visual change replaces the operator's count
//!   (how many times `>` and `<` shift), not the amount of text.
//! - In a visual mode, `aw` growing backwards onto a word after the row's
//!   indentation takes all of the indentation.
//! - After `.` of a change over a selection with `>` or `<`, the caret goes
//!   to the column the repeated selection ended at, which can differ from
//!   nvim's past wide characters and tabs.
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
//!   returns to normal mode, and adopts the session's outcome (its last
//!   change for `.` and its carets record) before its next `.`, `u` or
//!   `<C-r>`.

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
mod visual;

pub use args::{
    CountArgs, InsertArgs, InsertAt, Motion, MoveArgs, OperateArgs, Operator, PutArgs, RangeClass,
    ReplaceCharArgs, SelectArgs, Target, TextObject, VisualArgs, VisualKind,
};
pub use commands::{VimCommands, VimTarget, registry};
pub use headless::{FeedError, HeadlessVim};
pub use insert::Splits;
pub use key::{KeyError, VimKey, parse_keys};
pub use reducer::{Pending, PendingKind, Step, step};
pub use register::{Register, RegisterError, Registers, Write};
pub use repeat::{Change, LastChange};
pub use state::{Mode, VimState, context};
pub use visual::Mark;

#[cfg(test)]
mod tests;
