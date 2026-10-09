---
task_uid: "01m4ewtpp47zhv0f2m4xx6xchg"
task: "T216"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::ex::entering_parsing_and_running: : commits vim.command_line {\"kind\":\"ex\"}; 3: prefills .,.+2 and : from visual mode prefills '\u003c,'\u003e and sets the \u003c and \u003e marks; parsing gives the documented Ex value for :5, :$, :.,+2d, :3;/x/d, :'\u003c,'\u003ed, :d a 3 and :noh; :.,+2d is one transaction undone by one u and . after it repeats the change before it; :noh clears VimState::highlight() until the next search; :frobnicate is E492 in VimState::message() and changes nothing; a line past the end is E16"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test ex_matches_neovim (tests/ex_oracle.rs): tests/corpus/ex.toml has at least 30 cases covering :5 :$ :+2 :-1, :'\u003c,'\u003e after vj, :%, :/pat/ and :?pat?, :.,+2d, :3,4d a, :d 3, ; versus , ranges, an empty range, a line past the end (last key), a backwards range, :noh and an unknown command (last key); each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers, and ex.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test loader_allows_plus_and_star_in_ex_ranges (tests/ex_oracle.rs): the corpus loader accepts :.,+2d\u003cCR\u003e and :s/a*/b/\u003cCR\u003e, and still rejects \"+p, \"*yy, :d +\u003cCR\u003e, :d *\u003cCR\u003e, :pu +\u003cCR\u003e and a \u003cC-r\u003e+ in a : line; corpus_loader_rejects_bad_cases passes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "In src/tests/reducer.rs only the : of prefixes_targets_cancel_and_forward's forwarded keys changes, to a key fe-vim still forwards; every earlier corpus and unit test passes with its files unchanged; nothing under crates/fe-vim/src/visual/ changes; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs list the ex commands and ranges and what is out of scope"
    required: true
    manual: false
---

# fe-vim: the : command line with ranges (N . $ % '< '> /pat/ offsets ; ,), :{line}, :d and :noh

# fe-vim: the : command line with ranges, :{line}, :d and :noh

Milestone M7 "Vim", second batch (resized), card 05a: the first part of the old card 5 (ex). Read
04b first (the command-line mode, `vim.command_line`, `_submit`, `_cancel`, `VimState::message()`)
and 04c (`:noh` clears its highlight). Design: `docs/decisions/vim-engine.md` §1 (the ex subset
with ranges), §9 item 12 ("`:` lines parse to registry invocations with a `range` arg"), §11
(hjkl's ex behaviour was never verified against nvim: our corpus is the only check).

Assumes card 00 (the `visual.rs` split) has landed; `:` from visual mode uses the root
`src/visual.rs` (leaving the mode, `'<`/`'>`), nothing under `src/visual/`.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help cmdline-ranges`, `:help :d`,
`:help :noh`; never source, hjkl-ex, Zed or Helix.

## Change

### Entering (`src/cmdline.rs`, `src/reducer.rs`, `src/visual.rs`)

- `:` commits `vim.command_line {"kind": "ex"}`. A count before `:` prefills `.,.+{count-1}`; `:`
  in a visual mode leaves it (setting `'<`/`'>`) and prefills `'<,'>`. The prefill is
  `VimState::command_line_prefill()`, so the host shows it; `HeadlessVim` starts its line from it.

### Parsing (new `src/ex.rs`)

- A line is `[range] command [!] [args]`, parsed to an `Ex` value; parsing is pure, and leaves room
  for 05b's host commands and 05c's `:s`/`:&` (unknown to this card: E492).
- **Ranges** (`:help cmdline-ranges`): `N`, `.`, `$`, `%`, `'<`, `'>` (08a adds `'a`–`'z`),
  `/pat/` and `?pat?` (04a's patterns), `\/` `\?` `\&` (last search), each with `+N`/`-N` offsets
  (a bare `+` is `+1`), separated by `,` or `;` (`;` moves the cursor first). A backwards range
  follows nvim's headless answer, or is rejected with E493's message. A line past the end is E16.
- **Commands** (full names and the abbreviations `:help` lists):
  - `:{range}` alone: go to the range's last line, as nvim places the cursor.
  - `:d[elete] [x] [count]` (register and count), linewise, as `dd`; one transaction, `.` doesn't
    repeat it (nvim's `.` doesn't repeat ex commands).
  - `:noh[lsearch]`: clears 04c's highlight until the next search.
- Anything else is E492 "Not an editor command: …" in `VimState::message()`; nothing changes.

### Running (`src/commands.rs`)

`vim.command_line_submit` with kind `ex` parses and runs the line.

### The corpus loader's clipboard guard (`tests/support/mod.rs`)

T154's loader rejects any `+` or `*` in keys that also hold `" @ q <C-r> : =`, which rules out
`:.,+2d` and `:s/a*/b/`. Narrow it for `:` lines: from a `:` to its `<CR>`, `+` and `*` are allowed
except as a register argument (`:d +`, `:d *`, `:pu +`, after `<C-r>`). Everywhere else the guard is
as before. `corpus_loader_rejects_bad_cases` keeps passing unchanged.

### Corpus

`tests/corpus/ex.toml`, at least 30 cases: `:5`, `:$`, `:+2`, `:-1`, `:'<,'>` after `vj`, `:%`,
`:/pat/`, `:?pat?`, `:.,+2d`, `:3,4d a`, `:d 3`, `;` versus `,`, an empty range, a line past the end
(E16, last key), a backwards range, `:noh` (cursor and text unchanged), and an unknown command
(E492, last key).

## Tests (new files)

- `src/tests/ex.rs`, declared in `src/tests/mod.rs`.
- `tests/ex_oracle.rs` (`#[allow(dead_code)] mod support;`) with `ex_matches_neovim` and
  `loader_allows_plus_and_star_in_ex_ranges`.

## Existing tests

`tests::reducer::prefixes_targets_cancel_and_forward` lists `:` among keys forwarded with nothing
pending; replace it with another key fe-vim still forwards, and change nothing else there.

## Size

About 450 non-test lines (parser and ranges ~300, entering and running ~150).

## Out of scope

- Host commands `:w :q …` (05b), `:s`, `:&`, `&` (05c), the GPUI line (06a).
- `:g`, `:v`, `:normal`, `:m`, `:t`, `:co`, `:j`, `:y`, `:>`, `:<`, `:pu`, `:r`, `:sort`, `:set`,
  `:marks`, `:reg`, `:undo`, `:!`, `|` chaining, history, completion.
- Any change outside `crates/fe-vim`.
