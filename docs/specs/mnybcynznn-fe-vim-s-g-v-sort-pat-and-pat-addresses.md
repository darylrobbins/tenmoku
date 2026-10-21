---
task_uid: "01m5e06fpg7j67mvmnybcynznn"
task: "T239"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test ex_search_case_matches_neovim (top level, in crates/fe-vim/tests/ex_search_case_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_search_case.toml, at least 12 cases recorded with nvim 0.12.x using the set field, among them (asserted by name) s_ic, s_scs_uppercase, s_flag_upper_i_wins, s_flag_i_wins, s_empty_pattern, amp_repeat, g_ic, v_scs, sort_pattern_ic, address_pattern_ic."
    required: true
    manual: false
---

# fe-vim: :s, :g, :v, :sort /pat/ and /pat/ addresses follow 'ignorecase' and 'smartcase' as nvim's do (Mac T893)

## Problem

The search-case card makes `/ ? n N * #` follow 'ignorecase' and 'smartcase'; fe-vim's ex commands that take
a pattern still match case exactly: `:s` (`crates/fe-vim/src/substitute.rs`, `Pattern::parse_with` with only
its `i` flag), `:g`/`:v` (`crates/fe-vim/src/ex/global.rs`), `:sort /pat/` (`crates/fe-vim/src/ex/lines.rs`)
and `/pat/` addresses (`crates/fe-vim/src/ex.rs`). In nvim 0.12 all of them use 'ignorecase', and `:s` and
`:g` also 'smartcase' (`:help 'smartcase'`). The user approved the options in the "Vim parity" track on
2026-10-17.

## Change

Each of those reads `Registers`' search case (from the search-case card) when it parses its pattern, as nvim
does: `:s`'s `i` and `I` flags still win; `:s` with an empty pattern (the last search) and `:&`/`&`/`g&`
follow the options as nvim's do (probe); the highlight `:s` leaves matches as the search would. Probe
`:sort /pat/` and `:/pat/` addresses for smartcase.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/ex_search_case.toml` using the `set`
field, recorded into `ex_search_case.expected.json` by the ignored `record_corpus_with_neovim` in
`tests/oracle.rs`, and a new `crates/fe-vim/tests/ex_search_case_oracle.rs` modelled on
`tests/line_motions_oracle.rs`.

substitute.rs is changed by the lent T837 and T838, and ex.rs by T822 through T865 (T865 last): this card
depends on T838 and T865.
