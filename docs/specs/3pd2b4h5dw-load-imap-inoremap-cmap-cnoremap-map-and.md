---
task_uid: "01m5n8y5246v4ept3pd2b4h5dw"
task: "T206"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "parse_vimrc loads imap/inoremap/iunmap/imapclear (Insert), cmap/cnoremap/cunmap/cmapclear (CmdLine) and map!/noremap!/unmap!/mapclear! (Insert+CmdLine) as Remap records with no issue; nmap! is still 'not supported: nmap!'; plain map/noremap stay n+x+o; the 'insert and command-line remaps aren't supported yet' message is gone"
    required: true
    manual: false
  - id: "C2"
    text: "vim.remap mode accepts \"i\" and \"c\" alone or in a list: VimRemapMode has Insert and CmdLine, settings.schema.json lists them, and the settings_schema() test that compares the checked-in schema passes"
    required: true
    manual: false
  - id: "C3"
    text: "fastedit's remap_modes maps the new variants, and a [[vim.remap]] with mode = \"i\" produces a set whose lookup(RemapMode::Insert, ..) finds it (test in crates/fe-config/tests/vim_remap_insert_mode.rs and/or the vim_remaps.rs test module)"
    required: true
    manual: false
  - id: "C4"
    text: "New corpus area tests/corpus/remaps_insert.toml with recorded remaps_insert.expected.json (pinned nvim) and tests/remaps_insert_oracle.rs, whose fe-vim replay matches nvim, covers: inoremap jk \u003cEsc\u003e ending in normal mode, an expansion, imap chain vs inoremap, failed prefix, replace mode, nmap ab abx + imap b B typed ab, :normal and :normal! with an imap, cnoremap jk \u003cCR\u003e typed :s/a/b/jk, a map!/unmap! pair"
    required: true
    manual: false
  - id: "C5"
    text: "Existing tests updated only where the parser change forces it (remap_vimrc.rs vimrc_reports_problems); cargo test -p fe-vim -p fe-config and cargo test -p fastedit --bins pass"
    required: true
    manual: false
---

# Load imap, inoremap, cmap, cnoremap, map! and noremap! from the vimrc and settings (mode i/c), with insert and cmdline oracle corpus cases (T877 B) (Mac T1090)

## Problem

T877 split, card B. After card A the engine runs `i` and `c` maps, but nothing
loads them: `parse_vimrc` reports `imap`, `inoremap`, `cmap`, `cnoremap`, `map!`,
`noremap!` and their unmaps/mapclears as "insert and command-line remaps aren't
supported yet", and `[[vim.remap]]`'s `mode` accepts only n/x/v/o. The oracle
corpus builds its fe-vim side from `mapping` lines through `parse_vimrc`
(`tests/support/mod.rs` `check`), so the insert and cmdline corpus cases can only
land together with the parser change.

## Change

- `crates/fe-vim/src/vimrc.rs`: `imap`/`inoremap`/`iunmap`/`imapclear` map to
  `RemapMode::Insert`; `cmap`/`cnoremap`/`cunmap`/`cmapclear` to `CmdLine`;
  `map!`/`noremap!`/`unmap!`/`mapclear!` to `[Insert, CmdLine]` (replace
  `command_bang`/`INSERT_CMDLINE`; `nmap!` stays "not supported: nmap!"). Plain
  `map`/`noremap` stay n+x+o as nvim has them.
- `crates/fe-config/src/settings.rs` + `settings.schema.json` (+ the commented
  example in `settings-template.toml`): `VimRemapMode` gains `"i"` (insert and
  replace) and `"c"` (command line). Keep the edit to the enum, its doc comments and
  the schema block (other cards edit nearby lines of these files).
- `crates/fastedit/src/vim_remaps.rs`: `remap_modes` (an exhaustive match on
  `VimRemapMode`) maps the new variants.
- Update the existing assertions the parser change breaks:
  `crates/fe-vim/tests/remap_vimrc.rs` (`vimrc_reports_problems`: lines 2-6 load
  instead of giving issues, and the `imap c d` bar test at the end), and
  `crates/fe-config/tests/vim_remap_setting.rs` if it lists the modes. New cases go
  in NEW files `crates/fe-vim/tests/remap_vimrc_insert.rs` and
  `crates/fe-config/tests/vim_remap_insert_mode.rs`.
- Corpus, NEW area files (areas are discovered by `support::recorded_corpora`; the
  existing `remaps.toml`/`remaps.expected.json` stay untouched):
  `tests/corpus/remaps_insert.toml` + `remaps_insert.expected.json` +
  `tests/remaps_insert_oracle.rs` (shape of `tests/remaps_oracle.rs`, asserting the
  named cases exist). Cases: `inoremap jk <Esc>` typed `ijkx` (ends in normal mode,
  as the oracle cannot end in insert mode); an expansion (`inoremap kj hello`);
  `imap` chain vs `inoremap`; failed prefix (`inoremap jk X`, typed `ijx`); replace
  mode (`R` with an imap); `nmap ab abx` + `imap b B` typed `ab`; `:normal` and
  `:normal!` with an imap; `cnoremap jk <CR>` typed `:s/a/b/jk`; a `map!`/`unmap!`
  pair. Record the JSON with the pinned nvim (`NVIM` in `tests/support/mod.rs`) via
  the ignored `record_corpus_with_neovim` in `tests/oracle.rs`.

## Notes

- Needs card A merged (the `RemapMode` variants and the engine).
- Recording `*.expected.json` needs headless nvim at the pinned version. If the
  Linux host has none, record on the Mac and hand the JSON in; the replay test
  (`remaps_insert_oracle`) needs no nvim.
