---
task_uid: "01m47k8rz7e68zdkvwm67kv6nb"
task: "T91"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project test tests::finder_walk_safety::walk_ignores_a_symlinked_gitignore: a .gitignore that is a symlink to a file of rules is not applied by walk (the paths it would hide are indexed), matching FileTree"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test tests::finder_walk_safety::a_fifo_ignore_file_does_not_block_a_cancelled_walk: with a FIFO named .ignore in a subdirectory, walk completes (or stops when cancelled) within 5 s, run on a thread and judged by a bounded wait so the test fails rather than hangs at base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project test tests::finder_walk_safety::walk_does_not_follow_a_directory_symlink: a symlink to a directory outside the root is never descended, for the initial walk, a new-directory walk from apply, and Rescan"
    required: true
    manual: false
  - id: "C4"
    text: "The finder walk no longer uses ignore::WalkBuilder; every existing tests::finder test still passes unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-project/benches/finder.rs builds in the gate; PERF.md's finder rows are updated with one run (recorded, not asserted)"
    required: true
    manual: false
---

# Walk the finder index with FileTree's filesystem protections (T62 review F1)

# Walk the finder index with FileTree's filesystem protections

## Problem

T62's finder walks the project with `ignore::WalkBuilder::build_parallel`, as its spec chose. Review F1 on
T62 found that this walk does not keep the protections `listing.rs` gives FileTree (T61):

- `ignore` 0.4 loads `.gitignore` / `.ignore` with a plain `File::open`, so a symlinked ignore file changes
  finder results even though FileTree skips it, and a FIFO named `.ignore` blocks the walk before its
  cancellation callback runs. A synchronous `apply` walk then hangs its caller.
- Traversal lists directories by path, so a directory swapped for a symlink mid-walk is followed outside
  the project.

T62 documents these as known limits and lands with WalkBuilder; this card removes them.

## Change

Replace the WalkBuilder walk in `crates/fe-project/src/finder.rs` with a parallel walker built on the
handle-based listing `listing.rs` already uses for `Listing::run`: open directories relative to their parent
handle without following symlinks, and read ignore files only when they are regular files, opened
non-blocking and no-follow. Cache parsed ignore rules per directory so deep trees don't re-read ancestor
ignore files. Keep the finder's public API and the cancellation flag unchanged.

Re-run the finder bench (`crates/fe-project/benches/finder.rs`) and update the finder rows in PERF.md; a
slower walk is acceptable if PERF.md records it.

Put the new tests in the new file `crates/fe-project/src/tests/finder_walk_safety.rs`, declared from
`tests/mod.rs`, so they build against the base API.

## Out of scope

FileTree itself, and the T62 review's other findings (F2–F5), which T62 fixes.
