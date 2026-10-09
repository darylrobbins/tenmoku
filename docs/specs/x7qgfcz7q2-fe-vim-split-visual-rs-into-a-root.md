---
task_uid: "01m4cnjdhy49tky9x7qgfcz7q2"
task: "T188"
rev: 2
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "null"
    required: true
    manual: false
  - id: "C2"
    text: "Pure move: git diff --color-moved=zebra --color-moved-ws=ignore-all-space shows every removed line of visual.rs as moved, and the sorted non-blank lines of main's visual.rs and of the five files' concatenation (pub(super) and pub(crate) prefixes stripped) differ only in use, mod and doc-link lines, plus rustfmt's rewrap of a signature that a pub(super) or pub(crate) prefix pushed past 100 columns (no #[rustfmt::skip]); the command and its output are in the lane's evidence; nothing is renamed and no item becomes more public than pub(crate)"
    required: true
    manual: false
  - id: "C3"
    text: "No file outside crates/fe-vim/src/visual.rs, crates/fe-vim/src/visual/ and this task's own spec docs/specs/x7qgfcz7q2-*.md (committed by the harness) changes (callers reach moved items through pub(crate) use re-exports in visual.rs); no test file, Cargo.toml or Cargo.lock changes; fe_vim's public API (Mark and VimState's visual_cursor, sync_visual, visual_anchor, select_visual and mark) is unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "null"
    required: true
    manual: false
---

# fe-vim: split visual.rs into a root module and visual/object.rs, operate.rs, block.rs and put.rs, moving code only

# fe-vim: split visual.rs into a root module and visual/object.rs, operate.rs, block.rs and put.rs, moving code only

Milestone M7 "Vim", second batch (resized), card 00: a refactor before the rest of the batch.
After T174 (blockwise visual mode) `crates/fe-vim/src/visual.rs` is about 2,150 lines, and every
later vim card that touches visual mode (search, ex, marks, increment, scrolling) would diff into
it. The lead's rule since T174: no card puts a large diff into a file over ~1,500 lines. This card
splits the file along the seams it already has, **moving code only**.

## Behaviour

None changes. No test file changes. Every existing fe-vim unit test, corpus test and fe-gpui test
passes as it is.

## The split

`visual.rs` stays the module's root (`visual.rs` beside a `visual/` directory, as fe-gpui's `vim.rs`
and `vim/` are; not `visual/mod.rs`). Items move by function; the line numbers are T174's
candidate and only a guide (move whatever main has after T174 and T182, by these seams):

| File | Items | About |
|---|---|---|
| `src/visual.rs` (root) | crate docs of the module; `mod object; mod operate; mod block; mod put;` and the `pub(crate) use` re-exports below; `Visual`, `Pos`, `Mark` (and impls), `LastVisual`, `Extent`, `Ends` (and impl); `cursor_at`, `position`, `end_of`, `goal_of`, `ends`, `select`, `leave`, `to_normal`, `held_goal`, `effect`; the mode commands `visual`, `exit`, `swap`, `reselect`, `move_cursor`; `impl VimState` (`visual_cursor`, `sync_visual`, `visual_anchor`, `select_visual`, `mark`) | 630 lines |
| `src/visual/object.rs` | text objects in a visual mode: `object_ends`, `select_object`, `inner_words`, `back_inner_words`, `back_a_words` | 250 |
| `src/visual/operate.rs` | operators over a charwise or linewise selection and `.` after them: `extent`, `extent_from`, `repeat`, `operate`, `operate_as_wide`, `Landing`, `join_landings`, `recase`, `recased` | 400 |
| `src/visual/block.rs` | block geometry and block operators: `Block`, `Cut` (and impl), `at_column`, `column_of`, `width_from`, `insertion`, `spaced`, `shift_block`, `operate_block`, `block_row`, `recased_text` | 550 |
| `src/visual/put.rs` | `p` and `P` in a visual mode and blockwise puts: `put_size`, `split`, `put_bound`, `put_block`, `put` | 330 |

An item not listed goes with its only caller, or to the root if several files use it.

### What may change besides moving

- Visibility: a moved item another visual file uses becomes `pub(super)` (or keeps `pub(crate)`).
  Nothing becomes more public than `pub(crate)`.
- `use` lines in each file, and intra-doc link paths (`[`insertion`]` → `[`block::insertion`]`) so
  rustdoc's broken-link lint stays clean.
- The root re-exports what other modules call today (`visual::operate`, `repeat`, `put`, `put_size`,
  `put_block`, `select_object`, `spaced`, and `Extent`, `LastVisual`, `Visual`, `Mark` stay in the
  root), so **no file outside `src/visual.rs` and `src/visual/` changes**: not `lib.rs`,
  `commands.rs`, `motion.rs`, `operator.rs`, `repeat.rs`, `insert.rs` or `state.rs`.
- No function body, signature (beyond visibility), comment text, or item order within a moved run
  changes. No renames.

## How the reviewer checks it is a move

`git diff --color-moved=zebra --color-moved-ws=ignore-all-space` over the change shows every
removed line of `visual.rs` as moved, except `use`/`mod` lines, visibility prefixes and doc-link
paths. A line-multiset check does the same mechanically: the sorted non-blank lines of main's
`visual.rs` and of the five new files' concatenation, with `pub(super) ` and `pub(crate) ` prefixes
stripped, differ only in `use`, `mod` and doc-comment link lines. Put the command and its output in
the lane's evidence.

## Size

About 1,520 lines move (counted as added in the new files, removed from `visual.rs`); no new logic.
This pure move is over the usual 1,000-line card budget; the user approved splitting the file
(2026-10-08), and the move is checked mechanically, as above.

## Out of scope

- Any behaviour change, rename, clean-up, new test, or change to another file. T182's fixes (the
  text objects in visual mode) land in `visual/object.rs` after this card, not here.
- Splitting `insert.rs` (also over 1,500 lines after T174): a separate card if needed.
