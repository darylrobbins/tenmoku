---
task_uid: "01m4emachx47384gzmn4t7bcpx"
task: "T206"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "On three \"abcd\" rows at (0,1), \u003cC-v\u003ejjlcXY, a Down excursion typing Z, Up, Q and Esc leave the second row \"aXYQdZ\", as nvim 0.12.4 does: tests::block_insert_excursion::change_types_on_a_row_the_session_typed_on fails at base and passes."
    required: true
    manual: false
  - id: "C2"
    text: "I and A over a block type on a nonempty row the session typed on during an excursion, and A on an empty (padded) one, where nvim 0.12.4 does, each expected outcome checked against nvim 0.12.4 with its keys and result in the decision file: tests::block_insert_excursion::insert_types_on_a_row_the_session_typed_on, ::append_types_on_a_row_the_session_typed_on and ::append_types_on_an_empty_row_the_session_typed_on fail at base and pass."
    required: true
    manual: false
  - id: "C3"
    text: "Another origin's edit on a secondary row, a deleted row boundary or a join still costs that row its text: tests::block_insert_excursion::another_origins_edit_still_costs_a_row_its_text passes, and every existing tests::visual_block and tests::insert test passes unchanged."
    required: true
    manual: false
  - id: "C4"
    text: "The new row-following code lives in crates/fe-vim/src/insert/block_rows.rs (declared by mod block_rows; in insert.rs), keeping insert.rs's diff to about 100 lines; lib.rs's Known limits no longer lists Esc typing nothing on a row the session typed on."
    required: true
    manual: false
---

# fe-vim: Esc types a block insert's text on a row the session typed on during a cursor excursion, as nvim does (T174 F26)

# fe-vim: Esc types a block insert's text on a row the session typed on during a cursor excursion, as nvim does (T174 F26)

## Problem

T174's round-3 review (codex, review `01m4ed2qq3vstnkp1qj8rac675`, 2026-10-08) filed F26
(medium, follow-up) against `crates/fe-vim/src/insert.rs:215` and `:221`:

> Secondary-row validation rejects the session's own typing during a cursor excursion just as it
> rejects another origin's edits. On three "abcd" rows at (0,1), <C-v>jjlcXY<Down>Z<Up>Q<Esc>
> leaves the second row as "adZ"; Neovim produces "aXYQdZ" there. I and A share this omission,
> and both empty and nonempty secondary rows are affected.
>
> fix: Track secondary-row changes by origin, permitting the session's own typing while retaining
> protection for another origin's edits and deleted row boundaries. Add I, A and c excursion
> regressions for empty and nonempty rows.

It is still open on main `c2bbe1c`: `BlockRow::found` (insert.rs) accepts a row only if its text
still equals what it was when the block's session opened, whoever changed it, and
`crates/fe-vim/src/lib.rs` documents it under "Known limits": "After the cursor keys split a session
over a block, `Esc` types nothing on another of its rows the session typed on meanwhile (nvim types
there too, at the block's column)".

## Change

`insert.rs` is 2,136 lines, so keep its diff small (about 100 lines): put the new row-following
code in a new `crates/fe-vim/src/insert/block_rows.rs`, declared by `mod block_rows;` in
`insert.rs` (`insert.rs` stays the root beside an `insert/` directory, as T188 does for
`visual.rs`).

- Follow each secondary `BlockRow` through edits by origin, as `FirstRow::follow` follows the first
  row: the session's own edits on it (during a split excursion, `VimState::split_session`) keep it
  eligible; another origin's edit on it, a deleted row boundary, or a join still make it get
  nothing, as now (the F17 and F21 protections stay).
- At `Esc`, type the text on such a row where nvim 0.12.4 does: at the block's column. Check each
  expected outcome against nvim 0.12.4 (`/opt/homebrew/bin/nvim`, or the oracle recorder in
  `crates/fe-vim/tests/support/oracle.lua`) and put the keys and nvim's results in the decision
  file.
- Drop that clause from "Known limits" in `lib.rs` (keep the rest of the bullet: undo grouping,
  joins, line breaks) and say in the module docs that the session's own typing on a row doesn't
  cost it the text.

Tests go in a new `crates/fe-vim/src/tests/block_insert_excursion.rs`, declared by one `mod
block_insert_excursion;` line in `crates/fe-vim/src/tests/mod.rs`, driving excursions with
`HeadlessVim::split_session(Splits::Moves, …)` as `tests/visual_block.rs` does, asserting text,
cursor, and that undoing back to the start restores the original text and redo puts the result back:

- `change_types_on_a_row_the_session_typed_on`: the F26 example; the second row is `"aXYQdZ"`.
- `insert_types_on_a_row_the_session_typed_on`: `I` with a Down/type/Up excursion over a nonempty
  row, nvim's result.
- `append_types_on_a_row_the_session_typed_on`: `A` likewise.
- `append_types_on_an_empty_row_the_session_typed_on`: `A` over a block whose middle row is empty
  (padded to the column), with the session typing on that row during the excursion.
- `another_origins_edit_still_costs_a_row_its_text` (not a regression test; passes at base): the
  session and another origin both edit a secondary row during the excursion; that row gets nothing.

## Out of scope

The other known limits in that bullet (undo of `c`'s replication, joins, typed line breaks),
visual put, and any file outside the scope.
