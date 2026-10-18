---
task_uid: "01m55ce1579jfqqvpyqrmf30bf"
task: "T149"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test put_indent_matches_neovim (top level, in crates/fe-vim/tests/put_indent_oracle.rs) passes on crates/fe-vim/tests/corpus/put_indent.toml, at least 14 cases recorded with nvim 0.12.x, among them (asserted by name) bracket_p_below_indents, bracket_p_above_indents, big_p_forms_go_above, outdents_to_a_shallower_row, mixed_indents_keep_their_offsets, first_line_blank, tab_indented_row, charwise_is_plain_put, block_is_plain_put, with_count, then_dot."
    required: true
    manual: false
  - id: "C2"
    text: "PutArgs's new indent field is skipped when false (an existing vim.put invocation's JSON is unchanged), and the bracket keys are taken by put_keys.rs's bracket hook, with no change to reducer.rs, changes.rs or feature.rs."
    required: true
    manual: false
---

# fe-vim: ]p, [p, ]P and [P put lines with their indent moved to the cursor's row's, as nvim does, instead of cancelling (Mac T835)

## Problem

`]p`, `[p`, `]P` and `[P` (`:help ]p`) cancel in fe-vim at Mac main 92c57c57: after `[` or `]` only
`changes.rs`'s `c` is taken. The user approved them on 2026-10-17 in the "Vim breadth" track. Probed with
`nvim --clean --headless` 0.12.6 (ts=4 sw=4 et):

- `    foo/bar` at (1,0), `yyk]p`: `    foo/    bar/bar`, cursor (1,4): the lines put below, their indent
  moved to the cursor's row's.
- the same with `yyk[p`: `    bar/    foo/bar`, cursor (0,4): above, as `P`. `]P` and `[P` are `[p` too.
- a charwise register: `yiwk$]p` gives `    foobar/bar`, cursor (0,9): exactly `p`.

## Change

In `crates/fe-vim/src/put_keys.rs` (Mac T834's feature module), a `bracket` hook: `p` after `]` commits
`vim.put` below, and `P` after `]`, `p` and `P` after `[` commit it above, each with a new `PutArgs` field
in `crates/fe-vim/src/args.rs`, `indent: bool` (serde default false, skipped when false), count and
register kept. `crates/fe-vim/src/operator.rs` `put`: with `indent` and a linewise register, every line put
has its indentation changed by the same amount, so the first line's matches the cursor's row's (as nvim
does, measured in screen columns with the model's tab size and re-made with fe-vim's indent unit, as `>>`
makes it; probe a register mixing indents and one whose first line is blank); with a charwise or block
register it is the plain put. The cursor goes where nvim's does (first non-blank of the first line put).
`.` repeats it with the indent kept. In a visual mode they stay as they are.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/put_indent.toml` (header comment as
the other corpora; a key that fails is a case's last key), recorded into `put_indent.expected.json` by the
ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/put_indent_oracle.rs`
modelled on `tests/line_motions_oracle.rs`.

## Implementation notes

- Adding `PutArgs::indent` needs `indent: false` in the two `PutArgs` literals in `reducer.rs` (and in tests), so reducer.rs has that one-line-per-literal edit; no behaviour there.
- Empty lines stay empty; the first non-empty line sets the indent offset (probed with nvim 0.12).
