---
task_uid: "01m7x6q2tqzffpr7h6ngbbm2jd"
task: "T722"
rev: 2
type: "refactor"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-host is a new crate outside UI_CRATES (scripts/check-no-gpui.sh passes) that depends on no fe-* crate that will depend on it (fe-project, fe-buffer, fe-editor, fe-terminal, fe-git, fe-workbench); `DiskStamp` moves into it and fe-buffer re-exports it"
    required: true
    manual: false
  - id: "C2"
    text: "`FsHost` has read -\u003e (bytes, DiskStamp), write_atomic(path, bytes, WriteExpect) -\u003e Result\u003cDiskStamp, Conflict{current}\u003e, stat, list_dir and canonicalize; `WriteExpect` is D § 7's four cases and `HostSet` holds one Arc per trait (only FsHost so far)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor's SaveJob commits through LocalFs (temp file, re-check before the rename, xattr preservation, persist_noclobber or create_in_place for Absent and an empty AsFound), with SaveJob mapping to WriteExpect exactly as D § 7 lists"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fe-host/tests/local_fs_write_expect.rs: `absent_refuses_a_file_created_before_commit`, `matches_conflicts_when_the_stamp_changed`, `as_found_on_an_empty_path_does_not_clobber_a_newcomer` and `overwrite_replaces_whatever_is_there` pass, the races made at a boundary hook, not by timing"
    required: true
    manual: false
  - id: "C5"
    text: "fe-buffer, fe-editor and fe-workbench loader tests pass unchanged"
    required: true
    manual: false
---

# fe-host: a new GPUI-free crate with FsHost, WriteExpect and HostSet, whose LocalFs takes fe-editor's save commit and the loader's read, with no behaviour change (remote 1a) (Mac T2043)

# fe-host and FsHost (remote 1a)

D § 5 (the crate, the traits table, "moved rather than rewritten", phase 1 changes no behaviour), § 7 ("Write preconditions": WriteExpect and the SaveJob mapping, the no-clobber commit, Conflict), § 13 phase 1a.
Read `crates/fe-editor/src/disk.rs` (SaveJob), `crates/fe-buffer/src/buffer.rs` (DiskStamp, read_file at ~639) and `crates/fe-workbench/src/loader.rs` first. The loader passes bytes from the host to `Buffer::read_file`'s equivalent. The workbench does not hold a HostSet yet (card 1a-wb); call LocalFs directly where the code was.
fe-host also gets `ProjectLocation` in card 1f; leave room for it.

## Rules
Read the decision sections named above in `docs/decisions/j8wvgszah0-remote-development.md` (and the design file `docs/designs/remote-development.md` where cited) before starting; they are the spec, this card only cuts a slice of them. Do not widen --scope; if a file outside it is needed, stop and respec (notes and choices go in your lane's `docs/decisions/<lane>-*.md`, which is exempt). New tests go in new files, one fn per behaviour, small fixtures, no sleeps for timing (D § 12 spike notes). Gate tests on the affected crates only. Not here: anything the decision assigns to another phase (D § 13).

## Helium filing note

- Lent from the Mac as y156 (remote development, the fe-host foundation). Neon and other hosts hold sibling
  chains (protocol, server), so keep strictly to this card's scope; ask the lead for a respec rather than
  widening it yourself.
- Lead decisions on the T1986 decision file's open points: the client crate is fe-remote-client and the
  server is crates/tenmoku-server; pick the async runtime yourself but keep the protocol crate
  runtime-agnostic; tenmoku --version stays as it is. Read docs/decisions/j8wvgszah0-remote-development.md
  section 13 and its design doc first.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp. Disk is tight on helium: ask the lead
  (`herdr agent prompt wR7:p1 ...`) before rgt finish.
- Criteria tests need full cargo test paths (`fe-x::binary name` when a name is in several binaries).
- Ask the lead before rgt finish if a criterion can't be met as written. Run rgt finish once, and wait for its
  result before you report.

## Implementation notes

Choices, and the one deviation from D § 7, are in
`docs/decisions/h6ngbbm2jd-fe-host.md`: `Absent` follows a dangling link
present when the write starts (a Save As refuses one through
`CommitOptions::at_the_path`, moved into the commit after review F2), and
`Matches` is strict in `FsHost` while a save still writes a file deleted
since, through `CommitOptions::deleted_is_written` (review F1, round 2: pinned
by fe-editor `tests/save_conflict.rs` and fe-workbench `disk_sync_tests.rs`).
