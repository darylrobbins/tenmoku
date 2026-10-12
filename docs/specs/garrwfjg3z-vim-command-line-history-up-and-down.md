---
task_uid: "01m4p2z9091gc28jgarrwfjg3z"
task: "T422"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test in tests/history_oracle.rs: Registers keep a search history shared by / and ? and an ex history, newest last, a repeated entry moved to the end, at most 10000 entries, cancelled and empty lines left out; Registers::recall gives the next older or newer entry starting with the typed prefix"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test history_matches_neovim (tests/history_oracle.rs): tests/corpus/history.toml has at least 20 cases (/ then \u003cUp\u003e and \u003cDown\u003e chains, prefix recall, \u003cDown\u003e back to the typed text, ? sharing the / history, a repeated search, : history, an offset recalled with its pattern) run through HeadlessVim's line; each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench tests in src/vim_line/history_tests.rs: Up and Down in the open vim line replace its text with the recalled entry, caret at the end, and Down past the newest restores what was typed"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim and fe-workbench test passes, with two allowed edits the new behaviour requires: tests::cmdline::line_mode_and_routing checks a forwarded key with \u003cLeft\u003e instead of \u003cUp\u003e, and tests::substitute::flags_specials_and_bounds expects the failing :s line in the ex history"
    required: true
    manual: false
---

# Vim command-line history: up and down recall earlier / ? and : lines by prefix, in fe-vim and the workbench line

# Vim command-line history: up and down recall earlier / ? and : lines

## Problem

"Search history … isn't supported" (`crates/fe-vim/src/lib.rs` Known limits ~561), and the `:` line
has none either (`lib.rs` ~354 lists "the line's history and completion" as unsupported). The
workbench's vim line (`crates/fe-workbench/src/vim_line.rs`) has no up/down handling. Gap analysis
2026-10-11.

## Change

### fe-vim

- **`src/register.rs`**: `Registers` (shared by every editor, as nvim's history is global) keeps
  two histories: search (`/` and `?` share it) and ex. Submitting a line adds it, newest last; an
  identical earlier entry is removed; at most 10000 entries (nvim's 'history' default); an empty
  line or a cancelled one adds nothing. `Registers::recall(kind, typed, from, older) -> Option<(usize,
  &str)>` gives the next older/newer entry starting with `typed` (nvim's prefix recall).
- **`src/cmdline.rs`**: `submit` adds the line's text to its history before it runs (a failing
  line still goes in, as in nvim).
- **`src/headless.rs`**: `HeadlessVim`'s line (`line_key`) takes `<Up>` and `<Down>` as nvim's
  command line does: the first `<Up>` remembers what was typed, each replaces the line with the
  next older/newer entry starting with it, and `<Down>` past the newest puts the typed text back.
  So the oracle checks it.

### fe-workbench

- **`vim_line.rs`**: Up and Down in the open vim line do the same through fe-vim's
  `Registers::recall`, replacing the input's text and putting the caret at its end.

**Clean room**: `nvim --clean` (0.12.4) and `:help cmdline-history`, `:help c_<Up>`; never source,
hjkl, Zed or Helix.

### Corpus

`crates/fe-vim/tests/corpus/history.toml`, at least 20 cases: `/a<CR>/b<CR>/<Up><CR>`,
`/<Up><Up><CR>`, prefix recall (`/f<Up>`), `<Down>` back to the typed text, `?` sharing `/`'s
history, a repeated search moved to newest, `:` history (`:s/x/y/<CR>…:<Up><CR>`), offsets
recalled with their pattern.

## Tests (new files)

- `crates/fe-vim/tests/history_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `history_matches_neovim`, and a unit test there of `Registers::recall` (dedupe, cap, prefix).
- `crates/fe-workbench/src/vim_line/history_tests.rs` (declared `#[cfg(test)] mod history_tests;`
  from `vim_line.rs`): Up and Down in the open line recall entries.

## Size

About 200 non-test lines.

## Out of scope

Persisting history across runs (ShaDa), `q:`/`q/` windows, `<C-p>`/`<C-n>`/`<S-Up>`, completion,
the expression and input histories, the GPUI check.

## Conflicts

`vim_line.rs` is also in T231's and T357's scopes (both inbox): whichever is readied first goes
first. Depends on the search-offsets card (`register.rs`, `cmdline.rs`) and T367 (`headless.rs`).
