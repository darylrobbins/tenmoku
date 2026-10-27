---
task_uid: "01m5wbfgtxpjffysadj4pgsrr3"
task: "T363"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "compare_saved_tests::opens_saved_against_buffer: with a.txt open and an unsaved edit, diff.compare_saved opens a diff tab titled \"a.txt (Saved ↔ Working Tree)\" whose left side is the file as on disk (read-only) and whose right side is the open buffer (editable, the same buffer as the tab); the diff shows the edited row as a change"
    required: true
    manual: false
  - id: "C2"
    text: "compare_saved_tests::no_changes_and_no_file: with no unsaved changes the tab opens and reports the NO_CHANGES state of git_diff; with an untitled buffer or a file deleted on disk the command answers a status message (NO_FILE / file missing) and opens no tab; asking again for the same file shows the existing diff tab instead of a second one"
    required: true
    manual: false
  - id: "C3"
    text: "compare_saved_tests::follows_edits_and_save: typing in the right side updates the diff; the left side is the saved text at the time of asking and is not re-read from disk on a later edit, a save or a change on disk (as VS Code's snapshot), so after saving the diff still shows the changes against that snapshot"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap compare_saved_key: on macOS cmd-k d and on Linux ctrl-k d resolve to diff.compare_saved in the editor context; cmd-k cmd-d is not affected; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-workbench -p fe-keymap -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Compare Active File with Saved (cmd-k d), as VS Code's workbench.files.action.compareWithSaved (Mac T1192)

The diff view (git_diff.rs) compares a file with the index, HEAD or another file (diff.compare_file: read-only left, the active file's buffer on the right, titled with the sides). It cannot compare a file with its own saved text, which needs no repository: VS Code's Compare Active File with Saved (cmd-k d; ctrl-k d on Linux). Verified absent: rg finds no compare_saved / compareWithSaved in crates/.

Add the typed command diff.compare_saved ("Compare Active File with Saved", context "editor", next to diff.compare_file's registration in git_commands/workbench_commands) and implement it in the new module compare_saved.rs as a sibling of the diff.compare_file path: read the active file's text from disk off the UI thread (as git_diff.rs reads its git sides), then open the same DiffView with a read-only left side named "Saved" and the active buffer as the right side "Working Tree", reusing git_diff.rs's existing view builder (make the one helper you need pub(super); no other change there; keep the file's module doc in step if the change alters what it says). The left side is a snapshot: it is not refreshed after later disk changes or by refresh_git_diffs. A second ask for the same file shows the open tab. No repository is needed, so none of git_diff.rs's NO_REPO paths apply.

Modules: declare compare_saved.rs and its tests from workbench_commands.rs with #[path] attributes (no lib.rs edit). Key: "secondary-k d", context "editor", beside the other secondary-k chords (~line 372-390).

Tests: workbench with a temp folder and a file, edit through the editor model, run through RunCommand as close_tests.rs does; read the left side's text and the tab title from the diff view; no GUI, no snapshots.

Lent from Mac T1192 (y75). Linux-OK, headless tests. Serialized behind T360 on shared keymap/catalog lists.

## Implementation notes

- git_diff.rs is a private module of workbench, so compare_saved.rs (under workbench_commands) cannot reach its helpers; instead of widening several, git_diff.rs gains one `pub(crate) begin_saved_diff`/`finish_saved_diff` pair (path via op_path, existing-tab lookup before and after the read, the landing guards, new_diff, place, show). compare_saved.rs holds the command, the off-thread read and the answers.
- Scope widened (respec) to crates/fe-keymap/tests/git_keys.rs (its `diff.` binding count) and crates/fe-gpui/src/tests.rs (DEDICATED list of tested editor bindings).
- Review F1-F3: the landing/may_show guards, a tab lookup before disk I/O, and op_path. C3 was respecced: the left is a pure snapshot, untouched by edits, saves or disk changes (a save-following left failed review F4 and isn't in the spec). Controlled async regressions (review F5, low) are a follow-up.
