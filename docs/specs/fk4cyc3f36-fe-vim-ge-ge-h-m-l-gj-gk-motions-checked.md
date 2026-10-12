---
task_uid: "01m4p2ywnpsdjs9qfk4cyc3f36"
task: "T416"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::line_motions::*: ge and gE go back to the end of the count-th previous word or WORD (inclusive); _ + and - go count-1, count or count rows to the first non-blank (linewise); | goes to screen column count (exclusive); H M L go to the count-th row from the top or bottom of EditorHost::viewport(), or its middle, keeping the column as G does and setting the jump mark, with viewport() None meaning page_rows()+2 rows from row 0"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::line_motions::display_rows: with soft wrap on and a wrap width that wraps a row (model_mut().set_soft_wrap, set_wrap_width), gj and gk move by display rows keeping the display column and are charwise exclusive under d; with wrap off they move between buffer rows"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test line_motions_matches_neovim (tests/line_motions_oracle.rs): tests/corpus/line_motions.toml has at least 40 cases covering ge gE, _ + -, |, H M L with window_rows = 10 (counts, after \u003cC-e\u003e, dL, yH), gj gk and dgj on rows shorter than the window, each in v, and dot after d+; each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes, except that T416's 10 line_motions variants (word_end_backward … display_up) are removed from the move/operate loop of tests::feature_stubs::new_variants_change_nothing (their round_trips check kept). Nothing outside src/line_motions.rs, its test file, oracle test and corpus changes, except: src/tests/feature_stubs.rs (only that removal), src/commands.rs (only wrapping the vim.move, vim.operate, vim.repeat and vim.select handlers in line_motions::with_view) and src/operator.rs (only: motion_span returning Span::Move(reached) when ge gE gj or gk fails having moved the caret, as the WordBackward arm does; a gj/gk Move caret keeping the motion goal (nvim curswant); and - placing a linewise yank caret like a non-exact quote mark; operator::kept_goal and operator::keep_goals may become pub(crate)) and src/g_operators.rs (only: on its three caret-placing paths, carets whose span is a gj/gk Span::Move keep the goal via operator::kept_goal and operator::keep_goals, as operator.rs does; tests for it go in src/tests/line_motions.rs). with_view is scoped by an RAII guard that restores the previous context on exit (correct when nested and when the closure panics); it does no O(buffer) or O(rows) work per command (viewport scalars captured eagerly, display rows only via a cheap Arc/snapshot handle or lazily for gj/gk); with no view set, viewport None means page_rows()+2 rows from row 0 and gj/gk move as j/k but charwise; docs/specs/fk4cyc3f36-fe-vim-ge-ge-h-m-l-gj-gk-motions-checked.md is exempt: Regatta Spec/Respec commits and the lane own Implementation notes section"
    required: true
    manual: false
---

# fe-vim: ge gE _ + - | H M L gj gk motions, checked against nvim

# fe-vim: ge gE _ + - | H M L gj gk motions

## Problem

fe-vim lacks `ge gE _ + - | H M L gj gk` (`reducer.rs` `motion()` ~600–676; gap analysis
2026-10-11, M7 feature breadth). The seam card this depends on declared their variants
(`WordEndBackward`, `BigWordEndBackward`, `NextLineFirstNonBlank`, `PrevLineFirstNonBlank`,
`Column`, `WindowTop/Middle/Bottom`, `DisplayDown/Up`; `_` is the existing `Motion::Line`) and the
empty `src/line_motions.rs` they delegate to. T229 (a dependency of the seam) added the viewport
seam (`EditorHost::viewport()`) and `window_rows` corpus cases that `H M L` need.

## Change

The motions live in `src/line_motions.rs` (`line_motions::FEATURE` already in `FEATURES`); two
small changes outside it are allowed, described under *View context* and *Seam stub test* below:

- `FEATURE.key` takes `_ + - | H M L`, and `FEATURE.g` takes `e E j k` after `g`, in normal,
  visual and operator-pending mode, committing through `Pending::motion`.
- `ge`/`gE`: back to the end of the `count`th previous word/WORD, inclusive, across rows (an empty
  row is a word), using `motion.rs`'s word classes.
- `_`, `+`, `-`: `count - 1` rows down (`_`), `count` rows down or up, to the first non-blank,
  linewise; past the text's ends they fail as `j`/`k` do.
- `|`: to screen column `count` (default 1), clamped to the row's last char, exclusive, setting the
  goal column.
