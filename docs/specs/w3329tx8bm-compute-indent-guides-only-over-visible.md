---
task_uid: "01m4v99bqv5d4g8rw3329tx8bm"
task: "T664"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui indent_guides_fold_tests: with a 100k-row block folded inside the view, a frame's guide computation visits only visible buffer rows (counter or ranges, not wall time)"
    required: true
    manual: false
  - id: "C2"
    text: "indent_guides_fold_tests: guides around the fold match those drawn with the block unfolded; existing indent_guides_tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -D warnings pass"
    required: true
    manual: false
---

# Compute indent guides only over visible rows, skipping folded-out rows (T584 review F1)

## Problem

T584 review F1 (medium, deferred by the user's findings_override, 2026-10-13): fe-gpui's indent guide
painting (`crates/fe-gpui/src/element/indent_guides.rs:43`) computes guides every frame across the whole
buffer interval between the first and last displayed rows, including rows hidden inside folds. A large
collapsed block makes each redraw scan and allocate in proportion to its hidden contents; splitting the
resulting rectangles at fold boundaries afterwards doesn't avoid that work.

## Change

Compute guides only over the visible buffer-row ranges: ask fe-editor for the contiguous visible buffer-row
ranges of the frame (deduplicating wrapped pieces of one row), run `indent_guides` per range with its existing
bounded blank-row context, and skip folded-out rows entirely. The painted result must not change for unfolded
text.

## Tests

New `crates/fe-gpui/src/element/indent_guides_fold_tests.rs` (declared next to `indent_guides_tests` in
element.rs): with a 100k-row nested block folded inside the view, a frame's guide computation visits only
visible rows (assert on a visit counter or the ranges passed, not wall-clock time), and the guides drawn on the
rows around the fold are the same as with the block unfolded and scrolled to.
