---
task_uid: "01m6sf8j6tthb2pfqvc3vk0kgx"
task: "T1425"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "down_scrolls_the_selected_row_into_view passes: after Down past the fold the selected row is inside the page's bounds"
    required: true
    manual: false
  - id: "C2"
    text: "up_scrolls_back_to_the_first_row passes"
    required: true
    manual: false
  - id: "C3"
    text: "clearing_recents_refreshes_a_shown_page passes: after workbench.clear_recent no old entry row remains on a visible page"
    required: true
    manual: false
  - id: "C4"
    text: "the existing start_page::start_page_tests tests still pass"
    required: true
    manual: false
---

# Start page: scroll the keyboard selection into view and refresh recents while it shows

Start page: keep the keyboard selection in view, and refresh its recents while it shows (two findings carried from the review of 47a7425a, Mac T1398 / T475). Both confirmed on main 4b1254eb.

1. Selection never scrolls into view (crates/fe-workbench/src/start_page.rs). `select` (~line 175) only moves `self.selected` and notifies; `render` (~line 321) makes the page an `overflow_y_scroll` div with no `ScrollHandle`, so with a populated recent list or a short window Down/j moves the highlight below the fold and Enter (`run(self.selected)`) activates a row the user cannot see. Fix: track the page's scroll with a `ScrollHandle` (`track_scroll`), and after any selection change (Up/Down/j/k, and the reset to row 0 in `reload`) scroll the selected row into view. Rows live inside per-section divs, so use row bounds rather than `scroll_to_item` on direct children: either track each row's bounds in render, or give the rows a stable id and scroll the handle's offset so the selected row's top/bottom sits inside the viewport. Mouse clicks need no scrolling.

2. Recents go stale while the page shows (crates/fe-workbench/src/workbench.rs). `sync_start_page` (~line 775) calls `StartPageView::reload` only on the hidden-to-shown transition (`show && !self.start_page_shown`). Clear Recently Opened (workbench.clear_recent, via `recent_picker::clear`) and opening a folder or file (`recent_picker::record`) leave a visible page listing the old entries until it is hidden and shown again. Fix: reload a shown page after the recent list changes. Suggested: make `recent_picker::record`/`clear` (crates/fe-workbench/src/recent_picker.rs ~118, ~138) bump a global change counter or emit a signal after their queued job finishes, and have the workbench (or the page) observe it and call `reload` when `start_page_shown`. `load` already queues behind earlier jobs, so a reload issued after the change sees it. Keep the selected row stable if cheap; resetting to row 0 as `reload` does today is acceptable.

Tests: a NEW integration test file crates/fe-workbench/tests/start_page_selection_and_recents.rs. Style of tests/hot_exit_rootless.rs: a serial lock, clear `TENMOKU_CONFIG_DIR`/`XDG_CONFIG_HOME`/`HOME`, and set `TENMOKU_STATE_DIR` to a temp dir so the recent list is a temp `recent.toml` (integration tests get no cfg(test) isolation; `recent_picker::set_recent_file` is not visible). Seed it with `fe_config::recent` (public) or by writing the TOML; launch a rootless workbench (Launch { root: None, file: None }) so the Start page shows; find rows by the debug selectors `start-page-row:<label>` and `start-page` (cx.debug_bounds, as src/start_page_tests.rs does). StartPageView is not public, so assert on drawn bounds only.
- `down_scrolls_the_selected_row_into_view`: seed 30 recent files, shrink the window so the list overflows, press Down 25 times, draw; the selected row's bounds lie inside the `start-page` bounds. Fails at base (the row is below the fold).
- `up_scrolls_back_to_the_first_row`: after the above, press Up 25 times; the first row is inside the bounds again.
- `clearing_recents_refreshes_a_shown_page`: seed 3 recents, show the page, confirm their rows exist, run workbench.clear_recent (dispatch RunCommand as hot_exit_rootless `run` does), run until parked, draw; no `start-page-row:` for the old entries remain. Fails at base.
- `opening_a_folder_refreshes_a_shown_page` is optional: only if opening one in the same window keeps the page visible; otherwise cover recording through a second file open in a split-free flow, or skip it.

Scope: the two source files named above and recent_picker.rs, plus the new test file. Do not edit crates/fe-workbench/src/lib.rs or workbench_commands.rs unless an existing line must change.
