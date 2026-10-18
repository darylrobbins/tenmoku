---
task_uid: "01m57yae2zt1312fvw8xt7mgdh"
task: "T196"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Each OutlineKind maps to a bundled icon"
    required: true
    manual: false
  - id: "C2"
    text: "Outline rows carry the icon for their symbol's kind, proved in outline_kind_icons.rs"
    required: true
    manual: false
---

# Show a kind icon before each symbol in the Outline panel (Mac T990)

## Problem
The Outline panel draws every symbol as a bare name: `render_rows` in crates/fe-workbench/src/outline_panel.rs:301-317 builds `TreeRow::new(.., item.name.clone(), ..).depth(..)` with no `.icon(..)`, though `TreeRow::icon` exists (kiln/crates/kiln-ui/src/tree_row.rs:96) and `OutlineItem.kind` (crates/fe-syntax/src/outline.rs:40-65) already says function, struct, enum, field, etc. VS Code shows a kind icon before each symbol (user's VS Code parity notes).
## Change
Add bundled lucide icons for the kinds to the `icons!` list in kiln/crates/kiln-ui/src/file_icons.rs (SVGs under kiln/crates/kiln-ui/assets/icons/): one each for module, class, struct, enum, interface/trait, impl, type, function/method, constant, macro, table, heading, field, variant, variable. Add `pub(crate) fn symbol_icon(kind: OutlineKind) -> Icon` in outline_panel.rs (the breadcrumb's symbol crumbs reuse it in a later card) and set `.icon(symbol_icon(item.kind))` on each row. Heading levels share one icon. No other panel changes, so T984 can build on it.
