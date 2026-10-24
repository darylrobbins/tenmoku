---
task_uid: "01m5n96xfapwzcjf3b17yytsj1"
task: "T295"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "New command workbench.open_settings_ui ('Open Settings (UI)', context workbench) opens one Settings tab in the active pane, or focuses the one already open in any pane; the tab is titled Settings, is not a file (no path, not saved in layouts, not restored), and closes like any tab. workbench.open_settings (the file) is retitled 'Open Settings (JSON)' and keeps its id; secondary-, now runs open_settings_ui and the File/app menu 'Settings…' items run it; no other binding changes. Tests settings_command_opens_one_tab, second_open_focuses_it, tab_is_not_persisted, comma_binding_runs_the_ui (keymap)"
    required: true
    manual: false
  - id: "C2"
    text: "The tab shows a search field (focused on open, debug selector settings-search), then a scope bar with 'User' and 'Workspace' (Workspace hidden with no folder open; selector settings-scope-user/-workspace), then the grouped list: group heading rows ('Editor', 'Editor \u003e Minimap') and one row per setting from SettingsModel showing the key path as VS Code does (title-cased last segment, key path dim above or beside it), the description, and the current value as text for now (controls come in card 4). Typing in the search filters live via SettingsModel::search, group headings with no match disappear, and an empty result shows 'No settings found'. Tests lists_every_group, search_filters_and_hides_empty_groups, empty_result_message, scope_bar_hidden_without_folder"
    required: true
    manual: false
  - id: "C3"
    text: "A row whose value is set in the selected scope shows the modified marker (a 2px accent bar at the row's left edge, selector settings-modified-\u003ckey\u003e), and a row at default shows none; switching scope re-reads the value for that scope's file; a row whose key the workspace layer ignores is dimmed with 'This setting cannot be changed in a workspace' under the Workspace tab. The tab re-reads the files whenever the workbench's existing settings-reload notification reaches it (an external edit that changes an effective value), when the tab is activated or focused, when the scope is switched, and after the tab's own writes; it adds no second watcher and no change to the config service or the root watch. Limit (Known limits, review round 3): an external edit that leaves every effective value unchanged, or a workspace file under a root the existing watcher does not cover (a second window on another root), is shown only after the next of those re-reads; the Known limits section says so, and the follow-up card removes the limit. Tests modified_marker_follows_scope, switching_scope_rereads, workspace_restricted_rows_are_dimmed, external_edit_changing_an_effective_value_refreshes_the_tab, activating_the_tab_rereads"
    required: true
    manual: false
  - id: "C4"
    text: "Up/Down moves the selected row and scrolls it into view, / or cmd-f focuses the search, Escape clears it; the list is virtualised (only visible rows are rendered; a test with all schema rows asserts the rendered row count is bounded)"
    required: true
    manual: false
  - id: "C5"
    text: "GPUI view tests use a headless window and run on Linux; no screenshot golden changes (a golden is a separate Mac-only card)"
    required: true
    manual: false
---

# Add Open Settings (UI): a Settings tab, as VS Code's Settings editor, with a search box, a grouped list of every setting with its description, default and modified marker, and User / Workspace scope tabs (Mac T1096)

## Problem
Changing a setting means opening `settings.toml` and knowing the key names. VS Code's Settings editor (workbench.action.openSettings, cmd-,) lists every setting with a description and default, a search box, a modified marker and User / Workspace tabs. Card 1 provides the model; this card is the read-only view, tab and command. Controls and write-back are card 4.

## Change (copy VS Code)
- New `crates/fe-workbench/src/settings_editor.rs`: `SettingsEditor` entity, a pane item like `DiffView`/`FileViewer` (see how `pane.rs` `Tab::of` and `Pane::open` accept any `ItemHandle`; `persist.rs` must skip it like the in-place Markdown preview: check that a saved layout with the tab open round-trips without it). It holds a `SettingsModel`, the search field (a single-line input as the Find bar builds one in `find_bar.rs`), the selected scope, and a scroll handle; rows are laid out with kiln-ui list virtualisation as the project panel or the Git Log do.
- `workbench_commands.rs`: new `#[kiln::command] workbench.open_settings_ui` beside `open_settings_command` (line ~778); `workbench_files.rs`: `open_settings_ui(window, cx)` finds an existing tab (scan `self.panes(cx)`) before opening.
- Keymap: `secondary-,` (fastedit.toml:468) now `workbench.open_settings_ui`; menus/fastedit.toml lines 32 and 112 point to it. `typed_command_ids_tests.rs` MOVED row for open_settings becomes ('Open Settings (JSON)') and gets a row for the new command; `command_catalog_tests.rs` list gets the id; `tests/open_settings.rs` keeps testing the file command; `fe-keymap/tests/menu.rs:484` only uses the id as a sample, check it still passes.
- Reads settings text off the main thread (`cx.background_spawn`, as `open_settings` does for the template) and builds the model on completion.
- Out of scope: editing, reset, the gear menu, JSON-link rows for `Other` kinds (they show 'Edit in settings.json' text only; the link is card 4).

