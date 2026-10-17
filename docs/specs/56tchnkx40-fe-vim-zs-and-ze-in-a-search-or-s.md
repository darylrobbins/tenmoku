---
task_uid: "01m55asaebwsy8rs56tchnkx40"
task: "T144"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test search_match_bounds_match_neovim (top level, in crates/fe-vim/tests/search_match_bounds_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/search_match_bounds.toml (at least 20, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into search_match_bounds.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists: / ? and n landing on the \\zs start or the \\ze-bounded match, d/ with \\zs, and :s with \\zs, \\ze, overlapping a\\zsa, an empty \\zs match with g, \\1 beside \\zs, \\zs in a branch and in a repeated group, and \\v."
    required: true
    manual: false
---

# fe-vim: \zs and \ze in a search or :s pattern set where the match starts and ends, as nvim does, instead of refusing the pattern (Mac T799)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming `\zs` and
`\ze` in search patterns (`:help /\zs`, `:help /\ze`). At Mac main 92c57c57 fe-vim refuses them:
`search.rs`'s translator (the `'_' | 'z'` arm of its backslash atoms) returns
`PatternError::Unsupported`, so `/foo\zsbar<CR>` leaves "pattern not supported: \zs". Probed with
`nvim --clean --headless` 0.12.6 (the oracle's settings):

- `/foo\zsbar<CR>` on `foobar foobar` from col 0 lands on col 3, and `n` on col 10; `?foo\zsbar<CR>` from
  col 12 lands on 10; `/foo\zebar<CR>` from 0 lands on 7.
- `:s/foo\zsbar/X/g` makes `fooX fooX`; `:s/foo\zebar/X/g` makes `Xbar Xbar`; `:s/a\zsa/X/g` on `aaa`
  makes `aXa` (the next match is looked for after the whole match); `:s/\zsb/X/g` on `abab` makes `aXaX`;
  `:s/\zs/X/g` on `foo` makes `XfXoXo`; `:s/\(foo\)\zsbar/[\1]/` makes `foo[foo]`.
- `d/foo\zsbar<CR>` on `foobar` leaves `bar`.
- In a branch or a repeated group: `/\(foo\zsbar\|foo\)<CR>` and `/\(foo\zs\)*bar<CR>` on `foobar` land on
  col 3; `\vfoo\zsbar` works in very magic too.

## Change

In `crates/fe-vim/src/search.rs`, translate `\zs` and `\ze` (in every magic mode) into marks the matcher
reports, not into `regex` captures that shift `\1`…`\9`: a match's range becomes from where the last
`\zs` it passed matched (else its start) to where `\ze` matched (else its end), while groups and the
next-match position keep using the whole match. Searches (`/`, `?`, `n`, `N`, a search as an operator's
target, ex range searches) land on and count matches by that start, so a match whose `\zs` start is past
the cursor is found even when the whole match begins before it; `:s` replaces only that range (`&` and
`\0` are it, `\1`…`\9` still the groups), searching on after the whole match as nvim does; the painted
highlights use the same ranges. If a pattern also needs the backtracking matcher
(`crates/fe-vim/src/search/backtrack.rs`), give it the same two marks there. Any other `\z` atom stays
"pattern not supported".

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/search_match_bounds.toml`
(searches, `n`, `?`, `d/`, and `:s` cases, each probed case above among them; a failing key last),
recorded into `search_match_bounds.expected.json` by the ignored `record_corpus_with_neovim` test in
`tests/oracle.rs`, and a new `crates/fe-vim/tests/search_match_bounds_oracle.rs` modelled on
`tests/line_motions_oracle.rs`.

`substitute.rs` and `ex.rs` belong to Mac T769 (landing in stack 24): don't edit them; if `:s` needs a line
there, sync a mac/main that carries stack 24 first. `crates/fe-vim/src/lib.rs`'s docs (which list `\zs`
as unsupported) are out of scope; the lead files one docs chore after these cards land.
