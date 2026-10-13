---
task_uid: "01m4saeqm0pshvm9867az7k8xj"
task: "T98"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test repeat_command_line_matches_neovim: tests/corpus/repeat_command_line.toml has at least 15 cases recorded from nvim 0.12.4 and all agree, covering @: after :s, after :d with a range, after a failing line, 2@:, @@ after @:, @: after an \u003cEsc\u003e-cancelled line (runs the last executed one), @: inside a recorded macro, Q after qa...q, qA appending then Q, 3Q, and @@ after Q"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test repeat_command_line_oracle::no_previous_line: on a fresh HeadlessVim, @: fails with message E30: No previous command line and changes nothing; after @: the : history (HistoryRecall \u003cUp\u003e) holds no new entry"
    required: true
    manual: false
  - id: "C3"
    text: "Q with nothing recorded changes no text, so crates/fe-gpui's vim tests that type QK (tests.rs, visual_tests.rs, insert_tests.rs) pass unchanged; every earlier corpus expected file is unchanged and its oracle test passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-vim passes; no file outside crates/fe-vim changes; no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: @: repeats the last command line and Q the last recorded macro, with counts and @@, checked against nvim (Mac T576)

## Problem

M7 vim parity: fe-vim's `@` plays `a`–`z` and `@@` (`crates/fe-vim/src/macros.rs` `play`, the reducer's
`Prefix::Play` in `crates/fe-vim/src/reducer.rs`), but `@:` (repeat the last command line, `:help @:`) cancels, and
`Q` (nvim's "repeat the last recorded register [count] times", `:help Q`) is forwarded as nobody's key. Both are
everyday nvim keys: `:s/x/y/` then `j@:` / `@@` down a file, `qq…q` then `Q Q Q`.

Probed in nvim 0.12.4 (`nvim --clean --headless -l`): after `:s/a/b/<CR>:let g:x=1<CR>:let g:x=2<Esc>`, `@:` runs
`let g:x=1` (the last *executed* line, not the `<Esc>`-cancelled one, which T566 now keeps in history) and adds
nothing to the `:` history; `2@:` runs the line twice; `@@` after `@:` runs it again; after `qaxq` and `u`, `Q`
deletes again.

## Change

- `crates/fe-vim/src/macros.rs`: `Macros` gains `last_command_line: Option<String>` and `last_recorded:
  Option<char>` (lowercase; `qA` records into `a`), the latter set in `stop`. `play` takes `:` and `Q`:
  - `:`: runs the last command line `count` times through `ex::run` directly (not as replayed keys, so the `:`
    history is untouched, as nvim's), stopping at the first failure; none yet is `E30: No previous command line`
    (a failure, as an empty register is). `@@` afterwards repeats `@:` (`last_played = ':'`).
  - `Q`: plays `last_recorded` as `@{that}` does (and sets `last_played` as nvim does: probe); none is a failure
    with nvim's message (probe).
- `crates/fe-vim/src/commands.rs`: `vim.command_line_submit`'s handler records an ex line (not a search) into
  `last_command_line` when it's submitted, typed or replayed, whether or not it then fails (probe nvim and follow
  it); lines run by `:normal`/`:g` and by `@:` itself don't set it. `vim.play` passes the actor through and runs
  `marks::settle` after an `@:` as submit does. Descriptions updated.
- `crates/fe-vim/src/reducer.rs`: `Prefix::Play` also takes `:`; `Q` in normal mode with nothing but a count pending
  commits `vim.play {"register": "Q", "count"}` (a register before it is dropped); in visual or operator-pending
  mode `Q` still cancels (nvim's visual `Q`/`@` are default mappings over `:normal`, not this card).
- `crates/fe-vim/src/tests/reducer.rs` and `crates/fe-vim/src/tests/commands.rs` drop `Q` from their
  "nobody's key" lists (it's a command now); nothing else in them changes.

Not this card: the read-only `":` register for `":p`/`<C-r>:` (in `register.rs`, held by T566). Clean room: fit
behaviour from `nvim --clean` probes and `:help` only. The oracle runs every case in one nvim session and can't reset
the last command line, so every `@:` case runs its own `:` line first, and E30 is a plain `HeadlessVim` test in the oracle test file, not a corpus case.

## Tests

New corpus `crates/fe-vim/tests/corpus/repeat_command_line.toml` with its recorded
`repeat_command_line.expected.json`, checked by new `crates/fe-vim/tests/repeat_command_line_oracle.rs`, shaped
like `tests/macros_oracle.rs`.
