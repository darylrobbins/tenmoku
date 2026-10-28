---
task_uid: "01m5wmbsh27y28cx9mvbexq8kd"
task: "T1193"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "close_many_tests::close_saved: workbench.close_saved closes every tab of the active pane that has no unsaved changes, keeps dirty tabs open without a prompt, activates a surviving tab (the one that was active if it survives, else its nearest surviving neighbour), and leaves other panes alone; pinned tabs are closed like any other saved tab"
    required: true
    manual: false
  - id: "C2"
    text: "close_many_tests::close_in_group: workbench.close_group_editors closes all tabs of the active pane; a dirty tab asks to discard exactly as pane.close does (Cancel keeps it and stops closing the remaining ones; Discard closes it); a tab whose buffer another pane shows closes without asking; the pane itself stays open and empty"
    required: true
    manual: false
  - id: "C3"
    text: "close_many_tests::close_all: workbench.close_all_editors does the same for every pane, in pane order, asking per dirty buffer once even when two panes show it; the layout of panes is kept; each closed file lands on the closed-editors stack so reopen_closed_editor restores the last one"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap close_many_keys: on macOS cmd-k u resolves to workbench.close_saved and cmd-k w to workbench.close_group_editors; on Linux ctrl-k u and ctrl-k w; cmd-k cmd-w still resolves to pane.close (unchanged; VS Code binds closeAllEditors there, left for the lead to decide) and workbench.close_all_editors has no default key; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-workbench -p fe-keymap -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Close Saved Editors (cmd-k u), Close Editors in Group (cmd-k w) and Close All Editors, as VS Code's workbench.action.closeUnmodifiedEditors, closeEditorsInGroup and closeAllEditors

Closing tabs is one at a time (workbench.close_tab) or a whole pane (pane.close, which refuses on the last pane). VS Code's tab-group commands: Close Saved Editors (cmd-k u; ctrl-k u elsewhere) closes every saved tab; Close Editors in Group (cmd-k w; ctrl-k w) closes the group's tabs but keeps the group; Close All Editors closes every group's tabs. Verified absent: no close_saved / close_all / close_group id anywhere in crates/, and the open Close Others card (blocked) covers only "other tabs".

Reuse, don't duplicate: Close Others has landed (Pane::close_others in pane.rs ~1426, Workbench::close_other_tabs in workbench_commands.rs ~46, tests close_others_tests.rs). Factor its "close these tabs, asking for each dirty one" loop into one Pane helper taking a predicate, make close_others call it, and build the three new commands on it; close_others_tests must pass unchanged.

Add three typed commands to the workbench_commands.rs #[command] block, titled "Close Saved Editors", "Close Editors in Group", "Close All Editors", ids workbench.close_saved, workbench.close_group_editors, workbench.close_all_editors. Put the logic in the new module close_many.rs (declared from workbench_commands.rs with #[path], tests likewise; no lib.rs edit). Reuse the existing dirty-close path: Pane::close_active / take_item and the discard prompt of workbench_chrome.rs close_active_pane (kiln_shell::close "tabs agree"); close_tests.rs documents the rules (an editor whose buffer another pane shows closes without asking). Do not reimplement the prompt: run the same agree-then-close flow over a list of tabs and stop at the first Cancel.

Keys: "secondary-k u" and "secondary-k w", context "workbench", next to "secondary-k secondary-w" (pane.close, ~line 883). Do not rebind cmd-k cmd-w: pane.close lives there today although VS Code's cmd-k cmd-w is Close All Editors; mention it in the PR so the lead can decide.

Tests: close_tests.rs shape (opened(), run(id), tabs()); open 3 files across two panes, dirty one with a typed edit, run each command, assert the tab counts and the prompt behaviour; the test platform answers the discard prompt, drive it as close_tests.rs does.
