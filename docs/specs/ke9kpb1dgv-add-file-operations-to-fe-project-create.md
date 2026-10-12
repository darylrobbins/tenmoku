---
task_uid: "01m4ry9weepacw16ke9kpb1dgv"
task: "T59"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fs_ops: create_file with a/b/c.rs creates the folders and the file, a trailing / creates a folder, and an existing target is AlreadyExists with nothing changed"
    required: true
    manual: false
  - id: "C2"
    text: "fs_ops: validate_name refuses empty, ., .., NUL and empty components; a target outside the root through .. or a symlinked parent is refused"
    required: true
    manual: false
  - id: "C3"
    text: "fs_ops: rename refuses an existing target, a case-only rename succeeds on a case-insensitive volume, and duplicate names name copy.ext then name copy 2.ext, copying folders recursively"
    required: true
    manual: false
  - id: "C4"
    text: "fs_ops: trash moves a file and a folder out of the project into the system Trash (test gated to a temp dir the platform trash accepts, skipped with a message where none exists)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-project, cargo clippy -p fe-project --all-targets -- -D warnings and cargo deny check pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Add file operations to fe-project: create file or folder (with parents), rename (case-only included), move to the Trash, duplicate and relative paths, all kept under the root (Mac T510)

Filed on helium from Mac T510 (~/coordination/helium-cards/T510.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-z.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch Z to fill idle lanes; independent of helium's T47-T55 chain.

## Problem

The project tree can't create, rename, delete or duplicate anything: `crates/fe-workbench/src/project_panel.rs`
has selection, reveal and open only, and fe-project has no file operations (day-one audit 2026-10-12; user: "card
the day-one ones"). T434 covers New File (an untitled tab) and Save As only, not the tree.

## Change

New module `crates/fe-project/src/fs_ops.rs`, exported from `crates/fe-project/src/lib.rs`; no GPUI; macOS and
Linux. Every path is resolved under the project root: a target outside it, through `..` or a symlinked parent, is
refused (as `open_file_under` refuses).

- `validate_name(&str) -> Result<(), NameError>`: refuses empty, `.`, `..`, a NUL, and a component that's empty.
- `create_file(root, dir, name)`: `name` may hold `/` to create the parent folders too, nvim-tree style
  (`a/b/c.rs`); a trailing `/` creates a folder instead. Refuses an existing target (`AlreadyExists`).
- `create_dir(root, dir, name)`, likewise.
- `rename(root, from, new_name)`: within the same folder; refuses an existing target, except a case-only rename
  (`readme.md` to `README.md`) on a case-insensitive volume, done through a temporary name.
- `trash(root, paths)`: moves to the system Trash (macOS Finder's Trash; Linux the freedesktop trash) through the
  `trash` crate (MIT; add it to `crates/fe-project/Cargo.toml`). Never deletes permanently.
- `duplicate(root, path) -> PathBuf`: Finder's naming, `name copy.ext`, then `name copy 2.ext`, …; folders
  recursively, symlinks copied as links.
- `relative_path(root, path) -> String`: `/`-separated.

## Not in scope (helium respec after 3 review rounds)

Review rounds 2 and 3 widened the threat model beyond this card. These are T64's, not this card's:

- **Other processes racing an operation:** replacing or adding entries under the project while it runs, beyond
  what exclusive create (`O_EXCL`, `mkdirat`) and exclusive rename (`RENAME_NOREPLACE`, `renamex_np`) already
  refuse. Failure cleanup may assume the entries it created are still its own, and a copy may reopen its folders
  by name to set their permissions. (Round-3 findings F11 and F13.)
- **Mounts statx can't identify:** where statx fails or omits `STATX_MNT_ID` (Linux before 5.8, or a filesystem
  that leaves it out), compare devices and say so in a comment at that fallback. (Finding F9.)

Still in scope from round 3: **F12.** A duplicate keeps its extension whenever a non-empty stem, the
` copy` / ` copy N` suffix, the dot and the extension fit in `NAME_MAX` bytes; only the stem is truncated. Only when
they can't fit is the extension dropped. Test at the old `NAME_MAX / 2` threshold and at the real limit.

## Tests

New file `crates/fe-project/tests/fs_ops.rs` (temp dirs; the case-only rename test runs only where the temp
volume is case-insensitive, and says so when skipped).

## Implementation notes

- `fs_ops` reuses `listing`'s no-follow handle walk (`open_dirs`, `open_at`, `stat_at`, `read_dir`): every
  change is made with `*at` calls through handles opened from `/` without following a symlink, so a `..` or a
  symlinked folder on the way is refused with `InvalidInput` (a file on the way is `NotADirectory`). As with
  `open_file_under`, the root must be reachable without a symlink (`FileTree::root_path` is canonical). Paths
  in are absolute under the root or relative to it, as `FileTree::find` takes them; paths out are absolute.
- Ops return `io::Result`; `NameError` converts to `InvalidInput`. `validate_name` allows one trailing `/`
  (a folder); `rename` also refuses a `/` (`NameError::Separator`).
- `create_file`/`create_dir` look before they make anything, so an existing target (`AlreadyExists`) or a file
  where a folder should be changes nothing; folders made before a later failure are removed again (only while
  empty).
- `rename` is atomic no-replace (`renameat2(RENAME_NOREPLACE)` on Linux, `renameatx_np(RENAME_EXCL)` on macOS).
  A file system without it fails the rename `Unsupported` (review F2/F6): a check-then-rename could replace what
  appeared between, and a link-then-unlink could delete what replaced the source. When the target finds the entry
  itself (same dev/inode, as a case-only rename on a case-insensitive volume), it goes through a temporary name
  (`.fastedit-rename-<pid>-<n>`, not built from the entry's name, so a 255-byte name works) and puts the entry back
  if the new name is still taken (a hard link: covered on Linux too).
- `trash` checks every entry through the handles first (closing each folder handle again, so a long selection
  doesn't hold one per entry), then finds each again and moves it by what was found, not by path (review F1/F8):
  - Linux: the freedesktop.org move is done here, from the validated folder handle: the home trash
    (`$XDG_DATA_HOME/Trash`) when the entry is on the same mount (device and `statx` mount id, so a bind-mounted
    project isn't sent to a home trash a rename can't reach, review F9), else `$topdir/.Trash/$uid` (sticky, shared)
    or `$topdir/.Trash-$uid`, the top found by climbing `..` handles while device and mount hold. A volume trash
    records `Path=` relative to that top, as freedesktop.org recommends, so the entry restores with the volume mounted
    elsewhere; only the home trash records it absolute (review F14; a `bwrap` test trashes on one bind mount and lists
    the entry from another). Where statx can't
    give a mount id, devices alone are compared (a comment at the fallback says so; telling mounts apart there is
    T64, per rev 2). The `.trashinfo`
    (absolute `Path=`, percent-encoded; local `DeletionDate=`) is made first with `O_EXCL`, then the entry is renamed
    no-replace into `files/`. Restoring works: the tests read entries back with the `trash` crate's
    `os_limited::list`.
  - macOS: `NSFileManager trashItemAtURL` (no Automation permission, unlike the Finder) on a file reference URL
    (`file:///.file/id=<volume>.<file>`), which names the entry by volume and file id. Its ids are checked against
    the entry found through the handles (the volume against `st_dev` or `statfs`'s `f_fsid`), so a swap before the
    check is refused and one after it can't redirect the move.
  - **Spec deviation:** the spec named the `trash` crate; it works by path only, so it is now just a Linux
    dev-dependency (reading the trash back in tests). macOS uses `objc2-foundation` (MIT/Apache, already in the lock
    through GPUI and `trash`) directly.
  - Unit tests swap a parent folder, and the root, for a symlink both before the last check and, through a test-only
    hook, between the last check and the move.
- `duplicate` follows Finder's naming (`name copy.ext`, `name copy 2.ext`; a copy's copy counts on; a folder's
  name isn't split at a dot), with only the stem cut (at a UTF-8 boundary) to keep the name within 255 bytes (review
  F10). The extension is kept whenever a non-empty stem, the suffix, the dot and the extension fit; only when they
  can't is it treated as part of the stem (review F12; tested around the old `NAME_MAX / 2` cut-off and at the limit).
  It copies contents, permission bits and a file's mtime, symlinks as links, and skips FIFOs/sockets inside a folder
  (refused at the top), looking at the real type before opening. Each new copy folder is `chmod`ed to `0700` after
  `mkdirat`, as the umask may have left its owner no access, and cleanup opens an inaccessible folder to its owner
  before emptying and removing it (review F15; tested in a child process under umask `0700`). Children are copied in name order; folders stay
  owner-writable until the whole copy is done, then get their modes inner ones first, each reopened from the copy's
  top, so open files stay bounded by depth, not breadth (review F8). Cleanup makes its own folders writable, so a
  failed copy is removed entirely.
- Tests that change process-wide state run in a child process (the test binary again, for one test): one lowers
  `RLIMIT_NOFILE` to 64 and copies, fails to copy and trashes 300 entries; one runs under `bwrap` with a bind mount on
  the home trash's file system (skipped with a message where bubblewrap can't make it).
- The trash test makes its project under `$XDG_DATA_HOME` (or `~/.local/share`) so the freedesktop trash uses
  the home trash, then purges its own items; on macOS it uses the temp dir and removes its uniquely named items
  from `~/.Trash` where it can. The case-insensitive rename test skips with a message on Linux.
- Checked on Linux; the library also type-checks for `aarch64-apple-darwin`, but nothing here ran on macOS (the test
  binary couldn't be cross-built: criterion's C dependency needs the macOS SDK), so the reference-URL trash is
  untested there.
