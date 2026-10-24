---
task_uid: "01m5p43gyt5jwt11qfk83wexkq"
task: "T242"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "EditorView::set_overlay(Option\u003cAnyView\u003e, Range\u003cusize\u003e) draws the overlay (deferred, anchored) at the text area's left edge, just below the anchor range's last row, clamped into the editor; one overlay per editor, a second replaces the first, None removes it. Tests overlay_is_drawn_under_the_anchor_row, a_second_overlay_replaces_the_first, none_removes_the_overlay"
    required: true
    manual: false
  - id: "C2"
    text: "The overlay follows its anchor through edits above it and is hidden while the anchor row is scrolled out of view. Tests overlay_follows_edits_above_the_anchor, overlay_hides_when_scrolled_out_of_view"
    required: true
    manual: false
  - id: "C3"
    text: "The overlay keeps focus and mouse events on itself; EditorView::overlay_anchor_row() returns the anchor's row for tests. Headless GPUI, Linux-OK, no golden, no new dependency."
    required: true
    manual: false
---

# Add an anchored overlay to the editor view: EditorView::set_overlay(Option<AnyView>, anchor range) drawn under the anchor's last row, for the Inline Edit input (split from Mac T1103)

## Problem
Inline Edit with AI (T234, Mac T1103) puts a one-line input under the selection's last row. T234's worker found that no overlay hook exists: `EditorView` (crates/fe-gpui/src/view.rs) has no child/overlay slot and no pixel geometry for a row, and the only renderers are `EditorView`/`element.rs`. This card adds the hook so T234 can stay in the workbench.

## Change
- `EditorView::set_overlay(Option<AnyView>, anchor: Range<usize>)` (and `clear_overlay`): an overlay view anchored to a byte range of the buffer. The anchor is the buffer range (it follows edits the way other anchored things in the view do; use the existing anchor type the view uses for e.g. signature help or find, do not invent a new one).
- Paint: in `element.rs`, draw the overlay with gpui `deferred(anchored(...))`, as `anchor_signature_help` does, with its top-left at the left edge of the editor text area and the bottom of the anchor range's LAST row (so it sits just below the selection), clamped into the editor's bounds; when the anchor row is scrolled out of view the overlay is hidden (not removed). One overlay per editor; setting another replaces it.
- Focus is the overlay view's own; the editor view does not steal it back while an overlay is set. Mouse events on the overlay do not reach the editor.
- A way for the owner to know the overlay's row: `EditorView::overlay_anchor_row(&self) -> Option<usize>` for tests.

## Tests
New `crates/fe-gpui/src/view/overlay_tests.rs` (declared from view.rs under `#[cfg(test)]` with `#[path]` as its other test files; check how the neighbours declare theirs). Headless GPUI (`VisualTestContext`), Linux-OK:
- an overlay with a known element id is drawn: `cx.debug_bounds(id)` is Some, its top is at or below the anchor's last row's bottom and its left is at the text area's left;
- editing text above the anchor moves the overlay down with the anchored rows;
- scrolling the anchor row out of view hides it, scrolling back shows it;
- `set_overlay(None, ..)` removes it; a second `set_overlay` replaces the first.
No golden images. No new dependency.
