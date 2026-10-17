---
task_uid: "01m55ceh2z3j1zbfczj27n5z30"
task: "T167"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test substitute_print_last_matches_neovim (top level, in crates/fe-vim/tests/substitute_print_last_oracle.rs) passes on crates/fe-vim/tests/corpus/substitute_print_last.toml, at least 14 cases recorded with nvim 0.12.x, among them (asserted by name) print_flag_p, print_flag_hash, print_flag_l, print_flags_with_g_and_count, amp_amp_keeps_no_print_flag, last_search_slash, last_search_question, last_substitute_amp, last_search_unset_fails, last_substitute_unset_fails, backslash_other_fails, n_after_last_search_form."
    required: true
    manual: false
  - id: "C2"
    text: "\\= in the replacement stays refused with its message (asserted in substitute_print_last_oracle.rs through HeadlessVim), and the change is in crates/fe-vim/src/substitute.rs alone."
    required: true
    manual: false
---

# fe-vim: :s takes the print flags p, # and l and ignores them, and :s\/, :s\? and :s\& substitute with the last search's or substitute's pattern, as nvim does (Mac T837)

## Problem

At Mac main 92c57c57 fe-vim's `:s` refuses the print flags `p`, `#` and `l` (E488, as trailing
characters: `FLAGS` in `crates/fe-vim/src/substitute.rs` lists them, but `flags_and_count` doesn't take
them) and the forms `:s\/{string}/`, `:s\?{string}?` and `:s\&{string}&` (`Substitute::parse` returns
`Unsupported(":s\\")`). The user approved both on 2026-10-17 in the "Vim breadth" track. Probed with
`nvim --clean --headless` 0.12.6:

- `abc`, `:s/b/B/p<CR>` and `:s/b/B/#<CR>`: `aBc`, as without the flag (nvim prints the last line
  changed; fastedit has nowhere to print it, as `:j`'s print flags are already taken and ignored).
- `abc abc`, `/c<CR>:s\/X/<CR>`: `abX abc`: the pattern is the last search's, the string up to the next
  `/` (`:help :s\/`); `:s\?X?` the same with `?`.
- `:s\&{string}&` uses the last substitute's pattern (`abc abc`, `:s/a/1/<CR>/c<CR>:s\&2&<CR>`: the last
  substitute's `a`, so `1bc 2bc`; probe it).

## Change

In `crates/fe-vim/src/substitute.rs`:

- `flags_and_count` takes `p`, `#` and `l` in any order among the other flags and ignores them (not
  `&`'s keep: they aren't kept by `&&`, check nvim), so `:s/x/y/gp 2` works as `:s/x/y/g 2`. The `:s`
  docs in substitute.rs say they are taken and ignored.
- `Substitute::parse`: after `\`, `/` and `?` read a string up to that delimiter with the last search's
  pattern, and `&` one up to `&` with the last substitute's pattern (E35 and E33 when unset, as for an
  empty pattern and `:&`); then flags and a count as usual. Whatever the pattern came from, it becomes the
  last search and the last substitute's pattern as an explicit one does (check `n` after each against
  nvim). A `\` before anything else stays E10-like as nvim's (probe `:s\x`).

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/substitute_print_last.toml` (header
comment as the other corpora; an error ends a case), recorded into `substitute_print_last.expected.json` by
the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/substitute_print_last_oracle.rs` modelled on `tests/ex_lines_oracle.rs`. Leave
`\=` (an expression) refused: fastedit has no expression evaluator.

substitute.rs is Mac T769's (the two-letter forms and the `r` flag, in the host's stack 24): start on a
trunk that has it.
