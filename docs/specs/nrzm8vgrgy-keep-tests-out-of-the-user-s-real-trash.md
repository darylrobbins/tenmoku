---
task_uid: "01m698zhrn92z0gvnrzm8vgrgy"
task: "T1254"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "Under cfg(test) or fe-project feature test-support, fs_ops::trash never calls NSFileManager trashItemAtURL or the freedesktop home/volume trash unless the calling thread holds fs_ops::testing::real_trash(); otherwise, after all existing checks, the entry is renamed from its folder's handle into a per-process test trash under std::env::temp_dir(), never falling back to the real trash, and removed when the process exits"
    required: true
    manual: false
  - id: "C2"
    text: "fs_ops::test_trash::tests::trash_goes_to_the_test_trash_not_the_system_trash (crates/fe-project/src/fs_ops_test_trash.rs) asserts a trashed temp file is in testing::trash_dir() (same inode) and that the real backend was not called"
    required: true
    manual: false
  - id: "C3"
    text: "tests::trash_isolation::project_panel_delete_goes_to_the_test_trash (new crates/fe-workbench/src/tests/trash_isolation.rs) deletes a file through project_panel.delete and asserts it landed in fe_project::fs_ops::testing::trash_dir()"
    required: true
    manual: false
  - id: "C4"
    text: "fe-project (self dev-dependency), fe-workbench and fastedit dev-dependencies enable fe-project/test-support; the fastedit release build does not"
    required: true
    manual: false
  - id: "C5"
    text: "The tests listed in the spec no longer make projects under HOME or XDG_DATA_HOME and drop their ~/.Trash purges; macOS real-NSFileManager coverage is #[ignore] holding real_trash(); the Linux freedesktop and bind-mount tests hold real_trash() and keep their verified purges"
    required: true
    manual: false
  - id: "C6"
    text: "cargo test -p fe-project and cargo test -p fe-workbench pass, and cargo clippy --workspace --all-targets is clean"
    required: true
    manual: false
---

# Keep tests out of the user's real Trash: fs_ops::trash goes to a per-process test trash in test builds

## Problem

Test runs fill the user's real macOS Trash (~/.Trash): 100k+ tiny files named
`fe-workbench-trash-*`, `renamed.rs`, `link.rs`, `d.rs`, `fe-project-trash-*`
and `000.txt`..`299.txt`, one batch per run. `fe_project::fs_ops::trash` always
reaches the system trash (macOS: `NSFileManager trashItemAtURL`, Linux: the
freedesktop home/volume trash), and the tests that reach it either never purge
or purge `~/.Trash/<name>` with `let _ = fs::remove_file(..)`, which fails
silently with EPERM without Full Disk Access (a terminal or regatta lane has
none). Linux purges through `.trashinfo`/`trash::os_limited` do work.

Tests that reach the real Trash today:

- fe-project `tests/fs_ops.rs`
  - `trash_moves_a_file_and_a_folder_to_the_system_trash`: `fe-project-trash-*.txt` and `-folder`; macOS purge silently fails.
  - `broad_folders_and_selections_need_few_open_files_child`: trashes 300 files `000.txt`..`299.txt`; no purge at all on macOS.
- fe-project `src/fs_ops.rs` unit tests `tests::trash_never_follows_a_parent_swapped_for_a_symlink` and `tests::trash_never_follows_a_root_swapped_for_a_symlink`: the `Swap::AfterCheck` case trashes `fe-project-trash-*.txt`; macOS purge silently fails.
- fe-workbench `src/project_panel_ops_tests.rs` `delete` and `src/project_panel_filter_tests.rs` `ops_delete`: project made in `$HOME` (`trashable_project`, prefix `fe-workbench-trash-`), trashed `fe-workbench-trash-*.txt` / `-main.rs` / `-only.txt`; macOS purge silently fails.
- fe-workbench `src/tests/lsp_file_operations.rs` `tree_operations_notify_servers`: trashes `src/renamed.rs`; no purge on any platform.
- fe-workbench `src/workspace_edit_ops_tests.rs` `renames_creates_and_deletes_apply_in_order` (`src/d.rs`) and `a_create_over_a_symlink_replaces_the_link` (`src/link.rs`, via `workspace_edit.rs` create-over-a-symlink): purge reads freedesktop `.trashinfo` only, so a no-op on macOS.

Linux-only real-trash tests (`trash_moves_to_the_volume_trash_off_the_home_volume`, the bind-mount tests, `fs_ops_race_tests.rs` bind-mount case) list and purge what they trashed, and stay real.

## Change

One seam in `fs_ops`, so no test can reach the user's Trash whatever path it takes:

- fe-project gets a `test-support` feature. Under `cfg(any(test, feature = "test-support"))`,
  `trash_target` keeps every existing check (handles, reference-URL check on
  macOS, `reach(Point::Checked)`), then, in place of the final
  `trashItemAtURL` / `freedesktop::move_into`, renames the entry from its
  folder's handle (`renameat`, so it follows the entry found, as the real path does) into a
  per-process test trash under `std::env::temp_dir()` with a unique name.
  It never falls back to the real trash: a cross-volume rename (EXDEV) is an
  error. The test trash is removed when the process exits (e.g. `libc::atexit`,
  or stale `fe-test-trash-<pid>` dirs of dead pids removed when one is made).
- The real backend runs in those builds only while the calling thread holds an
  opt-in guard, `fs_ops::testing::real_trash()` (thread-local: parallel tests
  are unaffected). Put the test backend in a new file
  `crates/fe-project/src/fs_ops_test_trash.rs` declared from `fs_ops.rs`, with
  `fs_ops::testing` exposing `trash_dir()` and `real_trash()`.
- Enable the feature for every crate whose tests can reach `fs_ops::trash`:
  fe-project's own integration tests (self dev-dependency with
  `features = ["test-support"]`), fe-workbench and fastedit dev-dependencies.
  Release/normal builds never enable it.
- The offending tests use ordinary temp projects (drop `trashable_project` /
  `trashable_temp`'s `$HOME` and `XDG_DATA_HOME` bases) and drop their
  `~/.Trash` purges; where they checked "it went to the Trash", check it is in
  `testing::trash_dir()`.
- Real behaviour stays covered but opt-in: macOS NSFileManager coverage moves
  to `#[ignore]` tests holding `real_trash()` (run by hand with `--ignored`);
  the Linux freedesktop tests above hold `real_trash()` and keep their verified
  purge.

Production behaviour is unchanged (feature off, cfg(test) off).

## Implementation notes

- `fs_ops::testing` exposes `trash_dir()`, `real_trash()` (guard `RealTrash`,
  not `Send`) and `trashed(name)`: every entry named `name` in the test trash.
  Each trashed entry gets a numbered folder of its own in
  `$TMPDIR/fe-test-trash-<pid>`, keeping its name (no name-length limits).
- The test trash is removed by an `atexit` handler, and stale ones of dead
  pids (`kill(pid, 0)` = `ESRCH`) when the next is made; folders are given
  their owner's access before removal.
- On Linux the test path skips the volume lookup and trash-dir creation
  (`Volume::of`, `home_trash`, `volume_trash`) as well as the move: they only
  choose a trash, and making one would touch the real trash dirs.
- The real-trash variants of the moved tests are `#[ignore]` on both
  platforms: `tests/fs_ops.rs` `trash_moves_a_file_and_a_folder_to_the_system_trash`
  and `fs_ops::tests::the_system_trash_never_follows_a_{parent,root}_swapped_for_a_symlink`.
  The Linux volume-trash and bind-mount tests hold `real_trash()` and still run.
