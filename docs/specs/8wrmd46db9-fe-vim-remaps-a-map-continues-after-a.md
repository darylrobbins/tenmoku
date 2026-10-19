---
task_uid: "01m59dj7jsc9n9pj8wrmd46db9"
task: "T222"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "vim::remap_tests::mapped_write_then_delete_runs_both: with nnoremap X :w\u003cCR\u003edd, X writes the file through the workbench and then deletes the line, in that order, and a failed write stops the map before the delete"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui -p fe-workbench passes with the T218 limit test replaced, not weakened; cargo clippy -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# fe-vim remaps: a map continues after a host command (:w<CR>dd) through a workbench completion protocol (T218 known limit)

## Problem

T218 (Mac T874, remaps run in the editor) ships with a known limit: a map whose right side contains a host command (`:w<CR>dd`, `do:w<CR>`) runs the keys before it, hands the host command to the workbench, and ends the map there with a status message, because the replay cannot wait for the workbench's asynchronous result. Review of T218 (four finishes) found no sound way to sequence it from the workbench side: owned diff editors, the Save As result, sync failures and interleaved edits each opened another path.

## Change

Add a completion protocol from the workbench back to the editor so the replay suspends at a host command and resumes after the workbench reports it done or failed, then lift the limit: `:w<CR>dd` writes, then deletes the line; a failed write stops the map. Paths to cover: the editor's own file, owned editors of diff views (`crates/fe-workbench/src/git_commands.rs`), Save As (`workbench.rs`), and edits made while the host command is pending. Remove the limit from the module docs of `crates/fe-gpui/src/vim.rs` and replace T218's pinning test with tests of the resumed map.
