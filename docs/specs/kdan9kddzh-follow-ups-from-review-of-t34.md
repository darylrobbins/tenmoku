---
task_uid: "01m4268ea611c2wakdan9kddzh"
task: "T49"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T34

Findings of the review of T34 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/injection.rs:412: T34's F3 remains partially unresolved. Sorting, intersection, overlap scanning, kept-layer lookup/range comparison, included-range installation, and range cloning still have neither cancellation nor a range/layer-count cap. Being linear in ranges collected before expiry does not keep these subsequent operations within the remaining shared deadline; kept-layer lookup can also run after find has already detected expiry. Additionally, already-combined content nodes bypass deadline checks at line 535. The new test counts checks in find, so it cannot detect bulk work between checks or exercise layer installation.
  fix: Make preparation scans and comparisons cancellable, and impose explicit size bounds before bulk sorting, allocation, cloning, hashing growth, and tree-sitter range installation. Check the deadline at the start of every content-node iteration, including duplicate-parent skips, and propagate cancellation as incomplete. Extend coverage through the full preparation/layer path, including expiry after collection and reuse of an existing large layer.
  instances: crates/fe-syntax/src/injection.rs:412, crates/fe-syntax/src/injection.rs:414, crates/fe-syntax/src/injection.rs:416, crates/fe-syntax/src/injection.rs:440, crates/fe-syntax/src/injection.rs:448, crates/fe-syntax/src/injection.rs:487, crates/fe-syntax/src/injection.rs:501, crates/fe-syntax/src/injection.rs:535

## Implementation notes

- Explicit size bounds, so what runs between deadline checks is bounded
  whatever the clock does: an injection with over `MAX_LAYER_RANGES` (16 Ki)
  ranges stays plain (counted complete, as it would be every time); a tree's
  matches stop at `MAX_INJECTIONS` (4096), bounding the matches collected and
  the combined parents hashed; at most `MAX_KEPT_LAYERS` (1024) layers are
  kept, and a layer parsed past that is used but not kept.
- Deadline checks: at the start of every content node in `match_ranges`
  (duplicate combined parents included), at each range in `intersect`, at
  each kept layer looked through for reuse, and before each injection's layer
  in `Painter::paint`, so nothing is looked up once `find` has stopped. The
  overlap scan is now a binary search. Every stop marks the result incomplete.
- `painting_stops_at_the_deadline` runs the whole `paint` (find, lookup,
  range installation, parse, layer highlight) with the deadline passing at
  ~100 points, for a 2001-range layer parsed afresh, kept, and reparsed after
  an edit, asserting it stops within 3 checks. Two more tests cover the caps.
