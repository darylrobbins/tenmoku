---
task_uid: "01m4rbpzjebgavg7mx05vwabdt"
task: "T47"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "marker_set proptest: with up to 200 random markers and 30 random edit batches, synced after each batch or after several, every sync leaves iter_in(0..len) listing each marker with the ends BufferSnapshot::map_offset gives its inserted ends under the set's biases (an end before its start raised to it), in start then insertion order"
    required: true
    manual: false
  - id: "C2"
    text: "marker_set: iter_in(range) returns exactly the markers a naive filter finds overlapping range, empty markers at either end included, for random ranges over random sets"
    required: true
    manual: false
  - id: "C3"
    text: "marker_set: splice, retain, remove and clear keep iter_in agreeing with the naive model, and MarkerIds stay unique"
    required: true
    manual: false
  - id: "C4"
    text: "marker_set: sync from a version below oldest_version returns MarkerError::Compacted and leaves the set unchanged; another buffer's snapshot returns MarkerError::OtherBuffer"
    required: true
    manual: false
  - id: "C5"
    text: "cargo bench -p fe-buffer --bench markers runs both 100,000-marker cases, and the spec's Implementation notes record both medians with MarkerSet::sync the faster"
    required: true
    manual: false
---

# Add MarkerSet to fe-buffer: anchored ranges with values in a sum_tree that follow edits in O(log n) and answer range queries (Mac T487)

Filed on argon from Mac T487 (card ~/coordination/argon-cards/T487.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12 note k (relayed by the user).md). Mac task uid 01m4rbfz8tvmajgj8hv5vewzd9. Title the host commit "... (Mac T487)".

## Problem

The approved plan (~/.claude/plans/glittery-foraging-graham.md, crate table) gives `fe-buffer`
"`Anchor`s plus a `MarkerSet` on sum_tree", and its reuse map lists sum_tree for "Anchors, markers,
diagnostics trees". Only the anchors exist. `Anchor` (crates/fe-buffer/src/snapshot.rs:30) is an offset at a
version, mapped through the edit log on every resolve, and every anchored collection is a sorted `Vec` that
remaps all of its entries after each edit:

- `DecorationSet` (crates/fe-editor/src/decorations.rs:166): entries sorted by start with a running
  `max_end`, re-resolved through `BufferSnapshot::resolve_many` (snapshot.rs:126);
- `DiagnosticSet` (crates/fe-editor/src/diagnostics.rs:120), refreshed the same way.

Both cost O(n) per catch-up whatever the edit touched. A file with tens of thousands of markers (search
matches, diagnostics on generated code, an AI's many hunks) pays that on every keystroke. sum_tree is already
a dependency (crates/fe-buffer/Cargo.toml, used by crates/fe-buffer/src/log.rs).

## Change

Add `MarkerSet<T>` in a new crates/fe-buffer/src/marker.rs, exported from crates/fe-buffer/src/lib.rs with
`MarkerId` and `MarkerError`. Headless, GPUI-free; no existing type changes and no caller moves to it in this
card (moving `DecorationSet` and `DiagnosticSet` onto it is a later card).

- Storage: a `sum_tree::SumTree` of items sorted by start, each storing its start as the gap from the previous
  item's start and its end as a length, so an edit shifts everything after it by changing one gap. The
  summary carries the item count, the total of the gaps, and the greatest end relative to the subtree's start,
  so a range query skips subtrees that end before it.
- `MarkerSet::new(snapshot: &BufferSnapshot)` uses start bias `Bias::Right` and end bias `Bias::Left` (inside
  neither, as decorations.rs:147 does); `MarkerSet::with_biases(snapshot, start, end)` sets them. The set
  remembers the snapshot's `BufferId` and `Version`.
- `insert(range, value) -> MarkerId`: `range` is bytes of the text at `self.version()`, clipped to it; an
  inverted range is stored empty at its start. Markers with the same start keep insertion order. `MarkerId`s
  are unique within the set.
- `sync(&mut self, snapshot) -> Result<(), MarkerError>` applies `snapshot.edits_since(self.version())` batch
  by batch: markers wholly before an edit are untouched, markers after it shift by one gap change, and each end
  inside or at an edit maps as `BufferSnapshot::map_offset` maps it with that end's bias; an end that would
  fall before its start becomes the start. Work is O((k + 1) log n) per edit for k markers that touch it.
  `MarkerError::Compacted` when `self.version()` is below `snapshot.oldest_version()` (the set is left
  unchanged so the caller rebuilds) and `MarkerError::OtherBuffer` for another buffer's snapshot.
- `iter_in(range) -> impl Iterator<Item = (MarkerId, Range<usize>, &T)>`: every marker overlapping `range`,
  an empty one at either end included (as `DecorationSet` queries count them), in start order then insertion
  order, in O(log n + hits).
- `splice(range, items)` removes the markers starting in `range` and inserts `items`; `retain(f)`, `remove(id)`
  (O(n), documented), `clear`, `len`, `is_empty`, `version`.

Tests go in a new crates/fe-buffer/tests/marker_set.rs (proptest is already a dev-dependency). Add a criterion
bench crates/fe-buffer/benches/markers.rs, registered in crates/fe-buffer/Cargo.toml, with two cases on
100,000 markers spread over a 1 MiB text: `MarkerSet::sync` after a one-byte insert at offset 0, and
`resolve_many` of the same 100,000 anchors after the same edit. Record both medians in the spec's
`## Implementation notes`.

## Implementation notes

- Bench medians (`cargo bench -p fe-buffer --bench markers`, release, x86_64 Linux, 12 cores), 100,000
  markers of 4 bytes spread evenly over 1 MiB, after a one-byte insert at offset 0:
  - `MarkerSet::sync`: **6.6 µs** (the set is cloned outside the timed part; the clone shares the tree, so
    the sync also pays for copying the nodes it changes);
  - `BufferSnapshot::resolve_many` of 100,000 anchors (one per marker, at its start): **1.28 ms**.
  `MarkerSet::sync` is about 190 times faster, and a `DecorationSet` resolves two anchors per entry.
- `sync` maps through the batches `BufferSnapshot::map_offset` maps through (the edit log's
  `mapping_batches`), not raw `edits_since`: a batch followed by its own undo is skipped as a pair, which keeps
  C1's "ends as `map_offset` gives them" true when an undo lands between syncs. A set synced between the edit
  and its undo moves its markers through both, as a refreshed `Anchor` would.
- Each batch: the markers touching an edit (start at or before its old end, end at or after its old start)
  are found with the summary's `max_end`, taken out and remapped with the crate's `map_offset`; the rest are
  re-joined run by run with one gap change per run, then the remapped ones are merged back by (start, id).
  So markers that collapse onto one start come back in insertion (id) order.
- Range queries walk the tree with `Cursor::search_forward`, whose filter only sees subtree summaries; the
  iterator tracks the cursor's position alongside the filter so a subtree's relative `max_end` can be compared
  with the query start. Each hit costs O(log n) at worst, consecutive hits much less.
- `MarkerError::Older` was added for a snapshot older than the set (`edits_since` would panic there).
- `splice` removes markers whose start is in the half-open `range`, so an empty range removes none.
- `T: Clone` is required: sum_tree items are `Clone`, and remapped markers are cloned out of the tree.
