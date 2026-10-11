---
task_uid: "01m4ewv2s2acpz78phabc3c6e8"
task: "T222"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::clipboard::unnamedplus: with set_clipboard_unnamed(true) on HeadlessVim, yy writes the line and its line break to HeadlessHost's clipboard and to \" and 0; dw writes the word to the clipboard and -; \"ayy leaves the clipboard unchanged; p with no register puts the clipboard's text, linewise when it ends in a line break; \"_dd leaves the clipboard unchanged; with set_clipboard_unnamed(false) yy leaves the clipboard unchanged, as before"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::clipboard::kinds_round_trip: a charwise yank ending in a line break (v$y on \"ab\\ncd\" yanks \"ab\\n\") pasted from the clipboard with \"+p stays charwise because the clipboard text equals what fe-vim last wrote; a blockwise yank of width 2 pasted with \"+p pastes as a block; other text ending in a line break, set on the clipboard by another writer (HeadlessHost.clipboard assigned directly), pastes linewise; reading an empty clipboard puts nothing"
    required: true
    manual: false
  - id: "C3"
    text: "Every earlier fe-vim corpus and unit test passes with its files unchanged, except one case in tests::visual_block::operators_registers_and_put: its last block (a block yanked with \"+y, then \"+P) is updated to expect the block put back as a block, as C2 requires, and its comment no longer says the clipboard never reads back as a block. No other line of visual_block.rs changes. scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted)"
    required: true
    manual: false
---

# fe-vim: clipboard=unnamedplus in the registers (yanks, deletes and puts through the clipboard), and a yank's kind kept across it

# fe-vim: clipboard=unnamedplus in the registers, and a yank's kind kept across the clipboard

Milestone M7 "Vim", second batch (resized), card 07a: the fe-vim half of the old card 7 (split so the
fe-vim chain doesn't wait on the GPUI cards; 07b adds the setting and the editor wiring). Read
T155's spec (registers, `"+`/`"*` through `EditorHost`) and T174's (blockwise registers). Design:
`docs/decisions/vim-engine.md` §1 ("`+`/`*` through the host clipboard"), §4 (Registers), §9 item 3.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help clipboard`,
`:help clipboard-unnamedplus`, `:help quoteplus`.

## What exists

`"+` and `"*` are one slot read and written through `EditorHost::read_clipboard` /
`write_clipboard`. A clipboard read counts as linewise when it ends in a line break; a blockwise
yank's kind is lost through the clipboard.

## Change (`crates/fe-vim/src/register.rs`)

- `Registers::set_clipboard_unnamed(bool)`. When on, a yank, delete or change that names no register
  writes the clipboard (as `"+` does) besides `"` and the numbered/`-` rules
  (`:help clipboard-unnamedplus`), and `p`/`P`/visual `p` with no register read the clipboard. `"0`
  is still written by a yank, as nvim does. `"_` never touches it. Off by default: behaviour is as
  today.
- **Kind across the round trip.** The registers remember the last text they wrote to the clipboard
  with its kind (charwise, linewise, blockwise and width). A clipboard read whose text equals it
  keeps that kind; any other text (copied in another app) is linewise only if it ends in a line
  break, as today.
- No Neovim oracle: the oracle disables nvim's clipboard provider (T154). Unit tests against
  `HeadlessHost`'s in-memory clipboard, following `:help` and a hand-run
  `nvim --clean -c 'set clipboard=unnamedplus'` with pbcopy, cited in the test. `HeadlessVim` gains
  `set_clipboard_unnamed(bool)` passing through, if it isn't reachable already.

## Tests (new files)

`crates/fe-vim/src/tests/clipboard.rs`, declared in `crates/fe-vim/src/tests/mod.rs`.

## Size

About 120 non-test lines.

## Out of scope

The setting and the editor (07b); `clipboard=unnamed` as distinct from `unnamedplus`; rich
clipboard metadata shared with other apps.
