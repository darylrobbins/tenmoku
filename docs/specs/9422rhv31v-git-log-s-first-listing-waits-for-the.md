---
task_uid: "01m591nyppnbh7tm9422rhv31v"
task: "T1002"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Restoring a session with Git Log visible and a delayed-loading active file in a repository different from the workspace root lists that file's repository, not the root's."
    required: true
    manual: false
  - id: "C2"
    text: "In a rootless session with a delayed active file inside a repository, the panel lists that repository and never shows 'Not in a git repository'."
    required: true
    manual: false
  - id: "C3"
    text: "A restored session whose active file never resolves (no file, rootless) still ends at 'Not in a git repository' with no toast, and lists at most once."
    required: true
    manual: false
  - id: "C4"
    text: "First show by rail click with no restore still lists once as before; T808's tests pass unchanged."
    required: true
    manual: false
---

# Git Log's first listing waits for the restored active file, so a restored session lists that file's repository, not the root's or 'Not in a git repository' (T808 review F1)

T808 review F1 (medium, round 1). git_log.rs `list_first_git_log` (~409) runs once, deferred from the panel's first draw (`ask_for_first_list`, `first_asked`, ~836-845). On session restore `Workspace::restore` shows the dock before the restored items load, so `active_editor()` is None: the log lists the root repository instead of the active file's, or in a rootless session sets `not_repository` for good. `first_asked`/read stop any later correction.

Fix: delay the first request until restoration finishes, or resolve the pending active file's path. Touch workbench.rs only if a restore-finished signal is needed. Put the regression test in the NEW file crates/fe-workbench/tests/git_log_restore_first_list.rs (don't extend existing test files); it uses a delayed file load, a repository different from the root, and a rootless session.