- `H`/`L`: to the `count`th row from the top/bottom of `EditorHost::viewport()`, `M` to the middle
  visible row; with `viewport()` `None`, the view is `page_rows() + 2` rows from row 0 (as T229's
  keys assume). Linewise, jumps (set `''`), and the column is kept as `G` keeps it (nvim's default
  'nostartofline').
- `gj`/`gk`: `count` display rows down/up through `model.display()` (`row_for_offset`, `row`),
  keeping the display column; exclusive and charwise (`:help gj`). With soft wrap off they move as
  `j`/`k` do, but charwise.
- `line_motions::class`/`is_jump`/`writes_numbered` as `:help` says for each.
- `src/line_motions.rs`'s module docs describe the keys.

### View context (`src/commands.rs`, approved respec)

`line_motions::motion(cursor, args, goal, tab)` sees only the text, but `H M L` need
`EditorHost::viewport()`/`page_rows()` and `gj gk` need `model.display()`. So:

- `src/line_motions.rs` adds `line_motions::with_view(target, |t| …)`, which makes the view
  available to `line_motions::motion` for the duration of the closure (e.g. in a thread-local).
- `src/commands.rs` wraps the `vim.move`, `vim.operate`, `vim.repeat` and `vim.select` handlers'
  bodies in `line_motions::with_view(target, |t| …)`. That wrapping is the only change to
  `commands.rs`.
- The context is scoped: an RAII guard sets it and restores the previous value on drop, so it is
  correct when nested and when the closure panics (no stale view leaks to a later command).
- No O(buffer) or O(rows) work per command: the viewport scalars (top row, rows) are captured
  eagerly; the display rows only through a cheap handle (an `Arc`/snapshot clone), or lazily, only
  when the motion is `gj`/`gk`.
- With no view set (a caller outside those handlers), the documented fallbacks apply: viewport
  `None` means `page_rows() + 2` rows from row 0, and with no display `gj`/`gk` move as `j`/`k` but charwise.

### Seam stub test (`src/tests/feature_stubs.rs`, approved respec)

`tests::feature_stubs::new_variants_change_nothing` asserts each new line-motion variant leaves the
caret alone, which C1 makes false. Remove T416's 10 variants (`word_end_backward` … `display_up`)
from that test's move/operate loop, keeping its `round_trips` check for them. That removal is the
only change to `feature_stubs.rs`.

**Clean room**: `nvim --clean` (0.12.4) and `:help ge`, `:help _`, `:help bar`, `:help H`,
`:help gj`, `:help 'startofline'`; never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/line_motions.toml`, at least 40 cases: `ge gE` over punctuation, across and onto
empty rows, at the text's start; `_ + -` with counts and at the ends; `|` past the row end and over
a tab and a wide char; `H M L` with `window_rows = 10` on a 30-row buffer, with counts, after
`<C-e>`, `dL`, `yH`; `gj gk` and `dgj` on rows shorter than the window width; each in `v`, and `.`
after `d+`. A key that fails is a case's last key.

## Tests (new files)

- `src/tests/line_motions.rs` (the seam's stub), including `gj`/`gk` over a wrapped row
  (`model_mut().set_soft_wrap(true)` and `set_wrap_width`), which the oracle can't check.
- `tests/line_motions_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `line_motions_matches_neovim`.

## Size

About 300 non-test lines.

## Out of scope

`<CR>` and `<BS>` as motions, `g0 g^ g$ gm g_`, the app's viewport (T230 wires the GPUI view), the
GPUI check. Any change outside `src/line_motions.rs` and its tests and corpus, except the
`commands.rs` wrapping and the `feature_stubs.rs` removal above.

## Implementation notes

Parked 2026-10-11 before any code, waiting on the respec that lets `src/commands.rs` call
`line_motions::with_view` (lead-approved: RAII thread-local that restores the previous value, no
O(rows) work per command, no view → viewport None) and lets `tests::feature_stubs` drop the
line-motion variants. Facts below are from `nvim --clean` 0.12.4 probes (tabstop 4).

