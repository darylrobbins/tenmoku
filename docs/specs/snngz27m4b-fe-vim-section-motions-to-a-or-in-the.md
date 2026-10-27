---
task_uid: "01m5qphzfghzvkrksnngz27m4b"
task: "T231"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test sections_match_neovim (top level, crates/fe-vim/tests/sections_oracle.rs): the recorded nvim corpus for ]] [[ ][ [] plain, counted, past the last section and from a { row agrees with fe-vim on buffer, cursor, mode, registers and marks"
    required: true
    manual: false
  - id: "C2"
    text: "sections_after_operators_match_neovim: d]] y[[ c][ and the .-repeat agree with nvim, including d]] also stopping below a } in the first column"
    required: true
    manual: false
  - id: "C3"
    text: "sections_visual_and_jumps_match_neovim: visual ]] and [[ extend as nvim does, and the jump mark is set for a motion that moves and not for one that fails (counted 5]] past the end)"
    required: true
    manual: false
  - id: "C4"
    text: "sections_without_braces_fail_quietly: text with no { or } in column 0 fails the motion, sets no jump mark, and a macro stops there"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim and cargo clippy -p fe-vim --all-targets pass; the module doc states the probed nvim facts and any documented limit"
    required: true
    manual: false
---

# fe-vim: ]] [[ ][ [] section motions to a { or } in the first column, with counts, operators and visual mode, checked against nvim (Mac T1158)

## Problem

The user asked on 2026-10-24 for the unimpaired-style pairs beyond `[d ]d`; `]]`, `[[`, `][` and `[]` are core Vim/Neovim section motions that fe-vim lacks (`]]` today cancels). They are also what most users type to hop between top-level C-family functions. Split from the syntax-based `]m` card.

## Current state

`[` and `]` go to feature hooks (`Feature::bracket`, `crates/fe-vim/src/feature.rs:142`; `crates/fe-vim/src/unmatched.rs` is the model for `[(`: a hook returning `pending.motion(Motion::Unmatched{..})`). A new motion needs: the variant, `class` and `is_jump` in `crates/fe-vim/src/args.rs` (:132, :375, :430), its arm in `motion.rs` (:310), `marks.rs` (:2251, `lands`, so a failing counted jump does not set `''`; memory fe-vim-feature-seam-gaps) and `visual.rs` (:754). Oracle corpus tests are `tests/corpus/<area>.toml` + `.expected.json` + a `tests/<area>_oracle.rs` replay (shape of `tests/changelist_oracle.rs`).

## Change

New `crates/fe-vim/src/feature/sections.rs`: after `[` or `]`, the keys `[`/`]` give `Motion::Section { forward, close }`: `]]` goes forward to the next `{` in the first column, `[[` back to the previous one, `][` forward to the next `}` in the first column, `[]` back (`:help ]]`). Counted; charwise exclusive; a jump; after an operator `d]]` also stops below a `}` in the first column, as the help says. With fewer than `count` it goes as far as there are, none fails and sets no jump mark. Probe nvim 0.12 (`--clean`) for the rest before coding, and write the answers into the module doc: the 'sections' nroff macros (`.SH`...) and formfeed lines, the first and last row, empty buffer, a caret already on such a row, `y]]` landing, visual `]]`. Match nvim where it is simple; list any macro handling you leave out as a documented, tested limit.

## Tests

New `tests/sections_oracle.rs` + `tests/corpus/sections.toml` + `sections.expected.json` recorded with the pinned nvim (record on the Mac if the host has none; the replay needs no nvim), with C-style, Python (no `{`: no motion), counts, operators, visual and the dot repeat; plus unit tests in the same file for `lands`. Every oracle case must fail at base (it does: `]]` cancels).

## Implementation notes

- nvim differs from the spec's guess on running out: as Vim's `findpar` does, each step that finds no `{` stops at the text's last row (first, going back), and only a step *before the last* that runs out fails. So a count of one never fails, even in Python or an empty buffer: `]]` goes to the last row's first non-blank. C4's "fails quietly" is checked with counts (`2]]`, `2[[`, `2][`, `2[]`, `d2[[`, `v]]2[[`), which fail, keep the jump mark and stop a macro (`]]2[[x`, `3@a`). The `none_` cases also record the count-one landing.
- Every probed fact is in the module doc of `crates/fe-vim/src/feature/sections.rs`: the formfeed rows and the default 'sections' macros (`.SH .NH .H .HU .nh .sh`, matched as `inmacro` does), the caret's own row never counting, the first non-blank in normal and visual mode, column 0 after an operator, the inclusive last char at the text's end with `]]` (even on a `{` row), `]]` after an operator also stopping at a `}` and ending on the row after it (or at a `}` on the last row, at its column 0), visual `]]` not stopping at a `}`, and the goal column resetting even when the motion fails.
- Documented limits: 'sections' can't be set, and starting inside a closed fold (where nvim starts past the fold) isn't handled.
- An operator's `]]` stops at more rows, so a count can land after an operator where it fails as a move (`d3]]` over `a { } b`). `marks::jump`, which the operators call, takes that into account (an operator sets the jump mark only once its target resolves). The moves call a new `marks::move_jump`, so a move's failing count doesn't set `''`.
- `visual.rs` needed no arm: its catch-all already moves a visual cursor with `motion::motion`. `which_key.rs` gets titles for the four keys, which its table test requires.
- Every one of the 116 oracle cases fails at base. Failing cases that would otherwise look like a cancelled key run a step nvim takes first (`]]2[[`), or check the jump mark with `:norm!`, whose failure doesn't flush the keys after it.
- Carried T197 F1: `Cursor::go_to_first_non_blank` (motion.rs) scans the row's bytes past the caret window, and the ten listed callers use it. `tests/caret_window.rs` `first_non_blanks_past_a_long_indent_land_as_past_a_short_one` checks `== gqq :s :> :d`, a linewise visual put, `cxx`, `yss` and the undo of `S`, with 400,000-blank indents and rows of blanks alone, against a 3-blank indent. It fails on the old lookup (`==` landed at column 16383).
- Review yv5nj8 F1: a failed section operator resets the goal column, as nvim's does (`$j:norm! d5]]` then `j` lands at column 2). The failure paths in `operator.rs` and `g_operators.rs` that already reset it for a `{` or `}` that ran out now take `Motion::Section` too. The `op_*_fail_resets_goal` cases cover `d y c gU gu gq` in all four directions, and `plain_fail_resets_goal` covers the move.
