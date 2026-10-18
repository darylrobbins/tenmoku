---
task_uid: "01m55cdvzghb82w9cjca6kz68k"
task: "T148"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test put_keys_match_neovim (top level, in crates/fe-vim/tests/put_keys_oracle.rs) passes on crates/fe-vim/tests/corpus/put_keys.toml, at least 18 cases recorded with nvim 0.12.x, among them (asserted by name) y_to_line_end, y_with_count, y_into_register, y_in_visual_is_linewise, gp_linewise, gp_linewise_count, gp_linewise_at_buffer_end, gp_charwise, gP_linewise, gP_charwise, gp_block, gp_then_dot, gp_empty_register_fails."
    required: true
    manual: false
  - id: "C2"
    text: "Y, gp and gP are taken by the new feature module crates/fe-vim/src/put_keys.rs through its FEATURES entry in feature.rs (no change to commands.rs, and reducer.rs changes only by the mechanical `cursor_after: false` field initializer in its existing PutArgs struct literals), and PutArgs's new cursor_after field is skipped when false, so the JSON of an existing vim.put invocation is unchanged (asserted in put_keys_oracle.rs)."
    required: true
    manual: false
---

# fe-vim: normal-mode Y yanks to the line's end as nvim's y$ mapping does, and gp and gP put as p and P but leave the cursor after the new text (Mac T834)

## Problem

At Mac main 92c57c57 normal mode's `Y` does nothing in fe-vim (the reducer's `shorthand` has `D` and `C`
but no `Y`, so the key is rejected), and `gp` and `gP` cancel (no `g` arm or feature takes `p`). The user
approved them on 2026-10-17 in the "Vim breadth" track. Probed with `nvim --clean --headless` 0.12.6 (the
oracle's settings):

- `Y` is nvim's default mapping `y$` (`:help Y-default`): `abc def/xyz` at (0,4), `Y` yanks `def`
  charwise, the cursor stays; `2Y` yanks `def\nxyz` charwise.
- `gp` and `gP` put as `p` and `P` do but leave the cursor just after the new text (`:help gp`):
  `a/b/c` at (0,0), `yygp`: `a/a/b/c`, cursor (2,0); `yy3gp` on `a/b`: `a/a/a/a/b`, cursor (4,0);
  `a/b/c` at (1,0), `yygP`: `a/b/b/c`, cursor (2,0); `abc def` at (0,0), `yiwgp`: `aabcbc def`, cursor
  (0,4).

## Change

A new feature module `crates/fe-vim/src/put_keys.rs`, listed in `crates/fe-vim/src/feature.rs`'s
`FEATURES` (the seam its docs describe):

- its `key` hook takes `Y` in normal mode (not visual, where `Y` is already a linewise yank, and not after
  an operator) and commits a yank to the line's end as `y$` would, count and register kept, through
  `Pending::operate` with `Motion::LineEnd` (so `2Y` is `y2$`, and `.` isn't affected: a yank isn't a
  change).
- its `g` hook takes `p` and `P` and commits `vim.put` with a new `PutArgs` field in
  `crates/fe-vim/src/args.rs`, `cursor_after: bool` (serde default false, skipped when false, so existing
  invocations and `.`'s recorded ones are unchanged), count and register kept.
- `crates/fe-vim/src/operator.rs` `put`: with `cursor_after`, the caret ends just after the last character
  put (charwise and block), or at the start of the row after the last row put (linewise; at the buffer's
  end, where nvim leaves it: probe `Gyygp`). `.` repeats a `gp` as a `gp`.

Visual `gp`/`gP` (`:help v_gp`) are out of scope. Test with the oracle pattern: a new corpus
`crates/fe-vim/tests/corpus/put_keys.toml` (header comment as the other corpora; a key that fails is a
case's last key), recorded into `put_keys.expected.json` by the ignored `record_corpus_with_neovim` in
`tests/oracle.rs`, and a new `crates/fe-vim/tests/put_keys_oracle.rs` modelled on
`tests/line_motions_oracle.rs`.

feature.rs, args.rs and operator.rs are also changed by Mac T798 and T801 (lent to neon, in that order):
this card depends on T801, so start it on a trunk that has both.

## Implementation notes

- reducer.rs gains only `cursor_after: false` in its two `PutArgs` struct literals (p/P normal and visual); a new required field cannot be added without them. No behaviour there changed: Y, gp and gP are taken by `put_keys.rs` through `FEATURES`.
- Linewise `'[` and `']` after `gp`/`gP` are covered by the `gp_marks_*` and `gP_marks_*` corpus cases.
