---
task_uid: "01m4pyvjzj9nwwn5ws2qezgy6r"
task: "T62"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/vim-remaps.md exists with Problem, Options (a keymap TOML table, a vimrc subset file, both), Recommendation and Questions for the user, covering modes, recursive maps, leader, multi-key ambiguity against fe-keymap chords and timeoutlen, where remaps run relative to VimState::feed and the feature hooks, counts and registers, dot-repeat, macros, :normal and :normal!, right sides that are not keys, hot reload and diagnostics, and corpus testing against nvim"
    required: true
    manual: false
  - id: "C2"
    text: "It ends with an outline of the implementation cards (files, sizes, how they stay off fe-vim's hub files), and the Not designed yet paragraph in docs/decisions/vim-engine.md section 10 points to it; no code changes"
    required: true
    manual: false
---

# Decide how vim key-to-key remaps (nnoremap and the like) are written, stored and run, in a decision record (Mac T425)

# Decide how vim key-to-key remaps (nnoremap and the like) are written, stored and run

## Problem

`docs/decisions/vim-engine.md` §10 (~764–766): "Not designed yet: vim-style key-to-key remaps
(`nnoremap H ^`). The keymap maps keys to commands, so they likely need a remap table inside
`fe-vim` (a later card)." T161 and T162 list them out of scope. nvim users bring remaps with them
(`nnoremap <leader>w :w<CR>`, `inoremap jk <Esc>`, `nnoremap Y y$`); without them fastedit's vim
mode can't be made to feel like their own. **This needs the user's decision before an
implementation card is drafted.**

## Change

Write `docs/decisions/vim-remaps.md` (new), a decision record with a recommendation, and replace
the "Not designed yet" paragraph in `vim-engine.md` §10 with a pointer to it. Read only fastedit's
code and docs and nvim's `:help` (`:help map-overview`, `:help :noremap`, `:help mapleader`,
`:help 'timeoutlen'`, `:help map-modes`); clean room as every M7 card.

The record covers, each with what fastedit does today (`crates/fe-keymap`, `crates/fe-vim/src/
reducer.rs`, `feature.rs`, `macros.rs`, `crates/fe-gpui/src/keymap.rs` routing keys to vim while
`vim.pending` is set):

1. **Where remaps are written** — options: (a) a table in fastedit's keymap TOML
   (`[[vim.remap]] mode = "n", from = "H", to = "^", recursive = false`); (b) a vimrc subset file
   (`~/.config/fastedit/vimrc`: `nnoremap`/`xnoremap`/`onoremap`/`inoremap`/`cnoremap`, `map`
   variants, `let mapleader`); (c) both, (b) compiled to (a). Hot reload and diagnostics for bad
   lines either way (M5's "invalid config keeps last good").
2. **Modes** n, x/v, o, i, c, and which come first.
3. **Recursive maps** (`map` vs `noremap`), with a recursion limit.
4. **Multi-key left sides and ambiguity**: `'timeoutlen'` against fe-keymap's chord engine (which
   already re-delivers keys for `jk` in insert mode), and who owns a key bound both ways.
5. **Where it runs**: a remap table in fe-vim in front of `step` (`VimState::feed`) vs in the
   keymap; how it meets the feature hooks, counts and registers typed before a mapped key, `.`,
   macros (nvim records typed keys, plays them through maps), `:normal` vs `:normal!`.
6. **Right sides that aren't keys**: `:w<CR>` (ex lines), `<Cmd>…<CR>`, fastedit command IDs
   (`<Cmd>workbench.save<CR>`?).
7. **Testing**: corpus cases against nvim with a mapping preamble (a new optional corpus field).

It ends with **## Recommendation** (one option, decisively) and **## Questions for the user**
(at least: (a), (b) or (c); recursive maps needed or `noremap` only; `<leader>` default), and an
outline of the implementation cards (files, sizes, how they stay off the hub files).

## Size

A docs card, about 200 lines of markdown.

## Out of scope

Implementing any of it; insert-mode `ctrl-` keys (§10's decision stands).
