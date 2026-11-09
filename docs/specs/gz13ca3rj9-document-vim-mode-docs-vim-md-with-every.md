---
task_uid: "01m70cf8rc0qs2cbgz13ca3rj9"
task: "T338"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/vim.md has sections Enabling, Settings, Modes and editing, Ex commands (with Unsupported ex commands), Emulated plugins, Keymaps, Differences from Neovim"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config --test vim_doc passes: every [vim] key of Settings::default() serialised is a row in the doc's settings table with the same default, and no row names a non-vim key"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim --test ex_unsupported_doc passes: each name in the Unsupported ex commands list gives E492 or the documented refusal through the public ex entry, and each listed supported command parses"
    required: true
    manual: false
  - id: "C4"
    text: "Emulated plugins lists only features found under crates/fe-vim/src/feature/ and crates/fe-vim/src/*.rs, each with its keys, its setting if any and the test file that covers it; easymotion, targets and anything not found by rg are stated as not implemented; T910 is not described as shipped"
    required: true
    manual: false
  - id: "C5"
    text: "Keymaps and Differences sections cite the files they come from (crates/fe-vim/src/vimrc.rs, remap.rs, docs/decisions/vim-remaps.md; the *_oracle.rs tests and the nvim 0.12 recording stamp), with no uncited behaviour claim"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy -p fe-config -p fe-vim --all-targets -- -D warnings passes; only the three scoped files change"
    required: true
    manual: false
---

# Document vim mode: docs/vim.md with every [vim] setting and default, modes, motions, operators, ex commands, emulated plugins, keymaps and differences from Neovim, checked by tests (Mac T1653)

## Problem

Vim mode (fe-vim) has the most settings and the most behaviour of any feature and no user
documentation: only comments in crates/fe-config/settings-template.toml and decision records
(docs/decisions/vim-engine.md, vim-remaps.md). Users cannot tell what is supported, which "plugins"
are emulated, or how Tenmoku differs from Neovim.

## Change

Add docs/vim.md, written from the code, not from memory of Vim. If it passes about 600 lines, stop and
ask the lead to respec into docs/vim/*.md rather than widening scope. Sections:

1. **Enabling**: `[vim] enabled` (default false) and the command that toggles it, if any (find it in
   fe-workbench), status-bar mode pill, Start page keys.
2. **Settings**: a table with every key of `VimSettings` (crates/fe-config/src/settings.rs) plus
   `[[vim.remap]]` and `[[vim.which_key_group]]`: key, type or values, default, one sentence. Keys
   today include enabled, clipboard, jump_on_s, jump_style, ignorecase, smartcase, incsearch,
   inccommand, replace_with_register, subword_motions, regex_time_limit, shell, filter_time_limit,
   which_key, which_key_delay_ms, highlight_yank, highlight_yank_ms, quick_scope, leader, preset;
   take the real list from the struct, which wins over this list.
3. **Modes and editing**: normal, insert, replace and virtual replace, visual (char, line, block),
   operator-pending, command line; motions, operators, text objects, registers (named, numbered,
   read-only, clipboard), marks and jumplist and changelist, macros, dot repeat, counts, folds, scroll
   keys, search and `:s` (flags, confirm, inccommand), `:g`, ranges.
4. **Ex commands**: the supported set generated from or checked against the `COMMANDS` table
   (crates/fe-vim/src/ex/table.rs) and the names ex.rs and lines.rs take; and an **Unsupported ex
   commands** list (see criteria).
5. **Emulated plugins**: only what the code implements, found under crates/fe-vim/src/feature/ and
   named by their tests: surround (surround.rs), commentary (comment.rs), ReplaceWithRegister
   (replace_register.rs, behind replace_with_register), exchange (exchange.rs), abolish
   (abolish.rs), argument objects, subword motions (vim-wordmotion, behind subword_motions), matchit
   (matchit/), quick-scope (quick_scope setting), leap/flash-style jump (jump_on_s, jump_style),
   sections, definition motions and objects, seek objects, which-key. Each with its keys and its
   setting if any. Say plainly that easymotion, targets.vim and others are not implemented unless
   `rg` finds them; T910 was abandoned and must not be described as shipped.
6. **Keymaps**: `[[vim.remap]]` (modes n, x or v, o, i, c; recursive), the vimrc subset
   (crates/fe-vim/src/vimrc.rs: map, noremap, unmap, mapclear, `let mapleader`, `:fegroup`, and what is
   reported as an issue), where the vimrc lives (crates/fe-config/src/paths.rs), `leader`,
   `preset = "lazyvim"`, which-key groups, and how vim keys relate to the editor keymap
   (crates/fe-keymap, keys the editor keeps in vim mode).
7. **Differences from Neovim**: from the fe-vim oracle tests (crates/fe-vim/tests/*_oracle.rs and
   corpus/*.toml, recorded against nvim 0.12): the version, what is matched byte for byte, the
   regex engine and its time limit, and every deliberate divergence you find in the decision
   records and in module docs (cite the file); no guesses.

## Checks that keep it true

- crates/fe-config/tests/vim_doc.rs (new): reads ../../docs/vim.md and fails if any `[vim]` key of
  `Settings::default()` serialised (`toml::Table::try_from`, as tests/template.rs does) is missing from the
  doc's settings table, if a table row's default differs from the serialised default, or if a table
  row names a key that is not a `[vim]` key.
- crates/fe-vim/tests/ex_unsupported_doc.rs (new): reads docs/vim.md, takes the names in the
  "Unsupported ex commands" list (one backticked name per line), and asserts running each through the
  public ex entry (as tests/ex_day_one.rs does) gives E492 (not an editor command) or the documented
  refusal; also asserts the supported-command list names only commands that parse.
- Both tests read the file at CARGO_MANIFEST_DIR/../../docs/vim.md.

## Out of scope

Code changes (a gap the doc finds is a note in the "Differences" section or a follow-up card, not a fix
here), README (the README card links this file), screenshots.

## Implementation notes

- docs/vim.md is 520 lines, under the 600-line limit, so it stays one file.
- crates/fe-vim/src/lib.rs's "Known limits" and pattern sections are partly out of date: uppercase
  marks, `'[ ']`, `'.`, `'^`, `@:`, `Q`, `r<CR>`, `\zs`, `\n` and multi-row `:s` all work now. The
  doc takes these facts from the module docs and tests (marks.rs, macros.rs, search.rs, the oracle
  tests) and from probing through `HeadlessVim`, not from lib.rs. Updating lib.rs is a follow-up;
  this card doesn't change code.
- ReplaceWithRegister is on this trunk as Mac T910 (argon T163, commit 29ec8bf6), behind
  `vim.replace_with_register`. The doc describes it from replace_register.rs and its test and
  never mentions "T910".
- vim_doc.rs reads each default in the table as a TOML value, because `toml` serialises the
  leader's `"\\"` as `'\'`.
- The supported-commands check runs every `:name[rest]` in the Ex commands tables through
  `HeadlessVim`, both short and full, and fails on E492. The unsupported list's
  `— refused: \`…\`` suffix gives the message prefix to expect in place of E492.
