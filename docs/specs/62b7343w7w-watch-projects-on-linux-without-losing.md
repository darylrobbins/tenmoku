---
task_uid: "01m4pzqaptbpfeh062b7343w7w"
task: "T63"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project test watcher::linux_tests::ignored_directories_go_last (Linux): with the watch cap set to the number of listed directories, ProjectWatcher::new succeeds, a write under src/b is delivered, a write under the gitignored target/x is not, and exactly one [FsChange::Rescan] is sent"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test watcher::linux_tests::new_directories_are_watched (Linux): after mkdir -p new/a/b and a write to new/a/b/f right away, a batch names new/a/b/f, and a later write in new/a/b is delivered too"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project test watcher::linux_tests::git_objects_are_not_watched (Linux): a write under .git/objects produces no batch; a write to .git/HEAD does"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-project passes on neon (Linux) and on macOS with the existing watcher tests unchanged, and cargo clippy -p fe-project --all-targets -- -D warnings passes on both"
    required: true
    manual: false
---

# Watch projects on Linux without losing updates to the inotify limit: watch listed directories first, skip .git/objects, keep the watches that fit and rescan once (Mac T454)

## Problem

On Linux the project tree can stop updating for good when a project has more directories than inotify watches are
left. `ProjectWatcher::new` (`crates/fe-project/src/watcher.rs` :58) asks notify for one `RecursiveMode::Recursive`
watch on the root. notify 8.2.0's inotify backend implements that by walking every directory under the root,
`target/`, `node_modules/` and `.git/objects` included, and adding a watch per directory
(`notify-8.2.0/src/inotify.rs` :400-415). The first `ENOSPC` aborts the walk with `ErrorKind::MaxFilesWatch`
(:449), so `ProjectWatcher::new` fails, and the project panel logs "won't update on changes" and runs with no
watcher at all (`crates/fe-workbench/src/project_panel.rs` :111-117). The watch limit is per user and shared with
every other program (other editors, `cargo watch`, browsers); many distributions still ship 8,192 or 65,536.
fastedit's own checkout on neon has 9,787 directories with `target/`. T61 recorded this as a known limit: "Linux/
Windows watch limits (inotify watch counts). macOS is first." Neon runs the Linux port, so it's due now.

## Change

On Linux only (`#[cfg(target_os = "linux")]`; macOS keeps its single recursive FSEvents watch unchanged), in
`crates/fe-project/src/watcher.rs`:

- Watch directories one by one with `RecursiveMode::NonRecursive`, breadth-first from the root, in two passes:
  first the directories the project listing shows (the same ignore rules as `listing::walk_builder`), then the
  ignored ones. `.git/objects` and everything under it is never watched; the rest of `.git` is (HEAD, refs and the
  index matter to git cards). Symlinks are not followed. `new` returns once both passes are done, as today's
  recursive watch does.
- When a watch fails with `MaxFilesWatch`, stop adding watches, keep the ones made (so the ignored directories are
  the ones left out), log one `warn!` saying how many directories are watched of how many and naming
  `fs.inotify.max_user_watches`, and send `[FsChange::Rescan]` once. Never fail `new` for it.
- A directory that appears later under a watched one (a create event for a directory, or any event for a path that
  is a directory not yet watched) is watched along with its subdirectories, walked the same way, and every entry
  found under it is sent as an `FsChange::Path`, so files created before its watch landed (`mkdir -p`, a checkout)
  aren't lost. This stops once the limit has been hit.
- A test seam caps the number of watches, the way `source()` (:117) swaps the event source on a test's thread, so
  tests reach the limit without changing the sysctl.

## Tests

New file `crates/fe-project/src/watcher_linux_tests.rs`, declared from watcher.rs with
`#[cfg(all(test, target_os = "linux"))] #[path = "watcher_linux_tests.rs"] mod linux_tests;`. They run on neon; on
macOS they compile out.

## Implementation notes

- The `warn!` needs the `log` crate, so `crates/fe-project/Cargo.toml` (and `Cargo.lock`) gain `log.workspace = true`,
  outside the listed scope.
- On Linux the watcher sits in an `Arc<Mutex<Option<_>>>` shared with the thread, so it can add watches; drop takes
  it out as before. Everything new is `cfg(target_os = "linux")`: macOS compiles the same code as before (the
  `Option<RecommendedWatcher>` field, its one recursive watch, its drop).
- The root is canonicalized, so each directory can be opened a component at a time from `/` without following
  symlinks (`listing::open_dirs`). Event paths now start with the canonical root, which `FileTree::apply` and
  `FinderIndex::apply` already accept.
- Listed and ignored are told apart through those handles too: the ignore files of each directory are read through
  its handle (`listing::Rules`, as the tree reads them), on top of the rules from `/` down, instead of a
  `walk_builder` walk by path. A new directory is classified from the rules above it, so a new `target/` is ignored.
- Each directory is opened, watched, then opened again; if the path no longer leads to the directory first opened,
  the watch is removed, and a directory already watched that the symlink led to (inotify shares one watch per inode)
  is watched again. Entries are read through the first handle after the watch lands, so nothing written between the
  walk and the watch is lost. The watched set keeps each directory's identity, so one replaced by another directory
  is watched again when a walk reaches it.
- Known limit, documented on `Dirs::watch_tree`: notify adds watches by path, and inotify resolves the path following
  symlinks, so a symlink swapped in between the identity check and the add redirects the watch until the check after
  it removes it. Meanwhile events in the directory it led to are named by the symlink's path, and a swap undone
  before that check goes unnoticed. Pinning a watch to the opened directory needs inotify without notify: a
  follow-up, per the lead.
- The ignored pass goes by depth below the directory walked.
- After an event with a rescan flag (an inotify queue overflow) or an error, while the limit hasn't been hit, the
  thread walks the whole project again for directories not watched, before it delivers the batch. The rescans that
  follow a burst (for FSEvents' silent losses) don't, as inotify flags its losses.
- Remove and rename-from events drop the path and everything under it from the watched set (notify drops those
  watches itself).
- The warning's total counts the directories left, walked once more without watching, when the limit is hit.
- `.git/objects` means a directory named `objects` directly in a `.git` directory, so `.git/modules/*/objects` is
  still watched.
- Tests beyond C1-C3: `ignored_directories_are_watched_by_depth`, `new_directories_put_ignored_ones_last`,
  `a_path_through_a_symlink_is_not_watched` and `a_symlink_to_a_watched_directory_leaves_its_watch` (a hook swaps
  in a symlink between a directory's open and its watch), and `a_directory_whose_event_is_lost_is_watched_after_a_rescan`
  (a test tap turns the new directory's event into an overflow's).
