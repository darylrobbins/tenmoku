---
task_uid: "01m7x6q3vzctf3755zajsn5anw"
task: "T723"
rev: 3
type: "refactor"
complexity: "l"
criteria:
  - id: "C1"
    text: "FsHost gains the tree operations fe-project's tree and tree undo use today (create recorded, rename, duplicate/copy_into/move_into with paste names, trash recorded and restore by EntryId, exists, listing a directory into `Listing`), each taking the host and keeping today's beneath-root rules"
    required: true
    manual: false
  - id: "C2"
    text: "The local impl is today's fs_ops.rs/listing.rs code (moved or delegated to, not rewritten); the existing fe-project tests, including fs_ops_race_tests and the seam hooks, pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-project/tests/fs_host_tree.rs: each tree operation through `\u0026dyn FsHost` gives the same result as the old free function on a fixture tree, and a trash then restore round trip keeps the EntryId"
    required: true
    manual: false
---

# fe-project listing and tree operations through FsHost: listing, create, rename, duplicate, copy, move, trash and restore as FsHost methods with today's code as the local impl, no behaviour change (remote 1a, tree) (Mac T2044)

# Tree operations through FsHost (remote 1a, tree)

D § 5's FsHost row is abridged (create, rename, remove, trash): the tree and tree undo need the recorded forms (`create_recorded`, `trash_recorded`/`restore`, `duplicate_recorded`, `copy_into_recorded`, `move_into`, `paste_name`, `exists`, `open_truncated`). Give FsHost what those callers need, with the wire in mind (no `File` handles across the trait: return paths, EntryIds and bytes). D § 11: tree operations are confirmed by the host, nothing optimistic.
fs_ops.rs is ~3,300 lines: prefer the trait in fe-host with the local impl living in fe-project over moving the code (fe-host cannot depend on fe-project); record the choice in the lane decision file. Callers in fe-workbench switch in card 1a-wb.

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
