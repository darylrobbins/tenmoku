//! Vim mode for fastedit's editor, headless. Must never depend on GPUI.
//!
//! Keys go through a pure reducer, [`step`], which keeps the whole
//! incomplete command (count, `"{reg}`, operator, a prefix such as `g` or
//! `f`) in one [`Pending`] value and never sees the buffer. A complete
//! command comes out as an [`Invocation`](fe_command::Invocation) of
//! `vim.move` or `vim.operate`, dispatched through [`registry`] against a
//! [`VimTarget`]. [`VimState`] holds the pending value, the goal columns and
//! the last character search, and publishes `vim.mode` (plus `vim.pending`
//! and `vim.pending_kind` while a command is incomplete) on the editor's own
//! context frame, so keymap bindings can be scoped to a mode. With vim off
//! nothing stamps the frame and `vim.mode` is never set. The design is
//! `docs/decisions/vim-engine.md` (§1, §9 and §10).
//!
//! # Clean room
//!
//! This crate is written from Vim's documented behaviour, never from
//! source. Don't open Vim's or Neovim's source (C, Lua or Vimscript
//! runtime), hjkl's crates or corpus (any `hjkl-*`), Zed's `vim` crate or
//! Helix while working on it; see `CLEAN_ROOM.md`. Learn behaviour by
//! running `nvim --clean` (0.12.4) and from `:help`. The corpus under
//! `tests/corpus/` is the spec: when unsure, add a case and record what nvim
//! does with the ignored `record_corpus_with_neovim` test.
//!
//! # Cards
//!
//! M7 builds this crate in three cards:
//!
//! 1. This one: the keys, the whole pending-input grammar, `vim.mode`,
//!    `vim.move` and its motions (`h j k l w b e W B E 0 ^ $ gg G f t F T`),
//!    and the Neovim oracle with its recorder.
//! 2. `vim.operate` with `d y > <`, text objects, `%`, `;` and `,`,
//!    registers, and `x X D p P`.
//! 3. Insert sessions as one transaction, `c C s S i a I A o O`, `J r ~`,
//!    `u <C-r>` and `.`.
//!
//! Visual modes, macros, marks, search and ex come later, as does the GPUI
//! wiring (key routing, cursor shapes and the `vim.enabled` setting).

mod args;
mod commands;
mod headless;
mod key;
mod motion;
mod reducer;
mod state;

pub use args::{Motion, MoveArgs, OperateArgs, Operator, Target, TextObject};
pub use commands::{VimCommands, VimTarget, registry};
pub use headless::{FeedError, HeadlessVim};
pub use key::{KeyError, VimKey, parse_keys};
pub use reducer::{Pending, PendingKind, Step, step};
pub use state::{Mode, VimState, context};

#[cfg(test)]
mod tests;
