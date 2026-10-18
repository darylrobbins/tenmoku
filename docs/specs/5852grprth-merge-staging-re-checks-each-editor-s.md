---
task_uid: "01m58jp5svabq3bc5852grprth"
task: "T206"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test git_panel::git_panel_groups_tests::merge_queue::a_retargeted_editor_aborts_the_queued_merge_staging passes: with Merge staging queued, retargeting the editor to another file before its save starts saves nothing, runs no git add, and leaves the conflicted file in Merge Changes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test git_panel::git_panel_groups_tests::merge_queue::a_utf16_file_with_restored_markers_still_asks passes: a UTF-16 file whose conflict markers are restored while staging waits (and one that has them on disk at the first check) still prompts 'Are you sure you want to stage \u003cname\u003e with merge conflicts?' instead of staging; an unreadable file aborts staging with an error naming it; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_panel_groups_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Merge staging re-checks each editor's buffer and path and decodes files by their encoding before looking for conflict markers (T177 review F1, F2)

## Problem

T177 (Merge Changes group and "stage with merge conflicts?" ask) landed on a user findings_override with two open review findings (both high, codex, round 3), both in `crates/fe-workbench/src/git_stage.rs`:

- **F1 (~line 414):** queued Merge staging keeps each editor entity without its original buffer id or path. If an editor changes files while staging waits in the queue, the closure saves the editor's new file, then stages the original path's disk contents; the post-save marker check cannot see the mismatch. `save_and_stage` already guards this transition; Merge staging does not.
- **F2 (~line 478):** the post-save check reads the saved bytes as UTF-8, but editors keep encodings such as UTF-16. For a UTF-16 buffer whose markers are restored while staging waits, the saved bytes have interleaved NULs, `parse` finds no conflict, and `git add` resolves the file without the confirmation. The initial disk-only check decodes the same way and has the same defect.

## Change

In `git_stage.rs`:
- Capture each editor's buffer id and path when Merge staging checks it. Before saving, validate both are unchanged; if either changed, abort that staging (no save, no `git add`) and say so in the status.
- Read the file through the application's encoding-aware text reader before parsing for markers, in both the initial disk-only check and the post-save check; handle a read failure explicitly (abort staging, report the file and the error) instead of treating it as "no markers".

New tests go in a new file `crates/fe-workbench/src/git_panel_merge_queue_tests.rs`, declared from `git_panel_groups_tests.rs` with one `#[path = "git_panel_merge_queue_tests.rs"] mod merge_queue;` line (a test file declares it, so the new module is seen at the base).
