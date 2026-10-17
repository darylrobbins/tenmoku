---
task_uid: "01m55asfm8vqr0sva1q1p9jkbn"
task: "T145"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test partial_word_search_matches_neovim (top level, in crates/fe-vim/tests/partial_word_search_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/partial_word_search.toml (at least 12, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into partial_word_search.expected.json) matches nvim's buffer, cursor, mode, registers (the / register among them), visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists (g*, 2g*, g*n, g# , dg*, g*\"/p, a keyword beside punctuation, vg*) plus g* on a blank row (failing) and `` back after g*."
    required: true
    manual: false
---

# fe-vim: g* and g# search for the word under the caret without word boundaries, as nvim does (Mac T801)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming `g*` and
`g#` (`:help gstar`): `*` and `#` without the `\<` `\>` word boundaries. At Mac main 92c57c57 `g` then `*`
cancels (no feature's `g` hook takes it). Probed with `nvim --clean --headless` 0.12.6 on
`foo foobar foo barfoo` from col 0:

- `g*` lands on col 4 (the `foo` in `foobar`), `2g*` and `g*n` on col 11; `g#` from col 11 lands on 4.
- `dg*` leaves `foobar foo barfoo`; `g*"/p` puts `foo` (the last search is `foo`, no boundaries).
- On `a.b a.bc`, `g*` searches `a` (the keyword under the caret) and lands on col 4.
- In visual mode `vg*` moves the vim cursor to col 4.

## Change

A new feature module `crates/fe-vim/src/partial_word.rs`, listed in `feature.rs`'s `FEATURES`, whose `g`
hook takes `*` and `#` and commits new motions `Motion::SearchPartialWordForward` and
`Motion::SearchPartialWordBackward` (`args.rs`) through `Pending::motion`. They behave exactly as
`SearchWordForward`/`SearchWordBackward` do everywhere those are listed (`args.rs`'s class, jump and
`writes_numbered` lists, `motion.rs`'s "resolved before they run" arm, `operator.rs`'s list near line 573),
and `cmdline.rs`'s `resolve` turns them into `Motion::SearchWord` with the pattern `search::word_at`
returns stripped of its `\<` prefix and `\>` suffix. Both are present exactly when the run is a keyword
run (a run is one class), so strip the suffix only when the prefix was there; don't change `search.rs`.
Everything else holds as for `*` and `#`: the count, E348 on a blank row, the last search and its
direction, `n`/`N`, the jump mark, visual mode.

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/partial_word_search.toml` (the
probed cases above among them; a failing key last) recorded into `partial_word_search.expected.json` by
the ignored `record_corpus_with_neovim` test in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/partial_word_search_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

motion.rs is held by T594 (lent to neon) and args.rs/motion.rs/feature.rs by Mac T798 (the bracket motions), so
this card depends on both. `crates/fe-vim/src/lib.rs`'s docs are out of scope; the lead files one docs
chore after these cards land.
