---
task_uid: "01m4nxsn8q2q63neghs1ptqa0y"
task: "T40"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T221 (Mac T357)

Findings of the review of T221 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/vim_host.rs:255: The resolver removes `..` lexically before filesystem resolution. If root/link points to /outside/subdir, `:e link/../file.txt` opens root/file.txt instead of /outside/file.txt; a subsequent :w can overwrite the wrong file. Rebuilding the components also drops trailing slashes, so `:e file.txt/` opens a regular file instead of reporting the directory error.
  fix: Preserve filesystem path semantics, including symlink traversal and trailing-slash directory requirements, while supporting missing final files and existing-tab matching. Add regression tests for both cases.
  instances: crates/fe-workbench/src/vim_host.rs:252

- F2 medium crates/fe-workbench/src/vim_host_tests.rs:178: The tests never exercise a failed file save: :wq only succeeds, :x only takes its clean branch, and :wa only saves named writable buffers. They therefore cannot detect closing a dirty tab after a failed save, suppressing save errors, stopping :wa at its first failure, or reporting failures in the wrong pane order.
  fix: Add deterministic filesystem-save failures for :w, :wq and dirty :x, asserting the error message and retained dirty tab without a prompt. Test :wa with multiple failures, including an untitled dirty buffer, and a later successful save; assert completion of the successful save and the first failure in pane order.
  instances: crates/fe-workbench/src/vim_host_tests.rs:157, crates/fe-workbench/src/vim_host_tests.rs:178, crates/fe-workbench/src/vim_host_tests.rs:186, crates/fe-workbench/src/vim_host_tests.rs:275
