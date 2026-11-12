---
task_uid: "01m78dvde3m2jpww4mgdp91bhb"
task: "T1752"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "tests::project_multi_select::shift_click_selects_a_range, cmd_click_toggles, shift_arrows_extend, plain_click_resets, delete_asks_once_and_trashes_topmost_only, cut_and_paste_moves_all, copy_path_joins_lines, partial_failure_names_what_failed, filter_clears_extra_selection pass"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_ops_tests, project_panel_clipboard_tests, project_panel_nav_tests, project_panel_filter_tests, project_panel_git_status_tests, tests::sidebar_b3, tests::project_panel and a11y_tabs_tree pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
---

# Project tree: select several rows with shift-click, cmd-click and shift-arrows, and delete, cut, copy, duplicate and copy path act on all of them

# Project tree: select several rows (shift/cmd-click, shift-arrows) and act on all of them

The panel's selection is one `Option<NodeId>` (project_panel.rs `selected`); delete, cut, copy, duplicate and copy path act on `selected_entry()` (one path). VS Code and IntelliJ select ranges (shift) and toggles (cmd on macOS) and apply delete, cut, copy, paste and copy path to all.

## Change
- Selection becomes an ordered set with an anchor and a primary row (the primary keeps today's meaning for keyboard navigation, reveal, rename and new file/folder, which stay single-target and act on the primary alone).
- shift-click extends from the anchor over visible rows; cmd-click toggles; shift-up/down extend; cmd-a is NOT bound (editor owns it). A plain click or any move resets to one.
- Delete: one prompt naming the count ("Move 3 items to the Trash?"), a nested selection collapsed to its topmost rows, one background task, partial failure toasts naming what failed. Cut/Copy/Paste: the clipboard holds a list of paths (project_panel_clipboard.rs); a paste keeps each name or the ` copy` rule, refuses a folder into itself per entry. Copy Path joins paths by newline. Duplicate duplicates each.
- Selected rows draw as today's focused/unfocused selection row; a11y: the row's selected state, and a count announced on extend.
- Strings in project.ftl with plural selectors.

## Pins (must pass unchanged)
project_panel_ops_tests, project_panel_clipboard_tests (single-path clipboard cases keep their meaning), project_panel_nav_tests, project_panel_filter_tests (a filter clears the extra selection), tests::sidebar_b3, tests::project_panel, a11y_tabs_tree.rs, project_panel_git_status_tests.
Goldens: `workbench.png` shows one selected row and must not move (T1600 also regenerates it: rebase on it, never both in one run).
FS facts that apply: moves go through fs_ops::move_into (dirfd-relative, refuses identity-changed paths); FSEvents never reports creation/removal inside a directory under its own path, and a case-only rename silences a root stream, so the per-entry results come from the operations, not from waiting for events.
Platform: linux-ok.

## Implementation notes

- **Selection model.** `selected` stays the primary (`Option<NodeId>`, so code and pinned tests that set it keep their meaning); `multi` holds the rows selected with it and counts only while it holds the primary and another. Anything that sets `selected` on its own (the right-click menu, `project_panel_menu.rs`, out of scope) therefore selects that row alone, unless it is one of the selected rows: a right click on a selected row keeps the selection, as VS Code's explorer does, so the menu's Delete, Cut, Copy, Duplicate and Copy Path act on all of them. Rename and New File/Folder act on the primary.
- **Shift-up/down** are handled in the panel's `on_key_down`, not bound in `fe-keymap` (out of scope) and not registered as commands (their titles would need `commands.ftl`, out of scope). No binding names them in the panel's context, so kiln's matcher lets them through; the name field, the filter's box and the open menu keep them.
- **a11y.** Every selected row is a selected `TreeItem`; only the primary is the tree's active descendant (`TreeRow::active`, kiln-ui). The count is announced politely on each extend or toggle.
- **Several entries, one job.** Delete trashes each topmost entry in turn in one background job, checking each one's identity as the single delete does; duplicate and a copy's paste do the same. A cut's paste asks the language servers about every move, then moves each on the main thread. A failure of one entry in such an operation is a toast, not a dialog: one failure names its entry in the toast's message, and several are counted, with the detail naming each and why, a line apiece. One entry alone keeps today's paths exactly (its dialog for a failed trash).
- **A cut's paste of several (review round 1).** Each move's name is looked up passing over the names earlier moves of the batch planned (`fs_ops::paste_name_besides`, compared ignoring case), so two `x.txt` from different folders become `x.txt` and `x copy.txt`. The servers are asked once for every move (`Workbench::will_rename_many`, one `workspace/willRenameFiles` with all the files), so each server's edits to a shared file come from one answer written against one text; servers' answers are joined as for a rename. Those edits are made, at the moved files' new paths, only if every move asked about was made: they were written for all of them together.
- **Keys that move select one (review round 1).** Up/down, first/last, Left, Right, Enter and vim's `o` select the primary alone before acting; cmd-click on the last selected row leaves none selected.
- **Carried T1750 F1.** The empty-space right click now sits on a container around the rows or the no-matches line (test `no_match_empty_space_opens_the_menu`).
- **Carried T1541 F7, in part.** A refused name (`NameError`, also when an operation's `io::Error` wraps one) is kept typed and worded as it's drawn. Messages fe-project words itself inside an `io::Error` (a file used as a folder, say) are still text by the time the panel sees them: keeping them as keys needs typed errors from `fe_project::fs_ops`, out of this card's scope, and so does the locale-switch test, which lives in `tests/project_text_fake_locale.rs`.
