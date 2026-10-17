---
task_uid: "01m55jk1khx3asvffnttfd90he"
task: "T180"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer integration test an_edit_past_the_history_limit_allocates_no_more_with_a_longer_history (crates/fe-buffer/tests/history_trim_cost.rs) passes, and fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Buffer::trim_history makes no full pass over the history and no allocation that grows with its length: Buffer::history is a VecDeque, and only the prefix up to the last dropped entry is touched"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-buffer/benches/buffer.rs has the case 'edit/keystroke with a full undo history in 100MB'; the decision file records its base and candidate times and those of 'edit/keystroke in 100MB' (machine, commit, date, load), with the candidate's full-history case at most 3x its empty-history keystroke case"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-buffer passes with the existing history_cap, history_overhead, log_compaction, proposed_change, transaction_extend and ai_stream tests unchanged"
    required: true
    manual: false
---

# fe-buffer: an edit past the undo history limit trims in time proportional to what it drops, not to the whole history

## Problem

Once an origin's undo history passes its limit (`Buffer::DEFAULT_HISTORY_LIMIT`, 16 MiB, about 50k one-character
transactions), every edit pays O(history) in `Buffer::trim_history` (`crates/fe-buffer/src/buffer.rs`):

- it collects the indices of *every* droppable entry of the origin into a fresh `Vec<usize>` (`droppable`), a
  full pass plus an allocation that grows with the history, although it then drops only one or two of them;
- it removes those entries with `self.history.retain(..)`, a second full pass that moves every later entry
  (`history` is a `Vec<HistoryEntry>`, so dropping the oldest shifts all the rest).

`record`, `finish_transaction`, `edit_in_transaction` (via `trim_history(&origin, Some(transaction))`) all call
it after charging, so each keystroke past the limit trims one entry at the cost of the whole history.

Found by T159's worker while benchmarking (2026-10-17, helium): a single edit goes from about 3 us to about
275 us after about 50k keystrokes. T159's bench (`crates/fe-display/benches/display.rs`, lane ez74dvmwrv) works
around it with `buffer.set_history_limit(usize::MAX)` so it measures display work only. fe-buffer's own
`edit/keystroke in 100MB` bench (`crates/fe-buffer/benches/buffer.rs`) runs enough iterations to fill the
history too, so its numbers past warm-up include the trim.

## Change

Keep the semantics exactly as they are (drop the origin's oldest closed transactions until it's at or under
the limit; never its newest, the open one, the set-aside one or `keep`; fix `history_bytes` and `redoable`), and
make the cost proportional to what is dropped, not to the history:

- Make `Buffer::history` a `VecDeque<HistoryEntry>` (push_back for `push`; the few `remove(ix)` and
  index uses adapt). `compact_log` and `charge_batch`'s redo discard keep their `retain`; they are not per-edit.
- Rewrite `trim_history` without the `droppable` vector: find the origin's newest droppable entry with a
  reverse scan from the back (it is the newest, so this stops at once in practice), then walk from the front
  over the origin's droppable entries, oldest first, charging `entry_bytes` down until `held <= limit`,
  stopping before that newest one. Other origins' entries met on the way are skipped.
- Remove the dropped entries by touching only the prefix up to the last one dropped: `pop_front` while the
  front is a dropped entry, and for the rest drain that prefix and push the kept entries back to the front in
  order. Cost: the dropped entries plus other origins' entries older than them, never the whole history.
- No allocation in `trim_history` that grows with the history's length.

Regression test, in a new integration test file `crates/fe-buffer/tests/history_trim_cost.rs` (its own test
binary, against base APIs only: `Buffer::new`, `edit`, `history_bytes`, `set_history_limit`):

- A `#[global_allocator]` that wraps `std::alloc::System` and adds each allocation's size to a
  `thread_local!` `const` `Cell<usize>` (read with `try_with`, so it never allocates itself).
- One test, `an_edit_past_the_history_limit_allocates_no_more_with_a_longer_history`: measure `entry` as the
  `history_bytes(&Origin::User)` of a scratch buffer after one one-character append. For two buffers with
  limits `500 * entry` and `25_000 * entry`, append one character per edit until `history_bytes` has stayed at
  the limit for 100 edits, then record the bytes the thread allocates during each of 101 further one-character
  appends and take the median. Assert, with plain `assert!` and no custom message (so a failure at base is
  read as an assertion failure, not an error): `large_median <= 2 * small_median + 4096`. At base the large
  buffer allocates the 25,000-entry `droppable` vector on every edit (hundreds of KB) and fails.
- Discard each `edit`'s `Option<TransactionId>` with `assert!(.. .is_some())`, not `unwrap()`.

Bench evidence: add `edit/keystroke with a full undo history in 100MB` to `crates/fe-buffer/benches/buffer.rs`,
which fills the default 16 MiB history (one-character edits until `history_bytes` stops growing) before
measuring a one-character edit, and record base and candidate times for it and for `edit/keystroke in 100MB`
in the decision file (machine, commit, date, load). Not in `PERF.md` (T159 owns it now).

Leave T159's `set_history_limit(usize::MAX)` in `crates/fe-display/benches/display.rs` alone: that bench
measures display work, and with the cap lifted it stays independent of buffer bookkeeping. Don't touch that
file.
