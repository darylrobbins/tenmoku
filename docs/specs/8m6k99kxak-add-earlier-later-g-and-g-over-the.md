---
task_uid: "01m6rmmmvf23xj568m6k99kxak"
task: "T484"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "earlier_and_later_with_a_count_step_changes and earlier_by_seconds_uses_entry_times (flat file crates/fe-vim/tests/ex_earlier_later.rs, bare ids)"
    required: true
    manual: false
  - id: "C2"
    text: "earlier_1f_goes_to_the_previous_write and later_past_the_newest_stops_quietly"
    required: true
    manual: false
  - id: "C3"
    text: "invalid_argument_is_e475 and g_minus_and_g_plus_with_a_count"
    required: true
    manual: false
  - id: "C4"
    text: "entries_carry_a_creation_time_and_saves_are_marked (crates/fe-buffer/tests/history_times.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-vim -p fe-buffer pass"
    required: true
    manual: false
---

# Add :earlier, :later, g- and g+ over the linear undo history, by count, time and file write (Mac T1412)

Lent from Mac T1412 (lend y50, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

# Add :earlier, :later, g- and g+ over the linear undo history

## Problem
fe-vim has `u`, `<C-r>`, `:undo` and `:redo` (crates/fe-vim/src/ex/undo.rs, history.rs) but not `:earlier`/`:later` or `g-`/`g+`. `:undo {N}` for N above 0 is refused because fe-buffer's history is linear and unnumbered (undo.rs module docs). nvim users use `:earlier 10s`/`:later 1f` to step by time or by write, and `g-`/`g+` to step through the undo tree chronologically.

## Change
fe-buffer's history is not a tree: new edits after an undo drop the undone entries (checked in `Buffer::edit`/trim; confirm in the first commit, and if the undone entries survive, the divergence below shrinks). So these are the linear-history forms:
- `:earlier {N}` / `:later {N}`: N changes back or forward, the same code as N times `u` / `<C-r>` (`history::undo`), N defaults to 1.
- `:earlier {N}s|m|h|d` / `:later {N}s|m|h|d`: back (forward) to the state N seconds, minutes, hours, days earlier (later) than the current state's time. Needs a wall-clock time per history entry: add `at_unix_ms: u64` to `HistoryEntry` (buffer.rs) filled at creation, and `Buffer::history_times()` returning the applied and undone entries' times; the persisted record of the previous undo card stores it too (this card does not depend on it: the field is optional on import).
- `:earlier {N}f` / `:later {N}f`: back (forward) to the text as of the Nth previous (next) file write; `0f` the last write. Needs a mark in the history of each save (`Buffer` already knows its saved state, `saved_state.rs` test file; add the transaction id at save to a small list). `:earlier 1f` on a buffer with no unsaved changes goes to the write before the last.
- `g-` and `g+` (normal mode, with a count): in a linear history they equal `u` and `<C-r>`; module docs state that they differ from nvim only after an undo followed by a new change, where nvim can reach the abandoned branch and here it cannot. `:undolist`/`:undotree` are not added.
- Failure: going past the oldest/newest state stops there without error, as nvim does; a bad argument is `E475: Invalid argument`. The commands do not start a transaction of their own and `.` does not repeat them (as undo.rs).
- Implementation: new `crates/fe-vim/src/ex/earlier.rs` (`COMMANDS` entry through the table, `crates/fe-vim/src/ex/table.rs` one `#[path]` line and list entry; the table exists so ex.rs is untouched), `g-`/`g+` registered where `g` operators are (`crates/fe-vim/src/g_operators.rs`, check the dispatch there and in reducer.rs; if they need a `commands.rs` row include it), and the time/write queries on the buffer.

## Tests (new files)
- `crates/fe-vim/tests/ex_earlier_later.rs` (flat, bare ids): `earlier_and_later_with_a_count_step_changes`, `earlier_by_seconds_uses_entry_times` (a fake clock injected through `Buffer`'s time source), `earlier_1f_goes_to_the_previous_write`, `later_past_the_newest_stops_quietly`, `invalid_argument_is_e475`, `g_minus_and_g_plus_with_a_count`.
- `crates/fe-vim/tests/ex_earlier_oracle.rs` only for the count form against the nvim oracle harness (the `ex_undo_oracle.rs` pattern, which records with nvim 0.12.6; see `oracle-recording-nvim-version` note: record, do not download nvim). If the oracle is not available on the lane, omit this file and say so in the hand-in.
- `crates/fe-buffer/tests/history_times.rs`: `entries_carry_a_creation_time_and_saves_are_marked`.
Linux-OK (the oracle part is recorded once on the Mac and replayed).

## Implementation notes

- Confirmed linear: `Buffer::discard_redos` drops an origin's undone entries on its next change, so the forms step along one line of states per origin.
- fe-buffer: `HistoryEntry::at_unix_ms` from a clock set with `Buffer::set_clock` (system clock by default); `Buffer::history_times(filter)` lists the applied entries oldest first, then the undone ones in redo order (`HistoryTime { transaction, at_unix_ms, undone }`). Each `did_save`/`did_save_snapshot` (not a reload) is numbered and records, per origin, its newest applied transaction, the batches its transactions up to that one held, and how many of its transactions had been dropped; `Buffer::history_writes(filter)` lists, oldest first, only the saves undo/redo can still reach (`HistoryWrite { number, transaction }`), so a save is left out once a transaction up to it grows (open transaction, `edit_in`), is discarded as a redo, or history is trimmed or compacted (review rpy7z1 F2). Capped at 256 saves. No persisted undo record exists on this side yet, so nothing is stored to disk.
- The current state's time is its newest change's, or the oldest change's with everything undone (nvim takes the just-undone change's after an undo; the two differ only right after a `u`).
- `{N}f` counts writes in the order they were made, as nvim does (review rpy7z1 F1): from the write `:earlier`/`:later` last went to (`VimState::landed_write`, `crates/fe-vim/src/state.rs` added to scope), only while the state, `Buffer::history_revision` (bumped by every recorded change, undo, redo, line-ending step and drop, not by saves) and the newest write are as it left them, so a save, an undo/redo away and back (a line-ending step's included, which leaves the text version alone), or any edit since forgets it (reviews 3qzvd3 and d84qsz F4); else the newest write of the current state; else, with changes since, the newest write of an earlier state, those changes counting as one step back. `0f` goes to the write counted from. On a history without saves after undos this matched nvim 0.12.4 step for step (`earlier_1f_goes_to_the_previous_write`); with a save after an undo, nvim's tree keeps only the newer of two writes at one state and its search then jumps, so `writes_count_in_the_order_they_were_made` follows nvim where it is consistent (the reviewer's two cases) and the chronological rule after.
- `g-`/`g+` are `vim.earlier`/`vim.later` (registered in `g_operators.rs`), counted, not failing for running out; listed in which-key (`which_key.rs` added to scope). Running out is quiet, but a conflict part way (an older change overwritten by another origin) still fails and stops a `|` chain (review rpy7z1 F3; `history::steps`).
- `ex_earlier_oracle.rs` omitted: it was outside the card's scope and helium has nvim 0.12.4, not the 0.12.6 the oracle corpora record with. The count form is checked against `u`/`<C-r>` instead.
- Carried T397 F2: a reload now records its edit before clearing provenance, so undo gives the replaced text back to its authors and redo clears them; text outside the differing range (equal to the file's) stays unauthored. T397 F4: `with_authoring` restores the outer author through a drop guard; caught-panic tests cover a standalone and a nested call (`crates/fe-buffer/tests/provenance.rs` and `crates/fe-buffer/src/lib.rs` added to scope).
