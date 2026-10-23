---
task_uid: "01m5ge1f8c3wf5gmgm4ftrcf74"
task: "T1074"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A new SearchPanel shows no include or exclude field; a ... ToggleButton (debug selector search-details-toggle) sits right of the option toggles, and clicking it shows both fields (test details_hidden_until_toggled in search_panel_details_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Command search.toggle_details (title Toggle Search Details, SearchOp::ToggleDetails) flips the fields; bound to cmd-shift-j (macos) and ctrl-shift-j (default) in context search_panel (test cmd_shift_j_toggles_details in search_toggle_details_binding.rs, and in search_panel_details_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "The toggle button is highlighted (.on) while details are open; hiding them with non-empty include or exclude still applies those globs to the search (test hidden_filters_still_apply)"
    required: true
    manual: false
  - id: "C4"
    text: "Closing details while an include/exclude field has focus moves focus to the query (test hiding_details_refocuses_query)"
    required: true
    manual: false
  - id: "C5"
    text: "Existing search_panel_filter_tests.rs tests pass after opening details first; search_replace_tests.rs stays green"
    required: true
    manual: false
---

# Hide the search panel's files to include and exclude fields behind a ... Toggle Search Details button, opened with cmd-shift-j, as VS Code does

User request 2026-10-21: "Let's add some polish to the search in files pane. Follow after VS Code. Hide advanced options and expand".

## Problem
VS Code's Search view keeps "files to include" and "files to exclude" hidden by default behind a "..." Toggle Search Details button under the search box (command search.action.toggleQueryDetails, cmd-shift-J). fastedit always renders both fields under the query: crates/fe-workbench/src/search_panel.rs:1411-1418 (inputs created at :178-179), so the panel is taller and busier than VS Code's. The replace chevron already behaves like VS Code (hidden by default, search_panel.rs:1325-1337, `replace_shown`), so it is unchanged. There is no details command (SearchOp, :1436; command table, :1470) and no binding in crates/fe-keymap/keymaps/fastedit.toml.

## Change (copy VS Code)
- Add `details_shown: bool` (default false) to SearchPanel. Render the include and exclude inputs only while it is true. The entities and their Changed -> run subscriptions stay, so hidden non-empty globs still filter every search (VS Code applies them while collapsed).
- Add a "..." ToggleButton at the end of the query row's option toggles (debug selector `search-details-toggle`), `.on(details_shown)` so it is highlighted while open. Clicking it toggles.
- Add SearchOp::ToggleDetails and command `search.toggle_details` "Toggle Search Details", in the same table as search.toggle_replace. Bind cmd-shift-j (macos) and ctrl-shift-j (default) with context `search_panel`. Check for an existing ctrl-shift-j binding off macOS first and pick the context so vim's <C-J> is not shadowed.
- Closing details while include or exclude has focus focuses the query, as toggle_replace does (search_panel.rs:544).
- Do not persist across workspaces in this card. Replace stays as is. Results toolbar (Collapse All, Refresh, Clear, tree/list) is out of scope.

## Tests (GPUI view tests: Mac-only. The binding test is Linux-OK. No screenshot goldens change.)
- New crates/fe-workbench/src/search_panel_details_tests.rs, wired in with a #[path] mod next to the one at search_panel.rs:1658: details_hidden_until_toggled, hidden_filters_still_apply (type include, hide, run, files() still narrowed), hiding_details_refocuses_query, cmd_shift_j_toggles_details.
- New crates/fe-keymap/tests/search_toggle_details_binding.rs: search.toggle_details resolves for the platform key in context search_panel and not outside it.
- Edit crates/fe-workbench/src/search_panel_filter_tests.rs: its helpers open details before interacting with the fields (a_filter_with_focus_leaves_with_its_dock needs them rendered).
