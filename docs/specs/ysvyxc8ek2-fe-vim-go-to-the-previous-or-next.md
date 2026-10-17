---
task_uid: "01m55asa2fwrd7e9ysvyxc8ek2"
task: "T143"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test unmatched_brackets_match_neovim (top level, in crates/fe-vim/tests/unmatched_brackets_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/unmatched_brackets.toml (at least 24, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into unmatched_brackets.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, cases for each of [( [{ ]) ]}: with a count, across rows, from inside nested pairs, skipping a bracket inside a double-quoted string, with none unmatched (failing, the jump mark left unset), `` back after a jump, and in charwise and linewise visual mode."
    required: true
    manual: false
---

# fe-vim: [( [{ ]) ]} go to the previous or next unmatched bracket, as nvim does, in normal and visual mode (Mac T798)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track". Among them are the
unmatched bracket motions `[(` `[{` `])` `]}` (`:help [(`), which fe-vim lacks at Mac main 92c57c57: after
`[` or `]` only `changes.rs`'s `c` is taken, so `[(` cancels. Probed with `nvim --clean --headless` 0.12.6
(the oracle's settings) against `HeadlessVim`:

- `[(` on `a ( b ( c ) d ) e` from col 8: nvim (0,6), fe-vim stays at 8; `2[(` nvim (0,2).
- `])` from col 2: nvim (0,14). Across rows, `[(` on `f(x,\n  y)` from (1,2): nvim (0,1); `])` from (0,2): (1,3).
- Brackets in a double-quoted string are skipped: `])` on `f(x, ")", y)` from col 3 lands on col 11.
- With none unmatched (`[(` from col 0) nvim fails, staying put.
- They are jumps: ``[(`` then ``` `` ``` comes back to col 8; `''` too.
- In visual mode `v[(d` on the first buffer from col 8 leaves `a ( b  ) d ) e`.

## Change

A new feature module, `crates/fe-vim/src/unmatched.rs`, listed in `feature.rs`'s `FEATURES`, whose
`bracket` hook takes `(` and `{` after `[` and `)` and `}` after `]` (the count already ended) and commits a
new motion through `Pending::motion`: add `Motion::Unmatched { brace: bool, forward: bool }` (or an
equivalent single variant) in `args.rs`, exclusive and charwise as `:help [(` says, a jump in
`Motion::is_jump`, not writing register `1` within a row; and an arm in `motion.rs`'s `motion()` that
delegates to `unmatched::motion`. Search back (or forward) for the `count`th unmatched opener (closer),
across rows, skipping brackets that `%` skips in quotes: reuse `motion.rs`'s `Scan` (its `quoted` and
`escaped` tables) rather than a second quote scanner. A motion that finds none fails (`Err(goal)`), and,
like `%`'s and `gs`'s, a failing one must not set the jump mark: add its arm to `marks::jump`'s "lands"
check in `marks.rs`.

Under an operator (`d[(`) the reducer doesn't yet start a `[` prefix (`reducer.rs`: `'[' | ']' if
pending.operator.is_none()`); that is Mac T805, so leave
`reducer.rs` and `changes.rs` alone here and keep operator cases out of this corpus.

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/unmatched_brackets.toml` (a header
comment as the other corpora have; a key that fails is a case's last key), recorded into
`unmatched_brackets.expected.json` by the ignored `record_corpus_with_neovim` test in `tests/oracle.rs`, and
a new `crates/fe-vim/tests/unmatched_brackets_oracle.rs` modelled on `tests/line_motions_oracle.rs` (load,
a minimum case count, the list of case names the card asks for, `check` on each).

motion.rs is held by T594 (lent to neon), so this card depends on it: start on a trunk that has T594.
`crates/fe-vim/src/lib.rs`'s docs are out of scope; the lead files one docs chore after these cards land.
