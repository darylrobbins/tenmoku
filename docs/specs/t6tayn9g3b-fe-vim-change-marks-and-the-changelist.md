---
task_uid: "01m5fv9g9kqjs51qt6tayn9g3b"
task: "T265"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration tests filter_change_marks_at_buffer_boundaries and filter_changelist_records_the_logical_position (top level, crates/fe-vim/tests/filter_marks.rs) pass: empty filters at lines 1 and at the end, empty reads above the first and below the last line leave '[ '] and the changelist as nvim 0.12.x does (probed against the local nvim, noted in the test comments); cargo nextest run -p fe-vim and clippy -p fe-vim --all-targets -D warnings pass"
    required: true
    manual: false
---

# fe-vim: change marks and the changelist after a ! filter or :r ! follow Neovim at buffer boundaries and for unchanged output (T255 review F4/F7 follow-up)

## Problem

T255 (Mac T908, vim `!` filter and `:r !`) leaves two review findings as documented limits (review round 2, y16 process):

- **F4:** the explicit change marks `'[` and `']` after a filter or `:r !` clamp to existing rows, where Neovim 0.12.4 leaves them past EOF (e.g. filtering lines 4-5 to nothing in a five-line buffer leaves `'[` and `'.` on line 4; an empty `5r !true` leaves them on line 6) or unset (an empty filter starting at line 1 and an empty `0r` leave `']` unset).
- **F7:** the changelist (`g;`, `:changes`) records the physical edit batches, not the logical filter/read position (filtering lines 2-4 with only line 3 changed records line 3, Neovim line 2); unchanged filters and empty reads add no entry where Neovim adds one.

## Change

In `crates/fe-vim/src/marks.rs` (and its tests), represent past-EOF and unset endpoints explicitly instead of clamping or saturating, and record the logical filter/read change position in the changelist for the transaction. Tests: empty filters at both buffer boundaries, empty reads above the first and below the last line, `g;`/`:changes` for partial rewrites, unchanged filters and empty reads, each checked against the local nvim.

Depends on T255.
