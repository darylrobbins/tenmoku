---
task_uid: "01m5415s73g2wekjrnw2bxpjym"
task: "T126"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test visual_bracket_selection::brackets_over_a_selection_match_nvim passes: each of the 19 cases in the spec's first table gives nvim's anchor, cursor, mode v and failed flag, and it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "The same file's test brackets_nvim_already_matched_stay passes: the 8 cases the spec lists as already matching nvim are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test or corpus expectation edited (visual_matches_neovim, delimiters_matches_neovim, visual_block_objects and src/tests/delimiters.rs visual_objects_grow included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: i( a( i[ a[ i{ a{ i< a< typed over a visual selection of more than one position pick the pair from the selection's start as nvim does, instead of failing or growing (Mac T735)

Filed on argon from Mac T735 (card ~/coordination/argon-cards/T735.json rev 1; note zq). Mac task uid 01m540p318cfbc3b76am61pgnz. Base: argon trunk with T124 (Mac T731, landed c28dee0); crates/fe-vim is byte-identical between mac/main a6fea65 and c5e93ed. Argon dep: T124 (Mac T731), whose quote path in visual/object.rs select_object and text_object.rs this builds on. Title the host commit "... (Mac T735)".

## Problem

A bracket object (`i(` `a(` `i[` `a[` `i{` `a{` `i<` `a<`, and their `)` `]` `}` `>` spellings) typed in
charwise visual mode over a selection of more than one position picks a different pair from nvim's
whenever the selection starts outside the brackets, starts on one, or crosses one. fe-vim looks for the
smallest pair around the *cursor* that holds the whole selection and is larger than it
(`crates/fe-vim/src/visual/object.rs` `select_object`, its last branch, for `(` and `{`;
`crates/fe-vim/src/delimiters.rs` `grow`, for `[` and `<`) and fails when there's none. nvim picks the
pair from the selection's *start* and doesn't require it to hold the selection.

Probed at Mac main c5e93ed6 against `nvim --clean --headless` 0.12.6 (the oracle's settings), one nvim per
case, a failing object detected with a macro (`:let @q=...` + `@q`; a failure aborts it). A differential
fuzz of 4,176 single-row cases (8 buffers with nested `()` `[]` `{}` `<>`, every start column, `v` plus
1-4 `l` or 1-3 `h`, then `i`/`a`/`2i`/`2a` or two objects in a row) differed from fe-vim in about 60% of
cases; the rules under Change fit all 4,176 with no mismatch. Selections as anchor..cursor, `(row, col)`
where a row other than 0 is involved:

| buffer | cursor | keys | nvim | fe-vim |
|---|---|---|---|---|
| `a (b c) d` | 0 | `vllli(` | 3..5 | 0..3, fails |
| `a (b c) d` | 0 | `vlllla(` | 2..6 | 0..4, fails |
| `a (b c) d` | 6 | `vli(` | 3..5 | 6..7, fails |
| `a (b c) d` | 7 | `vhhi(` | 3..5 | 7..5, fails |
| `a (b c) d` | 3 | `vlllla(` | 2..6 | 3..7, fails |
| `a (b c) d` | 4 | `vlli(` | 3..5 | 4..6, fails |
| `a (b c) d` | 2 | `vla(` | fails, 2..3 | 2..6 |
| `a (b c) d` | 3 | `vha(` | fails, 3..2 | 2..6 |
| `f(x (y z) w) q` | 6 | `va(i(` | 2..10 | 4..8, fails |
| `f(x (y z) w) q` | 0 | `va(a(` | 4..8 | 1..11, fails |
| `f(x (y z) w) q` | 0 | `vl2i(` | 5..7 | 0..1, fails |
| `(a (b) c)` | 0 | `vli(` | 4..4 | 0..1, fails |
| `a (b) (c)` | 3 | `vlllli(` | 3..3 | 3..7, fails |
| `a [b c] d` | 0 | `vlli[` | 3..5 | 0..2, fails |
| `a [b c] d` | 2 | `vla[` | fails, 2..3 | 2..6 |
| `a {b c} d` | 3 | `vhhhi{` | 3..5 | 3..0, fails |
| `a <b c> d` | 0 | `vli>` | 3..5 | 0..1, fails |
| `xy\n(a b)` | (0,0) | `vli(` | (1,1)..(1,3) | (0,0)..(0,1), fails |
| `(a\nb) (c d)` | (0,0) | `jvli(` | (0,1)..(1,0) | (1,0)..(1,1), fails |

Already the same as nvim, and must stay so: `a (b c) d` at 3 `vlllli(` (fails, 3..7), at 3 `vl2i(`
(fails, 3..4), at 4 `vlla(` (2..6); `(ab) (cd)` at 0 `vli(` (fails, 0..1); `f(x (y z) w) q` at 5 `vl2i(`
(2..10), at 6 `vi(i(` (2..10); `[[a]]` at 2 `va[a[` (0..4); `x [ab cd] y` at 4 `viwi[` (3..7).

## Change

For a bracket object typed in charwise visual mode over a selection of more than one position, with `lo`
and `hi` the selection's first and last positions (whichever way it faces), choose the pair this way, in
one new function next to `text_object::brackets` in `crates/fe-vim/src/text_object.rs` that both
callers use (`select_object` for `InnerParen | AParen | InnerBrace | ABrace`, `delimiters::grow` for
`InnerBracket | ABracket | InnerAngle | AAngle`; backticks keep `grow`'s current path):

1. The opening bracket: the `count`th unclosed one before `lo`, the char at `lo` not counted (so a
   selection starting on `(` takes the pair around that one, and one starting on `)` the pair it closes),
   through `motion::unmatched` / `nth_unmatched` with `brackets`' rules for strings and escapes. If there
   are some but fewer than `count`, fail.
2. With none before `lo`: fail if `lo` is on an opening bracket; otherwise the opening bracket `count`
   deep after `lo`, across rows, as `next_bracket` finds it (make it `pub(crate)` or call it from the new
   function).
3. Its closing bracket as `brackets` finds it. `a`: the pair, both brackets included. `i`: the inside by
   `brackets`' current rules (an opening bracket ending its row, a closing one after only blanks, whole
   rows) and `object_ends` / `delimiters::ends`' visual end positions; an empty inside fails.
4. `i` only: while the inside lies within the selection (its start at or after `lo` and its last
   position at or before `hi`) and is more than one position, step out to the unclosed opening bracket
   before the position before `lo` (that position not counted) and take that pair's inside instead; none
   fails. An inside of one position within the selection is kept.
5. The selection becomes anchor at the result's start, cursor at its last position, whichever way the
   old one faced. It needn't hold the old selection (`va(a(` from outside moves onto the inner pair).

A failure leaves the selection and the mode as they are and sets `VimState::failed`, as today.

Unchanged: a one-position selection (the object at the cursor), blockwise selections (T650), tag,
backtick, quote (T731), word, sentence and paragraph objects, and operator-pending bracket objects.

New test file `crates/fe-vim/tests/visual_bracket_selection.rs` with `HeadlessVim` cases, expectations
probed from `nvim --clean --headless` 0.12.6 as in `tests/visual_block_objects.rs` and hard-coded (no nvim
at test time): every row of both lists above, each checking `visual_anchor()`, `cursor()`, `mode()` and
`state().failed()`. Use `i>`/`a>` for angle cases (a `<lt>` in an nvim probe macro misreads).

This card's lane starts after Mac T731 lands: both change `select_object` in `visual/object.rs` and
`text_object.rs`.
