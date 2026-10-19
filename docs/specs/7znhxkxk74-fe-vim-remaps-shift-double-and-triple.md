---
task_uid: "01m59dj7tp984hcm7znhxkxk74"
task: "T223"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::click_prefix_tests::a_double_click_resolves_a_held_prefix_first passes: with a held remap prefix whose resolution edits the buffer, a double click selects the word at the offset computed after the edit"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui passes with no existing test edited; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# fe-vim remaps: shift, double and triple click resolve a held prefix before click offsets are computed (T218 known limit)

## Problem

T218 (Mac T874, remaps run in the editor) keeps a held remap prefix owned by vim across keys. A plain click cancels or resolves it, but shift, double and triple click paths bypass `cancel_vim`, and click offsets are computed in `crates/fe-gpui/src/view.rs` before the prefix resolves (which may edit the buffer). Fixing that is `view.rs` mouse-handler work, which T218's spec forbids (that file was held by another task).

## Change

Make every mouse path in `view.rs` resolve a held prefix the way a timeout would before computing click offsets, and compute offsets after the prefix resolves. Tests in a new file declared from a test file; no change to key handling beyond calling the same resolve step.
