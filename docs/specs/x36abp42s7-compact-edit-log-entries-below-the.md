---
task_uid: "01m417z414j4bp9qx36abp42s7"
task: "T14"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-buffer test tests::log_compaction::anchors_at_or_above_the_floor_resolve_as_before passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test tests::log_compaction::undo_below_the_floor_is_refused_without_changing_text passes"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/soak.sh passes at 50k cases (manual: log)"
    required: true
    manual: false
---

# Compact edit-log entries below the oldest live version

## Problem

`EditLog` (`crates/fe-buffer/src/log.rs`) keeps every batch since the buffer
was created, so an `Anchor` or `BufferSnapshot` from any version can be mapped
forward (`EditLog::map`, `mapping_batches`, `map_for_history`) and
`edits_since(v)` works for every `v`. fe-syntax (`syntax.rs:460`) and
fe-display (`display_map.rs:245`) call `edits_since`. Its memory only grows
(measured in T-19a).

## Change

Compact log entries older than a floor version, below which no live snapshot,
anchor, undo-history entry or `edits_since` caller still needs to map:

- Below the floor, `EditLog` drops entries but keeps its `Count` offset, so
  `Version` numbers don't change.
- Collapse roots (`Collapse.root`, `inverts`) that point below the floor are
  treated as unknowable. `map_for_history` returns `None` and the undo is
  refused with `UndoError::Conflict`, which leaves the text unchanged.
- History entries whose batches are below the floor are dropped (see T-19b).

How the floor is found is the open question below. The card is not ready until
it's answered.

Tests go in a new file, `crates/fe-buffer/src/tests/log_compaction.rs`. Run
`scripts/soak.sh` afterwards.

## Open questions

- **From the source card:** how is the oldest live version tracked? Either
  register live snapshots and anchors, so `Anchor` stops being a plain value,
  or refresh anchors periodically to the current version and compact below the
  oldest refresh. The second option changes what `edits_since(v)` means for
  stale callers: fe-syntax and fe-display would need a "too old, rebuild" path,
  which touches `crates/fe-syntax/src/syntax.rs` (T-05, T-27) and
  `crates/fe-display/src/display_map.rs`.
- Is this needed at all? Decide after T-19a. If history caps (T-19b) bound
  memory enough, the log alone may be small. Each entry is an `Arc<[Edit]>` of
  ranges, with no text.

## Implementation notes

**Floor tracking (the open question).** The buffer's owner declares the
floor with `Buffer::compact_log(floor)`. Nothing is registered, and anchors
are refreshed only when their holders choose to. `Anchor` stays a plain
`Copy` value, and neither fe-syntax nor fe-display changes. The owner already
knows its anchors and `edits_since` cursors. It refreshes them, then compacts
below the oldest. `BufferSnapshot::oldest_version()` exposes the floor, so a
stale caller can check it and rebuild. Nothing calls `compact_log` yet. Wiring
it into the editor (and adding a "too old, rebuild" path in fe-syntax and
fe-display) is follow-up work, and it should wait on the "is this needed"
answer from T-19a.

- `EditLog` keeps `floor`, the index of its first kept entry, and adds it to
  the tree's `Count`, so `Version` numbers don't change. Snapshots cloned
  earlier keep their own entries, because the tree is persistent.
- Mapping from below the floor (`resolve`, `resolve_many`, `map_offset`,
  `edits_since`) panics, in line with the existing panic for anchors newer
  than the snapshot. `edit_from` with a base below the floor returns
  `EditError::ForeignSnapshot`.
- `map_for_history` returns `None` when the batch that produced `from` is
  below the floor (`from <= floor`). The undo or redo is then refused with
  `UndoError::Conflict` and the text is left unchanged.
- **Deviation:** collapse roots below the floor are *not* treated as
  unknowable. A root is an identifier that every entry in its undo/redo chain
  carries. `reverted_by` compares it only against entries in the mapped
  range, and all of those are at or above the floor. That comparison is
  therefore still exact, and refusing it would turn away undos that are safe.
- `compact_log` drops each closed history entry whose *next* revert (its
  `batches`, or `undone.batches` if it's undone) holds a batch below the
  floor. An undone transaction whose original batch was compacted can still
  be redone. The open transaction is kept, and undoing it is refused with
  `Conflict`. The next compaction after it closes drops it.
