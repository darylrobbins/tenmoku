---
task_uid: "01m55cee8w5hwhrm8yb02bmw3g"
task: "T164"
rev: 3
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test ex_star_range_matches_neovim (top level, in crates/fe-vim/tests/ex_star_range_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_star_range.toml, at least 8 cases recorded with nvim 0.12.x, among them (asserted by name) star_delete, star_substitute, star_alone_goes_to_last, star_with_offset, star_before_any_visual_fails, star_shift."
    required: true
    manual: false
  - id: "C2"
    text: "Ex::parse(\"*d\") gives the same range as Ex::parse(\"'\u003c,'\u003ed\"), asserted in ex_star_range_oracle.rs."
    required: true
    manual: false
---

# fe-vim: * as an ex range is the last visual area, '<,'>, as nvim's default cpoptions make it, instead of E492 (Mac T830)

## Problem

`*` as an ex range is E492 in fe-vim at Mac main 92c57c57 (listed as not planned). With nvim's default
'cpoptions' (no `*` flag) it stands for `'<,'>`, the last visual area (`:help :star-visual-range`). The user
approved it on 2026-10-17 in the "Vim breadth" track. Probed with `nvim --clean --headless` 0.12.6:

- `a/b/c/d` at (1,0), `Vj<Esc>gg:*d<CR>`: `a/d`, cursor (1,0).
- `a/b` at (0,0) with no visual area yet, `:*d<CR>`: E20 (mark not set), nothing changed.

## Change

In `crates/fe-vim/src/ex.rs` `Parser::address` and `Parser::range`: `*` where a line specifier can start
reads as the two specifiers `'<` and `'>` (`Base::Mark('<')`, `Base::Mark('>')`), so `:*d`, `:*s/x/y/`,
`:*` alone (to `'>`'s line, as `:'<,'>` goes), `:*>` and `:*y` act on the last visual area's lines, with
E20 before there is one, exactly as the `'<,'>` the command line's prefill gives. Offsets and searches
after `*` follow nvim (probe `:*+1d`, `:*,$d`). A visual mode's `:` still fills in `'<,'>`.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/ex_star_range.toml` (header comment
as the other corpora; an error ends a case), recorded into `ex_star_range.expected.json` by the ignored
`record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/ex_star_range_oracle.rs`
modelled on `tests/ex_lines_oracle.rs`.
