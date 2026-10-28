---
task_uid: "01m5qphzqhmkcff8t7jreczpwz"
task: "T234"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test blank_lines_match_neovim (top level, crates/fe-vim/tests/line_edits_oracle.rs): the recorded nvim corpus for [\u003cSpace\u003e and ]\u003cSpace\u003e, plain and counted, at the first, last and only row and in visual mode, agrees with fe-vim on buffer, cursor and marks"
    required: true
    manual: false
  - id: "C2"
    text: "line_edits::exchange_moves_the_row: ]e and [e (counted) move the caret's row down or up carrying the caret, marks and the changelist with it, as one undo step, and . repeats; the edges clamp as vim-unimpaired does; u and \u003cC-r\u003e restore a-z, '\u003c and '\u003e as nvim's undo does after :move (undo_puts_named_and_visual_marks_back)"
    required: true
    manual: false
  - id: "C3"
    text: "line_edits::visual_line_moves_the_selection: [e and ]e in V move the selected rows and keep the selection; multi-caret rows move together"
    required: true
    manual: false
  - id: "C4"
    text: "line_edits::operators_cancel: d]e and d[\u003cSpace\u003e cancel without editing, and an empty buffer behaves (] \u003cSpace\u003e adds a line, ]e fails)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim and cargo clippy -p fe-vim --all-targets pass"
    required: true
    manual: false
---

# fe-vim: [<Space> ]<Space> add blank lines (nvim 0.11 defaults) and [e ]e move the line up or down (vim-unimpaired), with counts and undo (Mac T1164)

## Problem

The user asked on 2026-10-24 for the unimpaired pairs beyond `[d ]d`: `[<Space> ]<Space>` (blank lines) and `[e ]e` (move line). Neovim 0.11 ships `[<Space>` and `]<Space>` as defaults ("add [count] blank lines above/below the cursor"); `[e` and `]e` are tpope's vim-unimpaired (exchange the line with the one above or below), which LazyVim users expect through mini.move.

## Current state

`[`/`]` keys go to feature hooks (`Feature::bracket`, `crates/fe-vim/src/feature.rs:142`); `crates/fe-vim/src/feature/exchange.rs` and `put_keys.rs` (`]p`, `[p`, `crates/fe-vim/src/put_keys.rs:59`) are the models for a bracket key that edits. Edits go through `operator::apply_steps`; fe-buffer drops edits that change nothing, so a command that must be an undo step still has to make one (memory fe-vim-oracle-facts, T423). Marks and the changelist must follow text moved by an edit.

## Change

New `crates/fe-vim/src/feature/line_edits.rs` (listed in `FEATURES`): the bracket hook takes `<Space>` and `e`:

- `]<Space>` / `[<Space>`: insert `count` empty lines below / above the caret's row (visual mode: the cursor row), the caret stays on its row and column, one undo step, `.` repeats; marks below shift as for any inserted rows.
- `]e` / `[e`: exchange the caret's row with the next/previous row `count` times (the row moves down/up `count` rows), the caret goes with its row at the same column, a jump-free change; at the buffer edge it clamps like unimpaired (moves as far as it can; none left: fails). Visual line selection moves the selected rows and keeps the selection (`[e` in `V`).
- Charwise `e`/`<Space>` after an operator is not an object here: `d]e` cancels.

## Tests

New `crates/fe-vim/tests/line_edits.rs`: both families, counts, first/last row, empty buffer, undo/redo as one step, `.`, marks and the changelist after a move, multi-caret, visual `V`. `[<Space>`/`]<Space>` get an nvim oracle: new `tests/line_edits_oracle.rs` + `tests/corpus/line_edits.toml` + `.expected.json` recorded with the pinned nvim (the replay needs none); `[e`/`]e` have none (not nvim) and are checked against vim-unimpaired's documented behaviour in the unit tests.

## Implementation notes

- nvim 0.12.4 maps `[<Space>` and `]<Space>` in normal mode only (`_core/defaults.lua`, a `g@l` whose operatorfunc calls `nvim_buf_set_lines`). In a visual mode `[` then `<Space>` beeps and keeps the selection, so fe-vim does the same rather than acting on the cursor row; the corpus records both (`space_below_in_visual`, `space_above_in_visual_line`). `'[` and `']` are where the caret was and `'.` and the changelist's entry on the first row put in, as nvim leaves them; `u` and `<C-r>` put the caret back at its row and column, as nvim's do.
- `]e` and `[e` at the edge move as far as they can, as the spec asks (and mini.move does). vim-unimpaired itself runs `:move +{count}` / `:move --{count}` under `silent!`, and nvim's `:move` refuses a destination past the buffer (E16), so unimpaired does nothing there: checked with nvim's `:move +5` from line 2 of 4. With no row left at all both fail.
- The move rewrites the rows it passes in place, and before the edit `Marks::moving` places every mark, jumplist and changelist entry on its row's new row (`visual::moving_rows` does `'<` and `'>`), as nvim's `:move` carries them (checked: a mark on the moved line follows it). `'[`/`']` go on the moved rows and `'.` and the new changelist entry on the first row changed, as nvim's `:move` leaves them. fe-vim's own `:m` still leaves marks on their rows (unchanged here).
- In a visual mode the selection's rows move and the selection stays on them (unimpaired's mapping leaves visual mode; mini.move keeps it). `.` after a visual move repeats over as many rows from the caret (`MoveRowsArgs::rows`). With several carets their rows move together by the same amount, as far as the one nearest the edge can go.
- `.` needed `Change::BlankLines` and `Change::MoveRows` in `repeat.rs`, the marks hooks live in `marks.rs` and `visual.rs`, and `which_key.rs` titles the new keys (its table test requires every key the reducer takes): small edits outside the listed scope.
- Undo and marks: nvim's undo header saves `a`–`z` and the visual extent with each change and `u`/`<C-r>` swap them back (`u_undoredo`). For `]e`/`[e`, `state.moves` keeps, by transaction, the anchored `a`–`z` and `'<`/`'>` from before the change's first move (in a visual mode, the extent leaving it would set, as unimpaired's `:` sets it), and `history::undo` swaps them in (`line_edits::reverted`), keeping for the step back the marks captured just before each `u`/`<C-r>` step (`line_edits::outgoing`, so a row the step deletes doesn't lose them), so marks set since, a replaced visual extent, and a move grouped with other row edits (`:normal [ ]e`) all come back as in nvim. Jumplist and changelist entries stay where the move took them, as in nvim. fe-vim has no such save for other commands (`madd u`): T255, filed from review.
- A move of rows with the same text (empty rows) makes the hidden undo step `g_operators::apply_noted` makes. `'.` and the changelist entry are set explicitly at the first row the move spans (`marks::moved`), since edits skip rows whose text doesn't change. `.` after a visual move keeps its row count on later dots. `[<Space>` past `MAX_PUT` bytes fails. `]<Space>` keeps `'<`/`'>` on an empty row, re-anchoring them once the rows are in so a later `[<Space>` moves them (oracle cases `space_*_visual_marks*`).
