---
task_uid: "01m57w9f1cpyz6tj05bmfpj9xx"
task: "T981"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Clicking a header or pressing Enter/Space on a focused header toggles that view's collapse; a collapsed view is header-only"
    required: true
    manual: false
  - id: "C2"
    text: "Expanded views split the column height by weight; with every view collapsed the headers stack at the top"
    required: true
    manual: false
  - id: "C3"
    text: "save/restore round-trips collapsed and weights by ViewId, ignoring unknown ids and tolerating missing ones"
    required: true
    manual: false
  - id: "C4"
    text: "tests/view_stack.rs proves the above"
    required: true
    manual: false
---

# Add a kiln-shell ViewStack: views with collapsible headers sharing a column by weight, with saved state

## Problem
kiln-shell's `Dock` (kiln/crates/kiln-shell/src/dock.rs) shows one `DockItem` panel per rail button; a panel is one undivided column. VS Code's model has a third layer: a view container (Explorer) holds several views (Folders, Outline, Timeline) stacked in a split with collapsible headers. kiln has no stacked-views widget.
## Change
Add `ViewStack` in a new kiln/crates/kiln-shell/src/view_stack.rs, exported from lib.rs. It holds an ordered list of views, each `ViewId(&'static str)`, a title, an `AnyView` body, a collapsed flag and a size weight. Draws a header per view (chevron, title, optional right-aligned header actions) that toggles collapse on click or Enter/Space when focused; the expanded views share the column height by weight, a collapsed view is header-only, and the last expanded view cannot be collapsed away silently (all collapsed leaves headers stacked at the top). `save()`/`restore(&Value)` round-trip collapsed and weights keyed by ViewId, ignoring unknown ids and tolerating missing ones, with the same Value shape style as `DockState::save`. No drag-resize yet (next card). Tests only in kiln/crates/kiln-shell/tests/view_stack.rs.

## Implementation notes
- "The last expanded view cannot be collapsed away silently" is read as: collapsing the last expanded view leaves its header in place with the others stacked at the top and the rest of the column empty, as VS Code does; it is never refused or removed.
- An expanded view's section has flex basis = header height and flex-grow = its weight over the expanded views' total (summed in f64), so the *bodies* fill what the headers leave by weight even when the weights total under 1.
- Saved shape: `{"views": {"<id>": {"collapsed": bool, "weight": number}}}`. Restore ignores unknown ids, leaves unsaved views and missing fields as they are, and rejects weights that are not positive and finite.
- Headers are focus tab stops; a click focuses the header. Enter/Space go through GPUI's own keyboard click on the focused header (fired on key-up), so one press toggles once and keys on a focused header action don't reach it. A mouse press on header actions stops propagation so it doesn't toggle the view.
- Every mutating method takes `&mut Context<ViewStack>` and notifies.
