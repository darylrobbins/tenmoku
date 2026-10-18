---
task_uid: "01m55ase26xffakhw81cvpwpfg"
task: "T158"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test unmatched_bracket_operators_match_neovim (top level, in crates/fe-vim/tests/unmatched_bracket_operators_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/unmatched_bracket_operators.toml (at least 10, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into unmatched_bracket_operators.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists (d[(, d]), y]), c])X\u003cEsc\u003e, d2[(, d[( with none unmatched), d[{ and d]} on braces, . after d]), and d[c cancelling."
    required: true
    manual: false
---

# fe-vim: an operator takes [( [{ ]) ]} as its target (d[(, c]), y]}), as nvim does, instead of cancelling (Mac T805)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming the
unmatched bracket motions `[(` `[{` `])` `]}`; Mac T798 adds them in normal and visual
mode. Under an operator they still fail at Mac main 92c57c57: `reducer.rs` starts the `[`/`]` prefix only
`if pending.operator.is_none()`, so `d[(` cancels. Probed with `nvim --clean --headless` 0.12.6 on
`a ( b ( c ) d ) e` from col 8: `d[(` leaves `a ( b c ) d ) e`, caret (0,6); `d])` leaves
`a ( b ( ) d ) e`; `y])` keeps the caret at col 8; `c])X<Esc>` puts X before the `)`; `d2[(` from col 8
takes back to col 2; `d[(` with none unmatched does nothing.

## Change

`crates/fe-vim/src/reducer.rs`: `[` and `]` start `Prefix::Bracket` after an operator too.
`crates/fe-vim/src/changes.rs`'s `bracket` hook returns `None` with an operator pending, so `d[c`
cancels as it does now (diff-mode `d]c` is out of scope). Mac T798's hook then commits
the operator over its motion (exclusive, charwise), and `.` repeats it.

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/unmatched_bracket_operators.toml`
(every probed case above, `d[{` and `d]}` on braces, `.` after `d])`, and `d[c` cancelling) recorded into
`unmatched_bracket_operators.expected.json` by the ignored `record_corpus_with_neovim` test in
`tests/oracle.rs`, and a new `crates/fe-vim/tests/unmatched_bracket_operators_oracle.rs` modelled on
`tests/line_motions_oracle.rs`.

reducer.rs belongs to Mac T770 (landing in Mac host stack 24) and to Mac T803 (gR), which this card depends on, and the
motions come from Mac T798: sync a mac/main that carries all three first.
`crates/fe-vim/src/lib.rs`'s docs are out of scope; the lead files one docs chore after these cards land.
