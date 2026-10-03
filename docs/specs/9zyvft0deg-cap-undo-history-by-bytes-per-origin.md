---
task_uid: "01m417z3zw8dy1t79zyvft0deg"
task: "T13"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-buffer test tests::history_cap::history_stays_under_the_cap passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test tests::history_cap::dropped_transactions_cannot_be_undone_but_newer_ones_can passes"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test tests::history_cap::cap_is_per_origin passes"
    required: true
    manual: false
  - id: "C4"
    text: "Existing fe-buffer reference-model and invariant tests still pass (gate stage test), and scripts/soak.sh passes at 50k cases (manual: log)"
    required: true
    manual: false
---

# Cap undo history by bytes per origin

## Problem

Undo history (`Buffer::history: Vec<HistoryEntry>` in
`crates/fe-buffer/src/buffer.rs`) keeps every transaction's `AppliedBatch`es
with their `old_texts` and `new_texts` (`Vec<Arc<str>>`), forever, for every
origin. T-19a measures how fast that grows. A replace-all over a large file
holds every replaced string for the rest of the session.

## Change

Cap undo history by bytes, per origin:

- Track the bytes held by each origin's entries: the sum of the lengths of
  `old_texts` and `new_texts`, including `Undone` batches.
- After a transaction closes (`end_transaction` / `close_open_transaction`),
  drop that origin's oldest entries until it's under the cap. Never drop the
  newest entry, even if it alone exceeds the cap.
- Keep `redoable` consistent when an undone entry is dropped.
- A dropped transaction can no longer be undone or redone:
  `undo`/`redo` with that origin's filter stop at the oldest remaining entry
  and return `Ok(None)` past it.
- The cap is a `Buffer` setting, `set_history_limit(bytes)`, with a default
  taken from T-19a's numbers.
- This card does not touch the `EditLog`. Anchors and `edits_since` keep
  working for every version (log compaction is T-19c).

Tests go in a new file, `crates/fe-buffer/src/tests/history_cap.rs`,
registered in `tests/mod.rs`. Because the change touches undo invariants, run
`scripts/soak.sh` at 50k cases afterwards (TESTING.md).

## Open questions

- What should the default cap be, and does the AI origin get a different one?
  Wait for T-19a's numbers.
- Should large deleted texts be stored as rope slices instead of `Arc<str>`
  (the source card's third option)? Only if T-19a shows `old_texts` dominating
  even under a cap.

## Implementation notes

- **Default:** `Buffer::DEFAULT_HISTORY_LIMIT` = 16 MiB of text per origin, the
  same for every origin (each AI session is its own origin, as `Origin` is the
  key). T7 measured about 7.1 MB of old and new text per replace-all pass on the
  33 MB file, so the last two passes stay undoable.
- **The cap barely touches typing.** T7's long session costs about 500 B per
  single-character transaction, but only a byte or two of that is text. At
  16 MiB, typing would never reach the cap. The rest is the edit log (T14) and
  per-entry overhead (`Edit`, `Arc` headers, `Vec`s). Counting a fixed overhead
  per edit would bound that too, but it's left out because the spec defines the
  cap as text bytes.
- Trimming runs when an entry closes: on a standalone `edit`, at
  `end_transaction`/`close_open_transaction`, and when `set_history_limit`
  lowers the cap. Undo can push an origin over the cap until its next close.
  Undone entries can only be trimmed through `set_history_limit`, because a new
  edit already discards that origin's redos.
- `Buffer::history_bytes(&origin)` exposes the running count, which is kept
  incrementally.
- Soak: `scripts/soak.sh` at 50k cases passed (fe-buffer 41, fe-text 30,
  fe-display 34, fe-editor 16).
