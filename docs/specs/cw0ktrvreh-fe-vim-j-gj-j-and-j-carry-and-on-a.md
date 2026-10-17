---
task_uid: "01m5467997j97ndwcw0ktrvreh"
task: "T129"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test join_visual_marks::joins_carry_visual_marks_with_their_text passes: J, gJ, counted J, J twice and through ., and J after a trailing blank, before ) or onto an empty row put '\u003c and '\u003e on a joined row where nvim --clean 0.12.6 does (e.g. \"ab\\ncd\\nef\" at (0,0), vj\u003cEsc\u003ekJ gives '\u003e (0,3)); a mark in the indentation J drops goes to where the rows met; a linewise '\u003e stays on its row's last byte; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test join_visual_marks::ex_joins_carry_visual_marks_with_their_text passes: :1,2j, :%j and :%j! move '\u003c and '\u003e as nvim does (\"ab\\ncd\\nef\" at (0,0), vj\u003cEsc\u003e:1,2j\u003cCR\u003e gives '\u003e (0,3)); it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "Visual J, visual gJ and gq keep their current '\u003c and '\u003e results; cargo nextest run -p fe-vim passes with no existing test edited; cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: J, gJ, :j and :j! carry '< and '> on a joined row with their text, as nvim does (Mac T744)

Filed on argon from Mac T744 (card ~/coordination/argon-cards/zu/T744.json rev 1; note zu). Mac task uid 01m545ffqz7qk3aejhp20rnh8z. Base: argon trunk eb9c1dd (= mac/main de87d2b synced); the files it touches are the same on Mac main 9328d2ca. Title the host commit "... (Mac T744)".

## Problem

`J`, `gJ`, `:j` and `:j!` leave `'<` and `'>` on a joined row at their old byte column on whatever row
their row's start anchor now resolves to, instead of carrying them with their text as nvim does. fe-vim
already carries `'a`-`'z` through a join (marks.rs `Note::Joined`), visual `J` carries `'<`/`'>`
(crates/fe-vim/src/visual/operate.rs `operate`, its `Operator::Join | Operator::JoinRaw` arm of `moved`,
via `join_landings`/`Landing::column`), and `gq` does too (visual.rs `reflowing`/`reflowed`), but the
plain joins in crates/fe-vim/src/edit.rs `join_rows` and the `:j!` branch of crates/fe-vim/src/ex/lines.rs
`join` never touch `state.last_visual`.

Probed with `nvim --clean --headless` 0.12.6 (Mac main 9328d2ca and mac/main de87d2b5 both fail):

- `"ab\ncd\nef"` at (0,0), `vj<Esc>kJ`: nvim `'<` (0,0) `'>` (0,3); fe-vim `'>` (0,0).
- `"ab\ncd\nef"` at (1,1), `v<Esc>ggJ`: nvim both (0,4); fe-vim (0,1).
- `"ab\n  cd\nef"` at (0,0), `vjl<Esc>kJ` (`'>` in the dropped indentation): nvim `'>` (0,2); fe-vim (0,1).
- `"ab\ncd\nef"` at (0,0), `vj<Esc>kgJ`: nvim `'>` (0,2); fe-vim (0,0).
- `"ab\ncd\nef"` at (0,0), `vj<Esc>:1,2j<CR>`: nvim `'>` (0,3); fe-vim (0,0).
- `"ab\n  cd\nef\n)g"` at (0,1), `vjjj<Esc>gg:%j!<CR>`: nvim `'>` (0,9); fe-vim (0,1).

A 3,000-case differential fuzz (J/gJ/x/p/~/<C-a>/visual ops) at 9328d2ca found three more such cases
(`<C-v>jr-J`, `<C-v>j$d3J`) and no other join divergence.

## Change

Move the visual-`J` mark rule into a shared helper and call it from every join:

- crates/fe-vim/src/visual/operate.rs: add `pub(crate) fn joined_marks(state, snapshot, joins: &[(first,
  rows)], raw) -> [Option<Pos>; 2]` (for `'<`/`'>` not `past_end`, on a row `first < row <= first +
  landings`, the `Pos` on `first` at `Landing::column(pos.column)` from `join_landings(text, first, rows,
  raw)`, read before the join) and `pub(crate) fn move_marks(state, [Option<Pos>; 2])`; drop the
  `Operator::Join | Operator::JoinRaw` arm (and its `landings`) from `operate`'s `moved`, since
  `edit::join_rows` now moves them. Re-export both from crates/fe-vim/src/visual.rs.
- crates/fe-vim/src/edit.rs `join_rows`: collect each caret's `(first, rows)` join, take
  `visual::joined_marks` before `apply` and `visual::move_marks` after it. This covers `J`, `gJ`, visual
  `J`/`gJ`, `.` and `:j` (which calls `edit::join`).
- crates/fe-vim/src/ex/lines.rs `join`, the `raw` (`:j!`) branch: the same around its `apply`, with
  `raw = true` and one join `(first, last - first + 1)`.

Linewise `'<`/`'>` keep reporting column 0 and the row's last byte (marks.rs reads them that way); only
their row moves. A prototype of this (about 60 lines) passes all of `cargo nextest run -p fe-vim`.
