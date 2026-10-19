---
task_uid: "01m57yae867s6e0gcnppr78rdb"
task: "T198"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Each symbol crumb carries the icon symbol_icon gives its OutlineKind; path and file crumbs are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "Clicking a symbol crumb still lists its siblings"
    required: true
    manual: false
  - id: "C3"
    text: "breadcrumb_symbol_icons.rs proves both through Pane::drawn_crumbs"
    required: true
    manual: false
---

# Show each breadcrumb symbol crumb with its kind icon, as in the Outline (Mac T992)

## Problem
The pane toolbar's breadcrumbs end with the symbols enclosing the caret (crates/fe-workbench/src/pane_toolbar.rs `symbol_chain`, drawn in crates/fe-workbench/src/pane.rs `crumbs`, ~L1588-1608), but a symbol crumb is a bare name: `Crumb::new(name, CrumbKind::Symbol)` with no `.icon(..)`, while file crumbs already carry `file_icon`. VS Code shows each symbol crumb with its kind icon (user's VS Code parity notes; the user asked for this).
## Change
Keep each item's `OutlineKind` in `SymbolChain::items` (pane_toolbar.rs: `symbol_chain` returns index, name and kind) and set `.icon(crate::outline_panel::symbol_icon(kind))` on each symbol crumb in pane.rs, so the crumb shows the same icon as its Outline row (T990 adds `symbol_icon`). Clicking a symbol crumb behaves as before. `Pane::drawn_crumbs` already records each crumb's icon, so the test reads it. Test in a new crates/fe-workbench/src/tests/breadcrumb_symbol_icons.rs (declared in tests/mod.rs).
