---
task_uid: "01m4t4yqpbsf631fdna5vnwf14"
task: "T100"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer test history_overhead::one_character_transactions_are_capped_by_their_bookkeeping: on Buffer::new(\"\") with set_history_limit(64 KiB), 20,000 one-character Origin::User transactions keep history_bytes(User) \u003c= 64 KiB after each, and fewer than 2,000 of them are undoable (undo returns Ok(None) before the 2,000th)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test history_overhead::charge_grows_with_edits: one batch of 100 one-character edits adds more to history_bytes than one batch of a single 100-character edit"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test history_overhead::default_limit_keeps_typing_undoable: under DEFAULT_HISTORY_LIMIT, after 10,001 one-character user transactions, undo succeeds 10,000 times and leaves the first transaction's text"
    required: true
    manual: false
  - id: "C4"
    text: "The history_cap and log_compaction tests still check what they check today (cap respected, newest entry kept, undone batches counted, lowering the limit trims at once, compaction frees bytes), using the new accounting; the docs on DEFAULT_HISTORY_LIMIT, set_history_limit and history_bytes say that bookkeeping is counted"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-buffer and cargo clippy -p fe-buffer --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Charge each undo-history batch for its bookkeeping as well as its text, so the history cap bounds a long typing session's memory (Mac T632)

## Problem

The undo history cap (`Buffer::set_history_limit`, default `Buffer::DEFAULT_HISTORY_LIMIT` = 16 MiB per
origin, `crates/fe-buffer/src/buffer.rs:314-320`) counts only the old and new text of each batch
(`batch_bytes`, buffer.rs:1223-1230). Each batch also holds an `AppliedBatch` (version, edits, two
`Vec<Arc<str>>`, collapse list) and each transaction a `HistoryEntry`, so a one-character edit is
charged 1 byte while it holds a few hundred. The doc comment says the cap "never limits ordinary
typing": it can't, because typing never reaches it.

Measured 2026-10-13 (release, this Mac, RSS), the `examples/history_memory.rs` long-session shape
(1 MB file, 200,000 one-character user transactions, every tenth a deletion):

- default limit: +100.3 MB, 501 B per transaction, `history_bytes(User)` = 200,000;
- `set_history_limit(0)` (history trimmed to the newest entry, the edit log alone): +39.1 MB, 195 B.

So the history holds about 306 B per one-character transaction against a 1-byte charge. The default
cap is reached only after about 16.7 million such transactions, roughly 5 GB of history per origin:
the cap doesn't bound memory for the most common edit there is. (The edit log's ~195 B per batch is
separate: it is freed only by `Buffer::compact_log`, which this card doesn't change.)

T13, which added the cap, saw this and left it out only because its spec defined the cap as text
bytes (`docs/specs/9zyvft0deg-cap-undo-history-by-bytes-per-origin.md`, Implementation notes:
"Counting a fixed overhead per edit would bound that too, but it's left out because the spec
defines the cap as text bytes").

## Change

In `crates/fe-buffer/src/buffer.rs`, charge each history batch its bookkeeping as well as its text:
a fixed cost per batch and per edit, derived from the sizes of the structures it holds
(`std::mem::size_of` of `AppliedBatch`, `Edit`, `Arc<str>` and so on, not a guess), so
`history_bytes` approximates the memory the history really holds and `set_history_limit` bounds it.
Undone entries keep counting both their batch lists, as today.

- Keep `DEFAULT_HISTORY_LIMIT` at 16 MiB unless the measurement below shows ordinary typing losing
  depth; it must still keep at least 10,000 one-character transactions undoable.
- Rewrite the `DEFAULT_HISTORY_LIMIT`, `set_history_limit` and `history_bytes` docs to say what is
  counted. Update the release numbers in `examples/history_memory.rs`'s header if you re-run it.
- Update the existing tests that assert exact `history_bytes` values
  (`crates/fe-buffer/src/tests/history_cap.rs`, `crates/fe-buffer/src/tests/log_compaction.rs`) to
  the new accounting, keeping what each one checks (cap respected, newest kept, undone counted,
  lowering the limit trims at once).

## Tests

New file `crates/fe-buffer/tests/history_overhead.rs` (public API only).