**Design.** `with_view(target, motion: Option<(Motion, Option<u32>)>, f)` captures the
viewport scalars (`viewport()` or `page_rows() + 2` rows from 0), whether a visual mode is on,
and only for `gj`/`gk`, for each selection head and the visual cursor (keyed by
`Cursor::at(..).offset()`), the `DisplayRow` holding it and the one `count` display rows
away (`row_for_offset`, `row`: O(carets · log), not O(rows)). `DisplayMap` isn't `Clone`.
`vim.repeat` passes the last change's motion and count. No captured hop → gj/gk move buffer
rows as j/k do. Keys: `key` takes `_ + - | H M L` (`_` = `Motion::Line`), `g` takes `e E j k`.
class: ge gE inclusive; `_ + - H M L` linewise; `| gj gk` exclusive. is_jump: H M L.
writes_numbered: none.

**ge/gE**: per step, past the current word (crossing a row boundary ends it), then back over
blanks; an empty row is a word, a blank-only row isn't. Stops at (0,0) if it gets there. It fails
(macro stops) only when it can't move at all. Its goal is reset.

**_ + -**: `+` on the last row and `-` on the first fail; a count past the end clamps without
failing; `2_` on the last row fails (already in `Motion::Line`). They land on the first non-blank,
and a blank-only row lands on its last char.

**|**: the goal is `count - 1`, kept even when clamped (`99|j` lands at col 15 of a longer row).
The cursor goes to the grapheme covering that column (a tab or a wide char's 2nd column → that
grapheme), else the last one. It never fails. In visual mode, a goal at or past the row's width
puts the cursor at the row's end (`v99|` on "abc" → col 3), as `vertical` motions do in
visual.rs.

**H M L**: `used = min(rows, last + 1 - top)`. H → `top + min(count, used) - 1`, L →
`top + used - min(count, used)`, M → `top + (used - 1) / 2` (no count). They keep the column and
goal as G does (`$L` → row end, goal MAX), set `''` even when staying put, and never fail.
`dL` keeps the cursor's column after the delete (nostartofline).

**gj gk** (rows narrower than the window = j/k, charwise exclusive): they keep the goal. On the
first or last display row they fail; a count past the end moves as far as it can AND fails
(normal mode: moved, macro stops; `v9gj` moves; `d9gj` moves the cursor and deletes nothing:
fe-vim's operator gives Span::Failed, a divergence). In visual mode a goal past a short row's
end puts the cursor at the row's end (`vgj` onto "ab" → col 2). `dgj` from col 0 takes the row
linewise (exclusive-linewise rule, already in motion_span).
Wrapped (20-col window): screen column = curswant mod width, goal becomes the absolute column
(`3gj` from col 5 on a 62-char row → col 61, goal 65), `$` keeps MAX and goes to each
segment's last char. fe-vim plan: rel = goal − the current segment's start column; target =
target segment's start + rel, clamped to that segment's last grapheme. It diverges from nvim
only when the goal is past the segment (pixel wrap widths have no modulo).

Probing wraps headless needs `vsplit` + `vertical resize N`; `columns`/`nvim_win_set_width` on
the only window don't take. Never set the start cursor mid-multibyte char (nvim doesn't clamp it).

**As built (rev 2).** `with_view(target, viewed, f)` takes the command it runs as a
`line_motions::Viewed` (motion, count, and the operator taking it): `line_motions::moved(&args)`
for `vim.move`, `operated(&args)` for `vim.operate`, `repeated(state, args)` for `vim.repeat`,
`None` for `vim.select`. So the display rows are looked up only for `gj`/`gk`: two `DisplayRow`s
per caret, sorted by offset. Outside any `with_view` (no host), `H M L` see a `HeadlessHost`'s
view with no viewport (`page_rows() + 2` = 22 rows from row 0), and `gj gk` step buffer rows.

**Review round 1 (codex, 8 findings), as fixed.** All probed with `nvim --clean` 0.12.4:
- F1: a `ge`/`gE` step that can't move at all fails the motion; one that runs into (0,0) stops
  there and doesn't (`d9ge` from row 1 of "abc def\nxyz" deletes to the start). So on an empty
  first row, `d3ge` cancels with the caret at (0,0), as does `2ge` from "a bc" (0,3).
- F3: `d9gj y9gj c9gj` (and `gk`) past the ends change nothing, the caret going where the
  motion got, keeping its goal (`d9gjk`). `v9gj` moves and fails (stops a macro).
- Under an operator both are `Span::Move` (rev 3, `operator.rs`): `motion_span` returns it when
  `line_motions::moves_when_failing` (`ge gE gj gk`) and the caret moved, as for `b`, so every
  operator path maps the caret through other carets' edits, merges carets and sets the effect
  (`Effect::Changed` when it moved). `move_only` keeps a failed `gj`/`gk`'s goal (re-running the
  motion for it) and saves goals against the merged selections. In a visual mode a motion that
  moves but fails notes it in the view, and `with_view` fails the key.
