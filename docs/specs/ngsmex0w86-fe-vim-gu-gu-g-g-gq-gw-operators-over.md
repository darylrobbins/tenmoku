---
task_uid: "01m4p2z1j3zh38cpngsmex0w86"
task: "T418"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::g_operators::case_and_rot13: gu gU g~ and g? over w, iw, j and a count lowercase, uppercase, toggle or rot13 what they cover, write no register, leave the caret at the range start and are one undo step; guu gugu gUU gUgU g~~ g~g~ g?? g?g? take count rows; dot repeats each; in v, V and ctrl-v they act on the selection"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim tests tests::g_operators::format: gq and gw (gqq, gqgq, gqj, gww) join each paragraph of the rows and break it at blanks so no row passes column 79, every row taking the first row's indent; gq leaves the caret on the last formatted row's first non-blank and gw where it was; one undo step; dot repeats"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test g_operators_matches_neovim (tests/g_operators_oracle.rs): tests/corpus/g_operators.toml has at least 45 cases (both doubled spellings, the visual modes, non-ASCII letters, long and indented rows, several paragraphs, a word longer than 79 columns, u and dot after), none using another breadth card's motions or objects; each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; = stays unbound; nothing outside src/g_operators.rs, its test file, oracle test and corpus changes, except: src/tests/feature_stubs.rs (only dropping the G_OPERATORS loop from new_variants_change_nothing, round_trips kept), src/operator.rs (only making motion_span pub(crate)), src/marks.rs (only Marks::reflowing and Place::follows_text, so marks follow their text through gq), src/visual.rs (only reflowing/reflowed, so '\u003c and '\u003e follow their text through gq), src/tests/jump.rs (only the unbound-g-key check using gZ instead of gq), and src/visual/operate.rs and src/visual/block.rs (only applying case and rot13 edits through g_operators::apply, so an unchanged visual case edit is still one undo step, as in nvim)"
    required: true
    manual: false
---

# fe-vim: gu gU g~ g? gq gw operators over motions, objects and visual selections, checked against nvim

# fe-vim: gu gU g~ g? gq gw operators

## Problem

fe-vim's case operators work only over a visual selection (`u U ~` in visual mode);
`operator::operate` refuses `Lowercase`, `Uppercase` and `ToggleCase` on a motion or object, and
`g?`, `gq`, `gw` don't exist (gap analysis 2026-10-11, `args.rs` ~55). The seam card this depends on
declared `Operator::Rot13`, `Format`, `FormatKeep`, routed every non-`d y c > <` operator on a
motion or object to `g_operators::operate`, routed visual `Rot13` to `g_operators::rot13` and visual
`Format`/`FormatKeep` to `g_operators::format_selection`, and let `g??` reach a hook.

## Change

Everything lives in `src/g_operators.rs` (`g_operators::FEATURE` already in `FEATURES`):

- `FEATURE.g` takes `u U ~ ? q w` after `g`: with no operator pending it starts that operator
  (`Pending::with_operator`); in a visual mode it applies it to the selection
  (`Pending::operate(op, Target::Selection)`); with the same operator pending (`gugu`, `gUgU`,
  `g~g~`, `g?g?`, `gqgq`, `gwgw`) it targets `count` rows (`Motion::Line`).
- `FEATURE.key` takes the doubled short forms `guu gUU g~~ g?? gqq gww` (the key `u U ~ ? q w`
  while its own operator is pending) and nothing else.
- `gu gU g~ g?` over a motion or object: lowercase, uppercase, toggle case or rot13 what it covers
  (using `edit::lowered`/`raised` as visual mode does), write no register, leave the caret at the
  range's start, one undo step, `.` repeats them (the existing `Change::Operate` path).
- `gq`/`gw` over rows (motions and objects act linewise, `:help gq`): each paragraph of the rows
  (blank rows separate) is joined and broken at blanks so no row passes column 79 (nvim with
  'textwidth' 0 in an 80-column window), every row taking the first row's indent; `gq` leaves the
  caret on the last formatted row's first non-blank, `gw` where it was. One undo step; `.` repeats.
- `g_operators::rot13` does the visual `g?`; `format_selection` formats the selection's rows.
- `src/g_operators.rs`'s module docs describe the operators.

