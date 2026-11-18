---
task_uid: "01m7reb2pckqzbejkxa7mks8dn"
task: "T1968"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "GH CI windows Test passes the 6 fe-git::worktree tests failing at 8ba6f008c: add_list_and_remove_a_detached_worktree, freeze_captures_files_outside_a_sparse_cone, freeze_takes_untracked_files_not_ignored_and_leaves_index_and_head, freezing_in_two_linked_worktrees_leaves_each_others_refs_alone, removing_a_worktree_whose_directory_is_gone_removes_only_it, removing_one_namespaces_refs_removes_no_others"
    required: true
    manual: false
  - id: "C2"
    text: "Only the test file changes; fe-git tests still pass on the gate"
    required: true
    manual: false
---

# fe-git worktree tests on Windows: init() canonicalizes the temp dir with std (verbatim \\?\), so git worktree add fails and FileStatus paths mismatch

# fe-git worktree tests: verbatim temp root on Windows

GH CI 44405849017 on 8ba6f008c, windows Test. `init()` (tests/worktree.rs:31) builds the root with `fs::canonicalize`, which gives `\\?\C:\...` on Windows:
- 4 tests panic at worktree.rs:71 (`add_lane`): `git worktree add --detach \\?\C:\...\lane-2` exits 128, "could not create leading directories".
- freeze_captures_files_outside_a_sparse_cone (:213) and freeze_takes_untracked_files_not_ignored_and_leaves_index_and_head (:344): `repo.changed_files` gives `C:\...\main\out/new.txt` while the expected `root.join(name)` is `\\?\C:\...\main\out\new.txt`.

Fix: `dunce::canonicalize` in `init()`. dunce is already an fe-git dependency. Callers in production already pass plain paths, so no production change.
