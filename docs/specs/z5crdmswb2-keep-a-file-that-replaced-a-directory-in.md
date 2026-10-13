---
task_uid: "01m4tej0a6rfa1thz5crdmswb2"
task: "T110"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-project test finder_dir_replaced_by_file::file_replacing_a_directory_stays_indexed_when_its_old_child_is_reported: after directory a holding b.rs is replaced by a file a, FinderIndex::apply of both paths leaves a/b.rs out and a in"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test finder_dir_replaced_by_file::child_reported_alone_under_a_new_file_indexes_the_file: after the same replacement, applying only a/b.rs leaves a/b.rs out and a in"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-project/src/tree.rs and listing.rs are unchanged; the existing fe-project finder tests pass unchanged"
    required: true
    manual: false
---

# Keep a file that replaced a directory in fe-project's finder index when a path under the old directory is applied with it, instead of removing the new file (Mac T644)

## Problem

`FinderIndex::apply` (crates/fe-project/src/finder.rs) loses a file that replaced a directory when a path
under the old directory is in the same batch, or is reported on its own. `apply_changes` sorts the batch,
so `a` is applied before `a/b.rs`: `a` is now a regular file, so `apply_path` removes what was under it and
inserts `a`. Then `a/b.rs` goes through `Chain::reach`, which finds `a` is not a directory
(`Ok(_) => Reached::Gone(depth)`), and the `Reached::Gone` arm of `apply_path` calls `self.remove("a")`,
which removes the key `a` itself, not only what is under it. The new file disappears from the finder until
the next rescan, and since it was recorded as added the delta does not report it removed either.
`FileTree` handles the same case right ("Under a file or link: its own change says what it is"). A real
`rm -r a; touch a` is reported by FSEvents as both `a` and `a/b.rs`.

Checked 2026-10-13 at Mac e6988aba (finder.rs is identical on helium main df738a8): with `a/b.rs` indexed,
`rm -r a`, write a file `a`, then `apply([Path(root/a), Path(root/a/b.rs)])`: `contains("a/b.rs")` is
false as it should be, but `contains("a")` is false too. Applying `[Path(root/a/b.rs)]` alone also leaves
`a` unindexed.

## Change

In crates/fe-project/src/finder.rs `FinderIndex::apply_path`, the `Reached::Gone(depth)` arm reconciles the
gone ancestor as a change of its own instead of only removing it: `return self.apply_path(chain, &gone);`.
That path then takes the ordinary route: absent is removed with what is under it, a file gets
`remove_under` and is inserted (subject to the ignore rules), a symlink is removed, a directory is walked.
The existing tests in crates/fe-project (finder tests, `apply_handles_replacements_rescans_and_building`)
pass unchanged; tree.rs and listing.rs are out of scope.

## Tests

New file crates/fe-project/tests/finder_dir_replaced_by_file.rs, each test with its own tempdir, walking
with `FinderIndex::walk` + `extend` + `finish` and applying `FsChange::Path`s under `index.root_path()`:

- `file_replacing_a_directory_stays_indexed_when_its_old_child_is_reported` (regression, fails at base):
  after replacing directory `a` (holding `b.rs`) with a file `a`, applying both paths leaves `a/b.rs` out
  and `a` in.
- `child_reported_alone_under_a_new_file_indexes_the_file` (regression, fails at base): the same
  replacement, applying only `a/b.rs`, leaves `a/b.rs` out and `a` in.
