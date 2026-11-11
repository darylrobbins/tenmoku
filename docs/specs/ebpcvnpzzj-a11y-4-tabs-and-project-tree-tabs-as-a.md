---
task_uid: "01m75959j7d66nfxebpcvnpzzj"
task: "T581"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tabs are radio buttons in a tab group with the active tab selected and the dirty state in the label, and project tree rows are rows of an outline with level and expanded state, as section 6 specifies"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in kiln/crates/kiln-ui/src/tests/tabs.rs, kiln/crates/kiln-ui/src/tests/tree_row.rs and crates/fe-workbench/tests/a11y_tabs_tree.rs assert those roles, labels and states, including after switching tabs, editing a tab dirty and expanding a folder; all pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-4 Tabs and project tree: tabs as a selectable tab group with dirty state, tree rows as an outline with level and expansion (T294 design) (Mac T1670)

# A11Y-4 Tabs and project tree

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-4 Tabs and project tree** row of section 11 "Proposed card breakdown", and section 6 "Chrome semantics".
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

- **The a11y kit** (`kiln-ui/src/a11y.rs`, added to scope): `States` gains
  `description`, `level`, `position_in_set` and `size_of_set`, which
  `node_with` sets and the test recorder records.
- **Tabs** (`kiln-ui/src/tab_bar.rs`): `TabBar` is a horizontal `TabList`
  "Tabs"; each `Tab` is a `Tab` with `selected` = active and its place in
  the strip (`Tab::in_set`, set by the bar). Its close button is the
  `Button` "Close name" with an id of its own (`NamedChild(tab, "close")`).
  Kiln-shell's and the Workbench's panes build their tabs through
  `TabBar`, so they need no change.
- **Dirty state in the label as well as the description.** Section 6 gives
  the label as "name" and the description as "modified, pinned, preview";
  this card's C1 asks for the dirty state in the label. accesskit_macos
  0.26.3 gives a node's description as `accessibilityHelp`
  (`node.rs:519`), which VoiceOver speaks only after a delay with hints on,
  so a description alone wouldn't reliably say a tab is unsaved. The label
  is "name" (then the folder the strip shows, for same-named tabs), then
  ", modified" when dirty; the description lists "modified", "pinned",
  "preview" as section 6 says.
- **Tree rows** (`kiln-ui/src/tree_row.rs`): every `TreeRow` is a
  `TreeItem` with `level` = depth + 1, `expanded` for a directory,
  `selected`, an optional `in_set` and `a11y_description`; a selected row
  claims `aria_active_descendant`, which GPUI honours only under a focused
  ancestor node.
- **Project tree** (`fe-workbench/src/project_panel.rs`): the panel's
  focus-tracking root is the `Tree` "Files" (the `uniform_list` holds no
  node, so the rows are its items), so the selected row is the focused
  tree's active descendant. Position and size come from the model,
  `FileTree::children` of the row's folder, not from the drawn rows. Git
  statuses are described as words ("modified", "added", …).
- Outline and hierarchy panels' rows become `TreeItem`s too, through
  `TreeRow`; their lists' `Tree` nodes are out of this card's scope.
- **Tests**: `kiln-ui/src/tests/tabs.rs` (`tests::tabs::a11y::*`),
  `kiln-ui/src/tests/tree_row.rs` (`tests::tree_row::a11y::*`), and
  `fe-workbench/tests/a11y_tabs_tree.rs`, which switches tabs, edits one
  dirty and expands a folder in a real Workbench.

### Section 12 human checks for this card (A11Y-11's VoiceOver pass)

- **H1′. Inspector, chrome.**
  - Tabs are radio buttons in an `AXTabGroup` with the selected one checked.
  - Tree rows are `AXRow`/`AXOutlineRow` with disclosure level.
  - Every button has a title.
- **H11a′. Focus and focus ring, keyboard only.** (its "list and tree rows")
  - Only the surfaces that take focus are checked: a toast after "Toast: Focus" (Tab between its buttons, Space and Return press), list and tree rows, text fields, and the view-stack headers. Each shows the focus ring. Other buttons take no focus in the first slice (Q3), so no ring is expected on them.