**Clean room**: `nvim --clean` (0.12.4) and `:help gu`, `:help g?`, `:help gq`, `:help gw`,
`:help 'textwidth'`, `:help fo-table`; never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/g_operators.toml`, at least 45 cases: each of `gu gU g~ g?` with `w`, `$`, `j`, `iw`,
a count, the doubled forms (both spellings), in `v`, `V` and `<C-v>`, over non-ASCII letters, `.`
and `u` after; `gqq`, `gqj`, `gqG`, `gww` on long rows, indented rows, several paragraphs, a word
longer than 79 columns, and `.` after `gqq`. It uses no motion or object from the other breadth
cards (they land in parallel).

## Tests (new files)

- `src/tests/g_operators.rs` (the seam's stub).
- `tests/g_operators_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `g_operators_matches_neovim`.

## Size

About 300 non-test lines (formatting most of it).

## Out of scope

`=` (nvim's is C-indenting; fastedit has no language indenter: needs a decision), comment leaders
and other 'formatoptions' in `gq` (the corpus has none), a `textwidth` setting, the GPUI check. Any
change outside `src/g_operators.rs` and its tests and corpus.

## Implementation notes

Everything is in `src/g_operators.rs`, but the seam lacked a few hooks, which are outside this
card's file list (C4) and need a respec to allow:

- `src/operator.rs`: `motion_span` is `pub(crate)` (one word). `g_operators::operate` needs each
  motion's span exactly as `d` measures it (`:help exclusive`, `:help word`), and duplicating its
  90 lines would be worse.
- `src/marks.rs` (+24 lines): `Marks::reflowing` and `Place::follows_text`. nvim's `gq` moves marks
  with their text through joins *and* breaks (`'a` on `word13` of a long row goes to row 1), where
  fe-vim's marks keep their byte column on a row through in-row edits.
- `src/visual.rs` (+45 lines): `reflowing`/`reflowed`, the same for `'<` and `'>` (their `Pos`
  fields are private to `visual`).
- `src/tests/feature_stubs.rs` (-9 lines): T414's `new_variants_change_nothing` ran every g
  operator over `w` from an empty row and asserted no undo step; nvim makes one even when nothing
  changes (`xgUUu` gives back `BC`), so that loop is dropped. The g operators over the other
  breadth cards' stub motions still change nothing.
- `src/tests/jump.rs` (1 line): `run("gq") == Cancel` used `gq` as an unbound key after `g`; it is
  `gZ` now.

`gww` is not a doubled form in nvim 0.12.4: `{count}gww` is `gw{count}w` (`2gww` on `a b/c/d`
changes nothing; `2gwgw` joins two rows). With no count the two agree on every row probed, so
fe-vim's reducer already reads `gww` right (`w` is a motion) and `FEATURE.key` doesn't take it.

nvim rules fitted by probing (`nvim --clean`, see the module docs): case operators leave the caret
at `min(start, motion end)` at its byte column; an exclusive motion that stays at column 0 takes
the grapheme there on row 0 and the whole row elsewhere (`gUh`); a failed text object resets the
goal column. `gq` joins as `J` (no space before `)`, none after a trailing blank, two spaces kept),
breaks greedily at blanks to 79 display columns (wide characters count 2, tabs to the next stop),
removes the blanks at a break, keeps trailing blanks, leaves blank-only rows alone; it joins and
breaks with the first row's indent as it is, marks following the text, then rewrites indents as
spaces (expandtab), marks keeping their byte columns. The caret goes to the last formatted row's
first non-blank, or the row after when an exclusive motion ended at its column 0 (`gq/pat`,
`2gqB`, and `gq2w` over an empty row); inclusive motions take the row they end on even if empty
(`gq2$`). `u` returns to the caret before `gq`/`gw`, and to where a case operator left it.

Evidence beyond the corpus: a scratch differential fuzz (6,000 random cases of all six operators
over motions, objects, counts, doubled forms and the visual modes, with `.` and `u`, one nvim per
case) matches nvim except for: (a) pre-existing visual `u U ~` `.` caret columns (visual
`~`/`u`/`U` fail the same cases); (b) nvim's `u` restoring marks moved by a join, which fe-vim's
undo never does (`J` too); (c) three rare quirks listed in the module docs (tab-indent marks, a
`.` over wrapped rows with wide characters, one off-by-one `'<`).
