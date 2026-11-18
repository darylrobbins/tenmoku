---
task_uid: "01m7qg8h8tx7hcnftrqf7jk11v"
task: "T693"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Each test replaces the entry so the new one cannot reuse the old inode (old file kept hard-linked or open while the new one is written, or the new file written beside it and renamed over); no production code changes"
    required: true
    manual: false
  - id: "C2"
    text: "On Linux, fe-project fs_ops_undo::another_entry_in_its_place_in_the_trash_is_not_taken and fe-workbench tests::project_multi_select::partial_failure_names_what_failed pass 20 of 20 repeated runs, and the decision file records the inodes seen before the fix (showing reuse)"
    required: true
    manual: false
  - id: "C3"
    text: "Both tests still pass on macOS"
    required: true
    manual: false
---

# Linux CI Test: fe-project another_entry_in_its_place_in_the_trash_is_not_taken and fe-workbench partial_failure_names_what_failed get the freed inode back on Linux, so the swapped entry looks unchanged (Mac T1952)

GitHub CI's linux "Test" job fails on every main run (44252290199, 44280161517) on the same two tests, both passing on macOS:

- fe-project `fs_ops_undo::another_entry_in_its_place_in_the_trash_is_not_taken` (crates/fe-project/tests/fs_ops_undo.rs:119): `restore` returns Ok where the test expects NotFound.
- fe-workbench `tests::project_multi_select::partial_failure_names_what_failed` (crates/fe-workbench/src/tests/project_multi_select.rs:~470-477): after `fs::remove_file(c)` then `fs::write(c, "again")`, the expected second toast never comes, so the earlier "2 items couldn't be moved" toast is still the last one.

Likely cause (read from code, not yet confirmed on Linux): entries are identified by (device, inode). Each test removes a file and writes a new one at the same path, and ext4/tmpfs hand the new file the freed inode, so the swap looks like the same entry. macOS doesn't reuse an inode at once.

Fix the tests so the replacement cannot get the old inode: keep the old file hard-linked or open while writing the new one, or write the new file beside it and rename it over. First confirm on Linux that inode reuse is the cause (print both inodes). If it is not the cause, or the production identity check is wrong for a case a user can hit, stop and report to the lead instead of changing production code.

## Helium filing note

- Lent from the Mac as y147 (test-only CI fix, chore). No dependency.
- **Confirm first** on helium (Linux, ext4 /home; /tmp is tmpfs): print the old and new inode in each test
  before changing anything, and record them in the decision file. If reuse is not the cause, or the
  production (device, inode) identity check is wrong for a case a user can hit, **stop and block** with the
  evidence; don't change production code.
- C3 (macOS) can't run here: map it to a manual entry citing that the fix only keeps the old inode alive or
  renames over it, which is platform-neutral; the Mac's batch gate runs both tests on macOS.
- Run the 20x loops with a CARGO_TARGET_DIR inside your worktree, never /tmp.
- Ask the lead before rgt finish if anything in the criteria can't be met as written.
