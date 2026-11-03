---
task_uid: "01m6enxs4003rzqvbwght17wjg"
task: "T1312"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "wait::tests::reply_comes_only_after_the_last_tab_of_the_request_closes"
    required: true
    manual: false
  - id: "C2"
    text: "wait::tests::a_file_already_open_counts_and_a_tab_moved_to_another_pane_still_holds_the_wait"
    required: true
    manual: false
  - id: "C3"
    text: "wait::tests::save_as_to_a_new_path_ends_the_wait_for_the_old_path"
    required: true
    manual: false
  - id: "C4"
    text: "wait_close::quitting_the_app_answers_every_pending_wait"
    required: true
    manual: false
  - id: "C5"
    text: "wait_close::a_dirty_tab_waits_until_its_save_or_discard_prompt_resolves"
    required: true
    manual: false
  - id: "C6"
    text: "wait_close::client_disconnect_forgets_the_wait"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Make tenmoku --wait block until every tab opened for the call is closed, then exit

## Problem
`EDITOR="tenmoku --wait"` (git commit, crontab -e, `gh pr create`) needs the command to block until the user is done with the file and then exit, 0. B2's handoff replies immediately; nothing tracks when an opened file's tab goes away. User, 2026-11-02: "Yes please" to `--wait`.

## Change
Extend the B2 request/reply: with `wait`, the server holds the connection and replies `{closed}` when every file opened for that request has no tab left in any window. Add `crates/fastedit/src/wait.rs`: a `WaitSet` of (file identity via `fe_workbench::file_identity`, window) entries per request, re-checked after each window's tab-close, window-close and tab-rename events (find the existing close hooks: `workbench_commands.rs` `close_tab`, `closed_editors.rs`). A file already open when the request arrives counts; a tab moved to another pane or window still counts as open; a Save As to another path ends the wait for the old path; quitting the app (cmd-Q, or the last window closing) replies for all pending waits. Client: exit 0 on `{closed}`, exit 1 if the connection drops before the reply with the server gone abnormally (crash), and forward SIGINT/SIGTERM as a clean disconnect so the server forgets the wait. Folders and a `--wait` with no file targets reply at once. A dirty tab's usual Save/Discard prompt on close is untouched; the wait ends only after it resolves.
