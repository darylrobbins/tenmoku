---
task_uid: "01m78dvebkv7c6e8syed5dx0az"
task: "T1753"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "tests::project_drag::drop_on_folder_moves_the_row, drop_on_file_uses_its_parent, option_drop_copies, drop_into_itself_does_nothing_and_toasts, drop_on_current_parent_is_a_no_op, selection_drags_together, collapsed_folder_opens_on_hover, editors_follow_a_dragged_rename, lsp_will_rename_runs_for_a_drop pass"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_clipboard_tests, project_panel_ops_tests, tests::lsp_file_operations, tests::project_panel and a11y_tabs_tree pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
---

# Project tree: drag rows onto a folder to move them, hold Option to copy, folders open under the pointer

# Project tree: drag rows onto a folder to move them, hold Option to copy

Today a row cannot be dragged at all (no on_drag/on_drop in project_panel.rs or kiln-ui tree_row.rs); a move is cut + paste. VS Code and IntelliJ drag one or many rows onto a folder (move; Option on macOS copies in VS Code, the copy modifier in IntelliJ), spring-load folders on hover, and highlight the target.

## Change
- kiln-ui `TreeRow`: drag source (carries the dragged paths, a small ghost: icon+name, or "3 items"), drop target on folder rows and on the root area, and a drop-hover highlight (kiln role token, no new hex). Dragging a row outside the selection drags that row alone; inside it, the whole selection (multi-select card).
- project_panel_drag.rs (new): a drop onto a folder (a file row drops onto its parent) moves through the same path as a cut-paste (`paste_cut` in project_panel_clipboard.rs: refused while busy_under, LSP willRename first, editors follow with `PanelEvent::Renamed`); with Option held it copies as paste_copy. Dropping into itself or a descendant, onto the current parent, or onto an unlistable folder does nothing and shows the existing paste-into-itself toast. A collapsed folder under the pointer for 700 ms expands. Escape cancels.
- No overwrite ever: name clashes use the existing ` copy` rule for copy, and for a move the existing name-taken toast (no replace prompt in this card).
- Strings in project.ftl.

## Pins
project_panel_clipboard_tests (cut/paste cases are the behaviour the drop reuses), project_panel_ops_tests, tests::lsp_file_operations (willRename on a move), tests::project_panel, a11y_tabs_tree.rs. No golden moves. Keyboard stays the accessible path (cut/paste); the drag is mouse-only and says so in the a11y note.
FS facts: a case-only target, a move across volumes (copy + delete, not rename) and symlinked targets go through fs_ops::move_into's existing identity checks; do not add a second move routine.
Platform: linux-ok (Option is `alt` in GPUI modifiers).

## Implementation notes

- kiln-ui `TreeRow` gained `on_drag(value, ghost, on_start)` (the ghost is the row's icon and name, or the host's text such as "3 items"), `on_drop`, `on_drag_move`, `drop_target(bool)` and `TreeRow::drop_color` (`state.drop_target` at 0.2 alpha, as a pane's drop overlay; no new hex). The ghost view is private to tree_row.rs, since kiln-ui's lib.rs is outside this card's scope.
- project_panel_drag.rs works out which row is under the pointer from the list's last layout (bounds, measured row height) and its scroll offset now. It does this when the drag starts (TreeRow's `on_start`, called from GPUI's ghost constructor, because the panel's own `on_drag_move` runs in capture, before the bubble-phase move that creates the drag), on every move (the panel div's `on_drag_move`, which also records the panel's bounds for "outside"), and as the panel draws. That last case catches rows that scroll or move under a pointer that stays put, and it resets the spring timer when the row changes. The spring timer checks the pointer again before it expands a folder. A drop that no row takes lands on the panel's element, which means the root, so the root's row, the space below the rows and the filter line all drop in the root. The folder a drop would go in, and the rows under it, are tinted only if the drop would do something.
- A drop goes through the clipboard's paste path (`paste_into`, split out of `paste`): one entry → `paste_cut`/`paste_copy`, several → `paste_cuts`/`paste_copies`, with the same busy checks, willRename request(s), `PanelEvent::Renamed` and edits. `MoveOf::Drop` replaces the cut's serial, so a drop never edits the clipboard, and a drop's move keeps its name: a taken name (case-insensitively, through `paste_name_besides`) refuses that move with a new toast string, `workbench-project-drop-name-taken`. The existing `workbench-project-name-taken` belongs under the name field ("Choose another name") and doesn't read as a toast.
- Review r1 (F5): a drop into itself or a folder inside it, into the folder an entry is in already (copies too), or into a folder that couldn't be read (`row.error`) shows the existing paste-into-itself toast for each entry, as the spec says. None of them shows a target while hovering. With several selected (F1), dragging any of them carries the selection's topmost paths, even when that leaves one path (a folder and a row inside it).
- Escape: while a drag is under way the panel renders with the `project_panel_drag` key context, which no binding names (the same approach as the right-click menu), so escape reaches the panel's `on_key_down`, which calls `stop_active_drag`. Escape counts with any modifier held, Option for a copy included (F3).
- Known gap, inherited from cut/paste: an expanded folder moved into a folder that has never been listed arrives collapsed, because `move_one` can't find its new node until that listing lands. The test moves it into an expanded folder.
- The drag is mouse-only; the TreeRow docs and the drag module docs say cut and paste is the keyboard path.
- Carried T1752 F6: `run_op` notifies when a navigation op clears a multi-selection. F8: the shift-click, cmd-click and shift-arrow fallbacks announce the count after selecting from empty. Their tests are in tests/project_drag.rs (`navigation_redraws_a_cleared_selection`, `selecting_from_empty_announces_the_count`) because tests/project_multi_select.rs is outside this card's scope; each fails without its fix.
