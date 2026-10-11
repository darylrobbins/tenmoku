---
task_uid: "01m4ewvg02eawwb36qeq62pegr"
task: "T227"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::increment::numbers: \u003cC-a\u003e on \"x 9 y\" from (0,0) gives \"x 10 y\" with the caret on 0; 5\u003cC-x\u003e on \"-3\" gives \"-8\"; on \"0xff\" \u003cC-a\u003e gives \"0x100\" and on \"0xFE\" \"0xFF\", keeping upper case; on \"0b101\" gives \"0b110\"; on \"007\" gives \"008\" (no octal); a row with no number changes nothing and leaves no undo step; 4294967295\u003cC-a\u003e on the largest 64-bit value behaves as nvim does (per the corpus) without panicking; in visual mode \u003cC-a\u003e on three rows changes each row's first number and g\u003cC-a\u003e adds 1, 2 and 3"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test increment_matches_neovim (tests/increment_oracle.rs): tests/corpus/increment.toml has at least 25 cases covering \u003cC-a\u003e and \u003cC-x\u003e on decimal, negative, hex in both cases, binary, a number after the cursor, none on the row, counts, crossing zero, the 64-bit edges, a word like x1, . after it, and visual \u003cC-a\u003e and g\u003cC-a\u003e; each matches its nvim 0.12.4 expectation; every earlier expected file is byte-identical and increment.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C3"
    text: "In src/tests/reducer.rs only the \u003cC-x\u003e of prefixes_targets_cancel_and_forward's forwarded keys changes, to a key fe-vim still forwards that T228 and T229 don't take; every earlier corpus and unit test passes with its files unchanged; neither src/visual.rs nor anything under crates/fe-vim/src/visual/ changes, nor src/lib.rs, src/args.rs, src/reducer.rs, src/commands.rs or src/tests/mod.rs; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim"
    required: true
    manual: false
---

# fe-vim: ctrl-a and ctrl-x on decimal, hex and binary numbers, in normal and visual mode, with g ctrl-a

# fe-vim: ctrl-a and ctrl-x on numbers, in normal and visual mode (g ctrl-a too)

Milestone M7 "Vim", second batch (resized), card 09b: the increment third of the old card 9. Read
cards 1 and 09a. Design: `docs/decisions/vim-engine.md` §10 ("Ctrl keys in normal and visual mode:
vim wins … `ctrl-a`").

Assumes card 00 (the `visual.rs` split) has landed. The selected rows come from `VimState`'s public
`visual_anchor` and `visual_cursor` and its mode's `VisualKind`, worked out in `src/increment.rs`, and
the mode ends through the existing `visual::exit`: neither `src/visual.rs` nor anything under
`src/visual/` changes.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help CTRL-A`, `:help 'nrformats'`,
`:help v_g_CTRL-A`.

## Where it registers (after the feature-module refactor this task depends on)

Everything below lives in `src/increment.rs`, the stub the refactor declared and wired in
(`pub mod increment`, `increment::FEATURE` in `feature::FEATURES`):

- `FEATURE.key` takes `<C-a>` and `<C-x>` in normal and visual mode (with a count, no operator);
- `FEATURE.g` takes `<C-a>` and `<C-x>` after `g` in visual mode;
- `FEATURE.register` registers `vim.increment`; its args type is `pub` in `fe_vim::increment`;
- the handler catches the marks up with `commands::follow` and closes a session with
  `insert::adopt_closing`, as the text-editing commands in `commands.rs` do.

`.` is `src/repeat.rs`'s: `Change` gains an `Increment` variant (its `invocation`, `counted` and
`replay` arms). `src/lib.rs`, `args.rs`, `reducer.rs`, `commands.rs` and `src/tests/mod.rs` don't
change; `src/increment.rs`'s module docs describe the keys for hosts.

## Change

- `<C-a>` and `<C-x>` commit `vim.increment {"delta", "count"}`: the first number at or after the
  cursor on the row changes by `count`; decimal with a leading `-`, hex `0x`/`0X` (keeping the
  letter case), binary `0b`/`0B` (nvim's default `nrformats=bin,hex`; octal off). The caret ends on
  its last digit. Arithmetic saturates or wraps as nvim does at the 64-bit edges (record it). One
  transaction; `.` repeats it. A row with no number changes nothing and leaves no undo step.
- In visual mode, `<C-a>`/`<C-x>` change the first number on each selected row, and `g<C-a>`/
  `g<C-x>` a progressive count (`:help v_g_CTRL-A`); the mode then ends as nvim's does.

### Corpus

`tests/corpus/increment.toml`, at least 25 cases: `<C-a>` and `<C-x>` on decimal, negative, hex
(upper and lower case), binary, a number after the cursor, none on the row, counts, crossing zero,
the 64-bit edges, a word like `x1`, `.` after it, and visual `<C-a>` and `g<C-a>`.

## Tests (new files)

- `src/tests/increment.rs` (the refactor's stub, already declared in `src/tests/mod.rs`).
- `tests/increment_oracle.rs` (`#[allow(dead_code)] mod support;`) with `increment_matches_neovim`.

## Existing tests

`tests::reducer::prefixes_targets_cancel_and_forward` lists `<C-x>` among keys forwarded with
nothing pending; replace it with another key fe-vim still forwards and that T228 and T229 don't
take (not `z` or `<C-d> <C-u> <C-e> <C-y> <C-f> <C-b>`), such as `<C-g>`.

## Size

About 250 non-test lines.

## Out of scope

Octal and alpha `nrformats`, the GPUI check (11). Any change outside `crates/fe-vim`.
