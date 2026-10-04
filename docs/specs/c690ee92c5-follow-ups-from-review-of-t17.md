---
task_uid: "01m41j2dgc0v9089c690ee92c5"
task: "T37"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T17

Findings of the review of T17 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/injection.rs:253: Empty highlights bypass layer cleanup. After highlighting an injected document, deleting all its text, and finishing the host parse, every retained layer has collapsed ranges but highlights(0..0) returns here without dropping it. Subsequent calls use the cached empty result, retaining obsolete trees indefinitely.
  fix: Run appropriate layer cleanup for empty highlighted ranges, with a new pass so previously used layers cannot survive as current. Add a regression that deletes the entire document, finishes parsing, highlights the empty buffer, and asserts that no layers remain.
  instances: crates/fe-syntax/src/injection.rs:253

- F2 medium crates/fe-syntax/src/syntax.rs:246: Cached highlights return before updating the layers' recent viewport history. Highlight eight widely separated ranges A–H, revisit cached A, then highlight I: eviction drops A and retains B, although A belongs to the last eight highlighted ranges and B does not. This retains obsolete viewport trees and loses incremental reuse for recently revisited content.
  fix: Refresh viewport recency and perform distance eviction on cache hits as well as queried highlights, without treating cached layers as disappeared. Add a regression covering revisiting a cached viewport before another viewport causes eviction.
  instances: crates/fe-syntax/src/syntax.rs:246

- F3 medium crates/fe-syntax/src/injection.rs:116: Dirtiness is determined from the layer's entire span rather than its included ranges. Editing strictly inside an excluded ${…} expression in an HTML template marks the HTML layer edited, even though none of its included bytes changed. The exact reuse path then rejects it and reparses it, contrary to the requirement that untouched layers remain unparsed.
  fix: Determine dirtiness from contact with individual included ranges while continuing to shift the tree and ranges for preceding edits. Preserve necessary invalidation at range boundaries, and add a parse-counter regression for edits strictly inside excluded interpolation content.
  instances: crates/fe-syntax/src/injection.rs:116

## Implementation notes

- F1: `injection::paint` starts a new pass and runs `Layers::highlighted` for an
  empty range too, so edited layers collapsed by deleting all text are dropped.
  Regression: `deleting_everything_drops_every_layer`.
- F2: the recency/distance part of `highlighted` is split out as
  `Layers::visited`, which `Syntax::highlights` also calls on a cache hit (no
  "gone" cleanup there). Regression: `a_cached_range_shown_again_keeps_its_layers`.
- F3: `Layers::edit` marks a layer edited only when the edit touches one of its
  included ranges (touching, so boundary edits still invalidate); the tree and
  ranges are still shifted as before. Regression:
  `an_edit_between_a_layers_ranges_reparses_nothing`.
- Each regression was confirmed to fail without its fix.
