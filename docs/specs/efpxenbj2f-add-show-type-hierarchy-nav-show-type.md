---
task_uid: "01m5n9hzswykshwpefpxenbj2f"
task: "T219"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "nav.show_type_hierarchy (no default key, in the Go menu and palette) shows the one Hierarchy panel in Types mode rooted at the type at the caret, supertypes first; the direction toggle shows subtypes; rows load lazily, cycles are marked, Enter opens the type"
    required: true
    manual: false
  - id: "C2"
    text: "No provider or no item toasts; calls and types modes replace each other in the one panel; the call hierarchy tests pass unchanged apart from the panel title"
    required: true
    manual: false
  - id: "C3"
    text: "New tests in type_hierarchy_tests.rs pass headless"
    required: true
    manual: false
---

# Add Show Type Hierarchy (nav.show_type_hierarchy): supertypes and subtypes in the hierarchy panel, as VS Code does (Mac T1116)

## Problem

Follows the call hierarchy panel and the hierarchy requests card. VS Code has `editor.showTypeHierarchy` ("Peek Type Hierarchy", no default key) showing a type's supertypes (and, toggled, its subtypes) as a tree. The fe-lsp side (`prepare_type_hierarchy`, `supertypes`, `subtypes`) exists once the requests card lands; the panel does not show them.

## Change

1. `crates/fe-workbench/src/hierarchy_panel.rs` gains a `Kind::{Calls, Types}` mode with the same tree, rows, lazy loading, cycle marker, navigation and toggle: for Types the toggle is Show Supertypes / Show Subtypes (`hierarchy.toggle_direction` flips whichever is active); rows have no call sites; opening a row jumps to the item's selection range; the panel title is "Type Hierarchy: <name>" (the sidebar entry stays one panel, "Hierarchy", renamed from "Call Hierarchy" by this card, its show command staying `nav.show_call_hierarchy`; the existing call tests pass unchanged apart from the title string they assert).
2. Command `nav.show_type_hierarchy` ("Show Type Hierarchy", `#[command]` in workbench_commands.rs, no default key as in VS Code; palette and Go menu entry in crates/fe-keymap/menus/fastedit.toml after "Show Call Hierarchy"): `prepare_type_hierarchy` at the caret, supertypes by default (VS Code's default); none toasts "No type hierarchy available here".
3. `crates/fe-workbench/src/command_catalog_tests.rs` lists `nav.show_type_hierarchy`.

## Tests

New `crates/fe-workbench/src/type_hierarchy_tests.rs` (#[path] mod at the end of hierarchy_panel.rs next to the call tests' mod; fake server): `show_type_hierarchy_lists_supertypes`, `toggle_shows_subtypes_lazily`, `a_cyclic_hierarchy_is_marked`, `enter_opens_the_type_at_its_selection`, `no_provider_toasts`, `calls_and_types_modes_replace_each_other_in_the_one_panel`. Linux-OK (headless). Small (s). Depends on the call hierarchy panel card (its panel and file); order: after it. Overlaps the same files as that card.
