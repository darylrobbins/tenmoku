---
task_uid: "01m4txwfjm9k46x1g4h10fmzee"
task: "T662"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui element::jump_tests::no_labels_after_first_char: with vim on and many p's followed by different chars, g s p paints no Label, Direct or DirectEdge jump paints and the status reads JUMP p; then u plus a shown Live label moves the caret to that target"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim jump_labels.toml preview_after_first_char and preview_after_operator expect labels = []; jump_overlay after {c1} returns no labels"
    required: true
    manual: false
  - id: "C3"
    text: "After {c2} the overlay is unchanged: autojump, Live labels on the rest of the group, Concealed for other pages (existing jump_spec, jump_fold, jump_style_tests, jump_wrap_tests pass)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-vim -p fe-gpui and cargo clippy -p fe-vim -p fe-gpui --all-targets -D warnings pass"
    required: true
    manual: false
---

# Paint no jump marks after the first char: drop the preview letters and direct-target rings, labels only after {c2}, as leap.nvim with preview off

## Problem

The user (2026-10-13, running app, `gs p` on crates/fe-editor/src/completion.rs; their screenshot shows it):
"The visuals are confusing though. Why is it showing highlighted targets that are irrelevant after the first
character? … Particularly the preview letters since typing them doesn't bring me there."

After {c1}, every {c1} in view in the jump direction gets a mark, plus the backdrop:

- `label::preview` (`crates/fe-vim/src/jump/label.rs:101-146`) groups each {c1} by the char after it and labels
  each group as if that pair were typed. `jump_overlay`'s `(None, Some(c1))` arm (`crates/fe-vim/src/jump.rs`
  ~724-734) emits `JumpLabelState::Direct` (label `None`: the group's autojump target) and
  `JumpLabelState::Preview` (the future label) for each.
- fe-gpui paints Direct as a 2px pink ring (`crates/fe-gpui/src/element.rs` ~2007-2015, 2168-2188,
  `JUMP_DIRECT_BORDER`, `direct_ring`, `subtract`; added by T483) and Preview as an opaque cyan box with the
  letter OVER the {c1} glyph (~2190-2229), so the pair can't be read ("Com[s]letionItem").
- The preview letters look pressable but aren't: `key()` (`jump.rs` ~520-533) always reads the next char as
  {c2}, so `s` after `p` searches `ps`. Each {c2} group is labelled from SAFE independently, so the same letter
  shows on many targets. Page>0 labels also show as plain previews (`jump.rs:724` ignores `label.page`).

leap.nvim with `preview = false` shows nothing after {c1}; even with its preview it drops marks on unlabeled
targets on purpose ("no highlight should be the very signal for no further keystrokes needed").

## Change (the user chose this, 2026-10-13)

After {c1}, paint no marks on targets: no Direct rings, no Preview letters. Each stage becomes:

- before any char: nothing (unchanged);
- after {c1}: the backdrop and the status `JUMP p` only;
- after {c2}: unchanged (autojump to the nearest match, Live labels on the rest of its group, Concealed boxes
  for other pages).

In `crates/fe-vim/src/jump.rs`, `jump_overlay`'s `(None, Some(c1))` arm returns no labels. Remove
`label::preview` and the `Preview` and `Direct` label states and their painting in `crates/fe-gpui/src/element.rs`
(the Direct ring path: `JUMP_DIRECT_BORDER`, `direct_ring`, `subtract`), unless something after {c2} still uses
them. Delete `crates/fe-gpui/src/element/jump_direct_tests.rs` and `jump_ring_tests.rs` (and their `mod` lines)
and `crates/fe-vim/tests/jump_direct.rs` if all they test is removed; rewrite T580's preview-equals-live tests in
`jump_tests.rs` to check the post-{c2} labels only. Update `crates/fe-vim/tests/corpus/jump_labels.toml`:
`preview_after_first_char` and `preview_after_operator` expect `labels = []`.

## Tests

Regression test in the EXISTING `crates/fe-gpui/src/element/jump_tests.rs` (declared at base, so the
regression check can run it there): `no_labels_after_first_char`: a drawn EditorView with vim on, a buffer
with many `p`s each followed by a different char; `g s p` paints no Label, Direct or DirectEdge entries in the
jump paints and the status reads `JUMP p`; then `u` and one of the shown Live labels moves the caret to that
target.

## Out of scope

The one-char flash jump (T659, parked). Label placement after {c2}.