## Tests
New `crates/fe-workbench/src/settings_editor_tests.rs` (declared like the other `*_tests` mods in lib.rs) and `crates/fe-keymap/tests/open_settings_ui_binding.rs` (modelled on `search_toggle_details_binding.rs`). Linux-OK. Conflicts to expect: other cards editing `fastedit.toml`, `workbench_commands.rs` and the two command tables append rows; rebase, do not rewrite.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.

## Implementation notes

- `Workbench::open_settings_ui` and `reload_settings_editors` live in `settings_editor.rs` (an `impl Workbench` block), beside the view, rather than in `workbench_files.rs`. The tab is inserted with `PaneView::insert_item`, which focuses it; its focus handle is the search field's, so showing it again focuses the search.
- The files are read and the `SettingsModel` built on the background executor; a newer read drops the older one's task.
- Refresh on external edits: `Workbench::reload_settings` (in `workbench_root.rs`, added to the scope by respec) calls `reload_settings_editors`, so the tab follows the existing ConfigWatcher → `reload_settings` path with no watcher of its own. That path is deduplicated upstream (kiln's config service publishes only a changed `Settings`, and fastedit `main.rs` `give` returns early on equal settings), and the config service watches only the launch root's project file (a window on another root reads its own once, `main.rs` `settle`). Neither is in this card's scope, and the criterion rules out a watcher of the tab's own. So the tab also re-reads both files whenever it's activated or focus comes back into it from another tab, pane or app (`on_focus_in` on the tab's root focus handle, which fires on window activation too), on a scope switch and on a root change. See Known limits.
- The search field handles the text-input editing commands (`text_input.backspace`, `delete_word_left`, `clear`, `paste`) as the find bar does (review round 1, F1).
- Rows are 96px with explicit line heights (18px title and value lines, two 16px description lines, 10px padding, 4px gaps), so a two-line description never squeezes the value (F4). Moving up to a group's first setting shows its heading too only if two rows fit, placing the heading at the top (strictly) when either row is out of view, as after the wheel scrolled the group away; otherwise the selected row alone is scrolled into view (F5, rounds 1 and 2).
- Values under Workspace: the project file's value if set, else the user's, else the default; the marker shows only for the selected scope's file. `Other` rows say "Edit in settings.toml" (the file fastedit has), not VS Code's settings.json.
- The list is a `uniform_list` with one row height (96px) for headings and settings; descriptions clamp to two lines.
- Test-only debug selectors besides the criteria's: `settings-row-<key>`, `settings-group-<title>`, `settings-restricted-<key>`, `settings-empty`.
- Review rounds 1 and 2 left F2 and F3 (refresh on every disk change, and a non-launch root's project file) open; the lead narrowed C3 by respec to the re-reads above and moved the watcher work to a follow-up card.
- C3's re-read after the tab's own writes has nothing to follow yet: this card's tab is read-only, and card 4's write-back calls `SettingsEditor::reload` once a write lands.

## Known limits

- An external edit to either settings file that leaves every effective value unchanged (a key set explicitly to its default, a user value the workspace overrides, a key the workspace layer ignores) doesn't reach `Workbench::reload_settings`: kiln's config service publishes only a changed `Settings`, and fastedit `main.rs` `give` skips equal settings. The tab shows such an edit at its next re-read: when it's activated or focused, when the scope switches, or after its own writes.
- The existing watcher covers only the launch root's `.fastedit/settings.toml`. A workspace file under another root (a second window on another folder, or a folder opened later) is read when the tab opens, on a root change and at those same re-reads, not when it changes on disk.
- Both are removed by the follow-up card that forwards the config service's file-change notification ahead of its dedup and has the watched paths follow each window's root; this card changes neither the config service nor the root watch.
