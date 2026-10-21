---
task_uid: "01m5f6t8swq4qrmez237cqd6zv"
task: "T254"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test exchange_swaps_regions (top level, in crates/fe-vim/tests/exchange.rs) passes: cxiw then cxiw on another word swaps them; cxx twice swaps rows; a charwise region with a linewise one; a contained region replacing its container; overlapping regions failing and clearing the mark; cxc clearing it; . repeating cx; one u undoing the swap; and the marked region following an edit made between the two cx."
    required: true
    manual: false
---

# fe-vim: cx{motion}, cxx and cxc mark and swap two regions as vim-exchange does (Mac T890)

## Problem

At Mac main 6f032b12 fe-vim has no exchange operator (vim-exchange's `cx`, emulated by VSCodeVim and
IdeaVim): `cx` cancels. The user approved it in the "Vim parity" track on 2026-10-17.

## Change

New feature module `crates/fe-vim/src/feature/exchange.rs`, declared in `crates/fe-vim/src/feature.rs` and
listed in `FEATURES`, with a new `Operator::Exchange` (`crates/fe-vim/src/args.rs`, arm in
`crates/fe-vim/src/operator.rs`):

- `cx{motion or object}` (the `x` key after `c` pending) marks the target the first time, changing nothing,
  and kept on `VimState` (`crates/fe-vim/src/state.rs`) as anchors so it follows edits; the second time it
  swaps the two regions' text in one undo step. `cxx` takes the row (as `cc` would), `cxc` clears a marked
  region. Charwise, linewise and the mixed case as vim-exchange's documentation says (a linewise region
  swapped with a charwise one takes the other's kind); if one region contains the other, the contained one
  replaces the container; overlapping regions that neither contains fail, clearing the mark.
- `.` repeats `cx` (so `cxiw` then `w.` swaps two words).
- No visual-mode key: vim-exchange's visual `X` is nvim's "delete the rows", which stays.
- Highlighting the marked region is out of scope (the highlighted-yank card's decoration seam can carry it
  in a follow-up).

Behaviour reference: vim-exchange's documented behaviour (its README and doc, no source); no nvim oracle.
Tests in a new integration file `crates/fe-vim/tests/exchange.rs`.

## Implementation notes

- Scope widened to `crates/fe-vim/src/visual/operate.rs` and `crates/fe-vim/src/visual/block.rs`: their
  exhaustive `match` on `Operator` needs an (unreachable) `Exchange` arm. `operator::operate_over` sends
  every `cx` to `feature::exchange::operate` before either is reached; over a selection (only the `gc`
  object makes one, `cxgc`) it takes a linewise selection's rows and refuses any other.
- Review zw57ze round 1: F1 (an empty marked row followed the wrong row after `O`) — rows also anchor the
  first row's end and the last row's start, telling an empty row from text put in at it; F2 (a failed `cx`
  replaced `.`) — `.` is recorded only by a mark or a swap; F3 (`cxgc` refused) — taken as above. Each has
  a regression test in `tests/exchange.rs`.
- Review a86c39 round 2: F4 (high, a one-byte slice splitting UTF-8 in the empty-row check) and F1 again
  (a blank `O` looks the same in the buffer as a blank `o`: both insert `"\n"` at the empty row's offset) —
  both gone with the anchor-and-heuristic row tracking, replaced by the marks' places above, which see
  `O` through `Marks::opening`. Regressions: blank and non-blank `O`/`o` on single and multirow regions,
  `O` then `o`, multibyte rows, and deleting marked rows.
- `marks.rs` carries T221 F1/F2 (changelist grouping across a macro replay; `:s`'s changelist column).
  This card adds only the `exchange` field and its two follow-up lines there; neither finding's code
  path is touched.
- A charwise region is kept as two anchors (start biased right, end biased left, so text typed at an edge
  stays out). A linewise one is kept as two `marks::Place`s, its first and last rows', in a new
  `Marks::exchange` field (scope widened to `crates/fe-vim/src/marks.rs` for it), which `Marks::advance`
  and `update_but_changes` follow as they do `a`-`z`: `O` and a linewise `P` (`Marks::opening`) move an
  empty row's mark down; `o` doesn't; deleting a row drops its place. The swap re-resolves both regions inside
  `operator::apply_plan`, so it plans from the text the edits go into.
- A region is its text, a row's line break left out, so swapping two such ranges gives each text the kind
  of the region it goes into, as the spec asks. Two row regions also swap indentation, as vim-exchange's
  documented default `g:exchange_indent` does.
- Caret: the first `cx` puts it at `'[` and sets `'[`/`']` as `g@` does (checked against nvim 0.12's `g@`
  with a no-op 'operatorfunc'); the second at the start of what fills its own region (first non-blank for
  rows).
- Known differences, documented in the module: no highlight; primary caret only; `c2x` read as `2cx`; the
  keys after `c` are read as operator-pending keys (an `omap` of `x` or `c` applies). The last two would need
  `reducer.rs` and `remap.rs` (the latter carries T238 F2), which this card leaves alone.
- Carried T238 F5 (`gc`'s `'.`) is not fixed here: the fix needs a new mark note in `marks.rs` and a
  change in `feature/comment.rs`, both outside this card; `cx` doesn't touch the commenting path.
