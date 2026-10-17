---
task_uid: "01m55asd0dk07fsn9d724qyx3f"
task: "T156"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test virtual_replace_matches_neovim (top level, in crates/fe-vim/tests/virtual_replace_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/virtual_replace.toml (at least 16, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into virtual_replace.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists: typing over a tab (kept, then consumed), a typed tab, over a wide character, past the row's end, \u003cBS\u003e putting back, a count, . and u."
    required: true
    manual: false
  - id: "C3"
    text: "vim.mode is replace throughout a gR session, and R's sessions behave as before: the existing replace_matches_neovim test in crates/fe-vim/tests/replace_oracle.rs still passes unchanged."
    required: true
    manual: false
---

# fe-vim: gR types in virtual replace mode, each character replacing the screen columns it takes, tabs and wide characters included, as nvim does (Mac T803)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming `gR`,
virtual replace mode (`:help gR`, `:help vreplace-mode`): typed characters replace screen columns, not
characters. At Mac main 92c57c57 `g` then `R` cancels, so `gRxy<Esc>` runs `x`, `y`; lib.rs's replace-mode
paragraph says "Not here: … `gR`". Probed with `nvim --clean --headless` 0.12.6 (tabstop 4, 'expandtab'):

- On `a\tbc` (the tab covering columns 1-3) from col 0: `gRxy<Esc>` makes `xy\tbc` (the tab stays while
  it still reaches its stop), `gRxyzw<Esc>` makes `xyzwbc`, `gRxyzwv<Esc>` makes `xyzwvc`; from col 1
  `gRx<Esc>` makes `ax\tbc`.
- `gR<Tab>x<Esc>` on `abc` makes `    x` (the typed tab, as spaces, takes four columns).
- `gRab<Esc>` on `日本語` makes `ab本語` (two narrow characters take the wide one's two columns).
- Past the row's end it goes in: `gRxyz<Esc>` on `abc` from col 2 makes `abxyz`.
- `<BS>` puts back what was replaced: `gRxyz<BS><BS><Esc>` on `a\tbc` makes `x\tbc`, and on `abc` from
  col 1 makes `axc`.
- `2gRxy<Esc>` on `abc def` from col 1 makes `axyxyef`; `gRxy<Esc>j0.` on `abc def\nmno` makes
  `axy def\nxyo`; `u` undoes the session in one step.

## Change

`gR` is a flavour of `R`'s session (`crates/fe-vim/src/replace.rs`), not a new mode: `vim.mode` stays
`replace` (nvim's `mode()` says `Rv`), so the keymap's `editor && vim.mode == replace` bindings and the
GPUI host's routing through `VimState::replace_text`, `line_break` and `replace_backspace` need no change.

- `crates/fe-vim/src/reducer.rs`: `Prefix::G` takes `R` (no operator, not in a visual mode) as a new
  `vim.virtual_replace_mode {"count"}`, registered in `crates/fe-vim/src/commands.rs` like
  `vim.replace_mode`; `crates/fe-vim/src/repeat.rs` records it as its own change so `.` replays it
  virtually.
- `replace.rs`: in a virtual session each typed character (a tab as the spaces to its next stop) replaces
  the graphemes covering the screen columns it takes from the caret's column; a tab only partly covered
  stays (it still reaches its stop), and a wide grapheme partly covered becomes spaces for its columns
  left over; past the row's end it goes in. `<BS>` takes out the last character typed and puts back what
  it replaced, as `R`'s does. The count's copies, `.` and `u` work as for `R`. `Enter` in a virtual
  session stays `R`'s line break here; Mac T804 changes it.

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/virtual_replace.toml` (every
probed case above; no `<CR>`; no case ends in insert or replace mode) recorded into
`virtual_replace.expected.json` by the ignored `record_corpus_with_neovim` test in `tests/oracle.rs`, and a
new `crates/fe-vim/tests/virtual_replace_oracle.rs` modelled on `tests/replace_oracle.rs`.

reducer.rs and commands.rs belong to Mac T770 (landing in Mac host stack 24) and to Mac T800 (r<CR> and gi), which this
card depends on: sync a mac/main that carries stack 24 first. `crates/fe-vim/src/lib.rs`'s docs are out of
scope; the lead files one docs chore after these cards land.