- F2: from a visual cursor past the row's end (`v$`), the first `ge` step starts with the
  blanks, so it lands on the row's last word end (`v$ge` on "abc def" → col 6).
- F4 (wrapped, probed with `vsplit` + `vertical resize 10`/`20`): the column along the display
  row is `goal − the caret's display row's start`, capped at the display row's capacity − 1,
  and the goal becomes the landing column; a row end that cuts it short keeps it (`3gj` from
  col 5 on 62 chars at width 20 → col 61, goal 65), and `$` keeps MAX. `99|gk` on 25 chars at
  width 10 → col 19, goal 19. The capacity is measured on the first display row of the `from`
  or `to` buffer row if one wraps; with neither wrapped there's no cap (a `|` goal at or past
  the width carried onto an unwrapped row is kept, where nvim caps it).
- F6: `repeated` adopts a closing insert session first (`insert::adopt_closing`, which `.` runs
  anyway), so a clone whose `cgj` session another clone ended sees that change.
- F7: the fallback above. F8: `v_ v2_ v- v2- vM` and the other new cases are in the corpus,
  124 cases, all matching nvim.

**Review round 2 (F9–F12), as fixed.** F9: fe-display wraps at words (pixel widths, whitespace
hanging past the edge), so a display row can be narrower than the wrap width and no fe-vim code
knows that width in columns. A display row's capacity is now the widest of the display rows
within two of the caret's and of the target's in their buffer rows (`a b…b` at width 10 from
col 11: `gj` → col 21). F10–F12 went with the operator route above. Two `.` cases (`dge3.`,
`dgj9.`) are in the corpus, 126 cases, all matching nvim 0.12.4.

**Review round 3 (F9, F13, F14).** F9/F14: columns along a display row now count tab stops from
that row's start, as fe-display lays it out, and the goal carries over as "as far along the
display row as the caret, plus how far the goal is past it". The cap no longer comes from sampled
segment lengths. Each display-row break bounds the wrap width from above: before a word, it is
narrower than the row plus that word; mid-word, narrower than the row plus one character. So the
cap is the smallest bound near the caret and the target, minus 2. It never cuts a column a
display row may hold, and it is exact for rows broken mid-word (F4's examples). F13: a failed
`gj`/`gk` caret's goal is kept on operator.rs's yank, delete and shift paths too
(`operator::keep_goals`, against the merged selections). Rev 4: the g-operators (`gu gU g~ g? gq gw`, in
`g_operators.rs`) keep it on their three caret-placing paths (not resolved, recase, format) with
the same `operator::kept_goal`/`keep_goals`; nvim 0.12.4 lands `gu9gjk`, `gU9gkj`, `gq9gjk`,
`gw9gjk`, `g?9gjk`, `g~9gjk` at (1,4) and `gugj9.k` at (2,4).

**Review round 4 (F13, F14).** F13, by construction: `operator::landed_at` (what `land` does,
returning the heads it gave the model) feeds `keep_goals`. So does the very vector `move_only`, the
yank path and the g-operators' not-resolved path hand `set_selections`. A failed `gj`/`gk`'s
goal is matched to the final carets by the same values the model got, whatever `land` remapped
(another origin's edit while measuring included). F14: the goal converts between the buffer
row's columns and the display row's grapheme by grapheme (`to_along`/`from_along`). A goal
within a tab's buffer columns lands within its display-row columns, as near as they go, and one
past a grapheme carries over by as much. So a caret on a tab that fe-display draws one column
wide goes on from that column.

**Review round 5 (F14, F15).** F15: `to_along`/`from_along` called `Line::width` on an empty row
and panicked. An empty row is now as far along as the goal, and 11 corpus cases cover gj gk
into, out of and over one (normal, visual, `d`). F14: within a grapheme, columns align from its
end, so a tab's goal (its last column) is its last display-row column whether the display row
draws it narrower or wider. Known limit: a goal *inside* a tab that fe-display draws wider than
its buffer columns can't be kept. The goal is a buffer column, so from column 5 of a tab spanning
display columns 4–7 (one buffer column), `2gk2gj` comes back at 7. Accepted by the lead as a known
limit; follow-up T493 (a display-row column kept next to the goal).
