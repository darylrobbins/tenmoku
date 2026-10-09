---
task_uid: "01m4ekwdqkd639cxqs727js9yh"
task: "T205"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "With an unset register (the unnamed one, or a named one never written), counted V2p and V2P on \"abc\\ndef\\nghi\" over the first, middle and final rows and the whole buffer give \"def\\nghi\", \"abc\\nghi\", \"abc\\ndef\" and \"\" with the cursor the uncounted put leaves: tests::visual_put_unset::a_counted_put_over_lines_from_the_unset_unnamed_register_puts_nothing and ::a_counted_put_over_lines_from_an_unset_named_register_puts_nothing fail at base and pass."
    required: true
    manual: false
  - id: "C2"
    text: "An unset register bounds no insertion, so ggV99999999p is not refused and gives \"def\\nghi\": tests::visual_put_unset::a_huge_count_from_an_unset_register_is_not_refused fails at base and passes."
    required: true
    manual: false
  - id: "C3"
    text: "\"_\" and an explicitly empty characterwise register still put one row per copy: tests::visual_block::black_hole_put_over_lines_keeps_a_row passes unchanged, and tests::visual_put_unset::an_empty_characterwise_register_still_puts_a_row_per_copy passes."
    required: true
    manual: false
  - id: "C4"
    text: "Outside the new test file, its one mod line and the lane's own Regatta spec file (docs/specs/qs727js9yh-*.md, added by the lane's spec commit), only crates/fe-vim/src/visual/put.rs (after T188) changes; crates/fe-vim/src/visual.rs is not edited."
    required: true
    manual: false
---

# fe-vim: counted linewise p and P from an unset register put nothing, as nvim does (T174 F25)

# fe-vim: counted linewise p and P from an unset register put nothing, as nvim does (T174 F25)

## Problem

T174's final review (codex, review `01m4ekwdp9xm91gmeva22c127s`, round 4; first raised in round 3,
`01m4ed2qq3vstnkp1qj8rac675`) left F25 (medium) open; the user overrode it at merge with C2 unmet:

> counted linewise p/P treats an unset register as empty characterwise content and inserts count−1
> unwanted blank rows. On "abc\ndef\nghi", V2p produces "\ndef\nghi", while Neovim 0.12.4 produces
> "def\nghi". This affects unset unnamed and named registers at first, middle, final and whole-buffer
> selections; the bound also rejects large counts although nothing should be inserted.

Instances on main `c2bbe1c`: `crates/fe-vim/src/visual.rs:1638` (`put_bound`, the
`(None, VisualKind::Line)` arm: `payload.len() + 1` times `count`) and `:2137` (`put`, the
"Over lines, each copy of characters on rows of its own" arm, which builds
`format!("{payload}\n").repeat(count)` for a linewise selection). `put` already computes
`unset` (`value.is_none() && args.register != Some('_')`, visual.rs:1993) and uses it to delete
the whole lines with their separator; the count expansion and the bound ignore it.

## Change

**Base: after T188.** T188 moves `put_size`, `split`, `put_bound`, `put_block` and `put` into
`crates/fe-vim/src/visual/put.rs` (about 330 lines), so this fix lands there and not in the
2,200-line `visual.rs`. Find the two sites by symbol, not by the line numbers above.

- In `put`, when `unset` is true, plan no insertion whatever the count: a counted `p` or `P`
  over a linewise selection leaves exactly what the uncounted one leaves (text and cursor), which
  T174 already matched to nvim.
- In `put_bound` (or its call in `put`), an unset register bounds nothing: pass it in, or skip
  the bound when `unset`, so a count past `operator::MAX_PUT` isn't refused.
- `_` and an explicitly empty characterwise register keep one row per copy, as now
  (`tests::visual_block::black_hole_put_over_lines_keeps_a_row` covers `ggjV2"_p`).

Tests go in a new `crates/fe-vim/src/tests/visual_put_unset.rs`, declared by one `mod
visual_put_unset;` line in `crates/fe-vim/src/tests/mod.rs`, using `HeadlessVim` as
`visual_block.rs` does. On `"abc\ndef\nghi"` with fresh registers:

- `a_counted_put_over_lines_from_the_unset_unnamed_register_puts_nothing`: `V2p` and `V2P` over
  the first row (`gg`), the middle row (`ggj`), the final row (`G`) and the whole buffer (`ggVG`)
  give `"def\nghi"`, `"abc\nghi"`, `"abc\ndef"` and `""`, each with the cursor the uncounted `Vp` /
  `VP` leaves there.
- `a_counted_put_over_lines_from_an_unset_named_register_puts_nothing`: the same eight cases with
  `"a` (`V"a2p`, `V"a2P`).
- `a_huge_count_from_an_unset_register_is_not_refused`: `ggV99999999p` succeeds and gives
  `"def\nghi"`.
- `an_empty_characterwise_register_still_puts_a_row_per_copy` (not a regression test; passes at
  base): a register written empty and characterwise through `registers_mut().write`, put with
  `ggjV2"ap`, gives `"abc\n\n\nghi"`, as `"_` does.

## Out of scope

Blockwise and characterwise selections, other T174 follow-ups, any other file.
