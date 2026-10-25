---
task_uid: "01m5n9hzqh4qeraq469m4wd2x8"
task: "T218"
rev: 3
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "nav.show_call_hierarchy (shift-alt-h, also in the Go menu) shows a right-dock Call Hierarchy panel rooted at the symbol at the caret; rows expand lazily asking only that item's incoming (or outgoing) calls, with loading, empty and error rows and a recursive-call marker"
    required: true
    manual: false
  - id: "C2"
    text: "Enter or click opens the item's file at its selection, and an incoming call row opens its call site; hierarchy.toggle_direction flips incoming and outgoing and reloads; no provider or no item toasts; a newer request cancels an older one. Documented limits: an asynchronous open of an unopened file can restore hierarchy focus after the user moved focus elsewhere (T240, T218 F7); toggling direction while the first prepare is pending cancels it, and an unopened target is positioned against read_to_string text, so a non-UTF-8 file does not open (T242, T218 F8, F9)"
    required: true
    manual: false
  - id: "C3"
    text: "The panel joins the rail after first use; new tests in hierarchy_panel_tests.rs and call_hierarchy_keys.rs pass headless"
    required: true
    manual: false
---

# Add a Call Hierarchy panel (nav.show_call_hierarchy, shift-alt-h): lazily expanded callers or callees of the symbol at the caret, as VS Code does (Mac T1115)

## Problem

Follows the hierarchy requests card. VS Code's Call Hierarchy (`editor.showCallHierarchy`, shift-alt-h) opens a panel showing the callers of the symbol at the caret as a lazily expanded tree, with a toggle to callees (Show Incoming/Outgoing Calls); clicking a call opens the file at the call site. fastedit's closest thing is the References panel (crates/fe-workbench/src/references_panel.rs), which is flat and fed by fe-nav, not by hierarchy requests.

## Change

1. New `crates/fe-workbench/src/hierarchy_panel.rs` (+ `HierarchyPanel` exported from lib.rs, mod in lib.rs): a sidebar panel in the right dock on the History panel's pattern (made the first time its command runs, then on the rail: `Dock::set_history` in dock.rs is the model), `SidebarPanel { name: "Call Hierarchy", icon: a call-tree icon already in fe_ui::Icon (pick the nearest; adding an icon is not in scope), show_command: "nav.show_call_hierarchy", always_on_rail: false }`.
2. The panel is a tree of rows `{ item, call sites, children: NotLoaded | Loading | Loaded | Failed }` using `fe_ui::TreeRow` and the Outline panel's row look (kind icon from `OutlineKind`, name, dimmed detail, call-site count when >1, chevron). The root is the item at the caret. Expanding a row asks its calls (incoming or outgoing per the panel's mode) off the UI thread; a loading row shows "Loading…", an empty one "No calls", an error the message. Cycles are shown (a row that equals an ancestor gets a "recursive" marker and no chevron). Click or Enter on a row opens its file at the item's selection, activates a call site row's first `from_range` when the mode is incoming and the row's caller file is shown (on a second Enter cycles through the sites). Left/Right collapse/expand, Up/Down move. A toolbar toggle (and `hierarchy.toggle_direction` "Show Outgoing Calls"/"Show Incoming Calls") flips the mode and reloads from the root; the title row shows "Call Hierarchy: <name>".
3. Command `nav.show_call_hierarchy` ("Show Call Hierarchy", `#[command]` in workbench_commands.rs, key shift-alt-h as in VS Code on every platform, context `workbench && !terminal`; also in the Go menu in crates/fe-keymap/menus/fastedit.toml after "Go to Implementations"): `prepare_call_hierarchy` at the active editor's caret; with several items the panel shows the first (VS Code picks the first); none toasts "No call hierarchy available here"; no server toasts the same as other LSP navigation does when none is running. Old requests are cancelled by a generation counter (dropping the future cancels).
4. `dock.rs`: the `DockOp`/panel registration the History panel has; `crates/fe-workbench/src/command_catalog_tests.rs` gains `nav.show_call_hierarchy` and `hierarchy.toggle_direction`.

## Tests

- New `crates/fe-workbench/src/hierarchy_panel_tests.rs` (#[path] mod at the end of hierarchy_panel.rs, fake server, `crate::tests::lsp::Lsp`): `show_call_hierarchy_lists_the_item_at_the_caret_as_root`, `expanding_loads_incoming_calls_lazily` (only that item's request is sent), `toggling_direction_asks_outgoing_calls`, `a_recursive_call_is_marked_and_not_expandable`, `enter_opens_the_callers_call_site`, `no_provider_toasts`, `empty_and_error_rows`, `a_newer_request_cancels_the_older_one`, `panel_is_on_the_rail_after_first_use`.
- New `crates/fe-keymap/tests/call_hierarchy_keys.rs`: shift-alt-h resolves to `nav.show_call_hierarchy` on macOS and Linux and the Go menu names it.
Linux-OK (headless GPUI + keymap tests). Overlaps: dock.rs, workbench_commands.rs (T937, T1063, T1079, T1080), keymaps/fastedit.toml (T937, T1077). Depends on the hierarchy requests card.

## Implementation notes

- `hierarchy.toggle_direction` is titled "Toggle Incoming and Outgoing Calls" in the palette (command titles are static); the panel's own button says "Show Outgoing Calls" / "Show Incoming Calls".
- `lsp-types` moved from dev-dependencies to dependencies of fe-workbench: call sites are `lsp_types::Range`.
- The root opens expanded (incoming calls load at once, as VS Code does); every other row loads on first expand. A click opens the row and expands it if collapsed.
- `crates/fe-keymap/tests/menu_go_nav.rs` lists the Go menu's items, so it gained the new entry.
