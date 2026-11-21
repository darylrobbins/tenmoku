# fe-remote-proto: frames and bodies (T394, remote 2a)

The choices behind `crates/fe-remote-proto` that
[`j8wvgszah0-remote-development.md`](j8wvgszah0-remote-development.md) ("D")
left open. The strict build match (D § 2.5) means any of them can change in a
later release with no compatibility rule.

## Frames

- **`decode_all` checks the channel as well as the kind.** Hello, Welcome,
  Refused, Ping and Pong are only valid on channel 0. A `Request` must be on 0
  (`ControlOp`), 1 (`FsOp`), 2 (`WatchOp`) or 3 (`SearchOp`). Channels 4 and 5
  (git, process control) have no ops until phases 6 and 8 add them, so a
  request there is `BadChannel` for now. `Data` on channels 0–3 has a typed
  body. On a dynamic channel (64 and up) it is opaque, because only the
  channel's owner knows whether it carries LSP, pty or TCP bytes. `End`,
  `Credit` and `Cancel` are valid on any channel.
- **Bytes after a body are an error** (`Trailing`). postcard's `from_bytes`
  ignores them, so `body()` uses `take_from_bytes` and checks the rest.
- **Ping, Pong and Cancel have empty bodies** (postcard `()`). The nonce, or
  the cancelled request, is the header's `id`.
- **The encoder refuses a body over `MAX_BODY`** with `BodyTooLong`, rather
  than the spike's `assert!`.

## Bodies

- **Uploads.** D § 2.2 only describes replies. `FsOp::WriteAtomic{path, len,
  expect}` is followed by exactly `len` bytes from the client, sent as
  `FsData::Bytes` chunks on the same channel and id and counted against the
  server's credit. The server commits once all of them are in. Because `len`
  is sent first, the client never sends an `End` of its own.
- **Read.** The reply is `FsData::File{stamp, len}`, then the contents as
  `Bytes` chunks, then `End(Ok)`.
- **Write outcome.** `Written(stamp)` or `Conflict{current: Option<DiskStamp>}`
  is a `Data` frame followed by `End(Ok)`. `Status` stays the same on every
  channel: `Ok`, `Cancelled`, `LinkLost` (which only the client gives its own
  requests) or `Error(IoError{kind, message})`. `current` is `None` when
  nothing is at the path any more.
- **`Hello.location`** is `Location{destination, path}`: the remote half of
  `ProjectLocation` (D § 6), which phase 1f defines in another crate. The
  destination is only used for the server's log.
- **Stream resume.** `Resume`, `StreamOffset` and `StreamResume{From | Gap}`
  are defined because Hello and Welcome carry them. Stream channels' data
  body `Chunk{offset, bytes}` and the `recv_next` carried in `Credit` are
  left to T1991, which owns their semantics. Until then a dynamic channel's
  `Data` stays opaque.
- **Times.** `DiskStamp.modified` is `Time{secs: i64, nanos: u32}` from the
  unix epoch, with `nanos` always positive, so a time before 1970 fits too.
- **Paths.** `PathBytes(Vec<u8>)` goes on the wire through `serde_bytes`.
  postcard's encoding of it is the same, but `serde_bytes` decodes it as a
  single slice, not byte by byte. `serde_bytes` is already in the lock
  (through zvariant), so the "no new encoding crate" rule holds. The
  conversions to and from `Path` and `OsStr` are unix-only. The Windows
  client (phase 12) owns its own.

## The build id

- The build script (`build.rs`, with its logic in `build_id.rs`) sets
  `TENMOKU_BUILD` to `<CARGO_PKG_VERSION>+<commit>`. The commit is HEAD's
  12-character abbreviation. It is followed by `-dirty` when `git status`
  reports anything under a top-level entry that git tracks, untracked files
  included.
- **No commit, no build.** `TENMOKU_BUILD_COMMIT` names the commit for a
  tree without git. Without it, and without a working `git rev-parse HEAD`,
  the build fails and says to set it. A `<version>+unknown` fallback would
  let any two builds of one version pass the strict match (review ksj267
  F2).
- **The dirty check covers exactly what the script watches**, so the id
  can't go stale between reruns (reviews ksj267 and 73h3qn, F1). The script
  watches:
  - every top-level entry tracked in HEAD or the index. Cargo scans a
    watched directory whole, so an edit, a revert or a new file anywhere
    under one reruns the script. An entry that is missing is watched anyway:
    cargo reruns the script on every build while a watched path is missing,
    so deleting a tracked file and restoring it both rerun it.
  - HEAD, the index and `packed-refs`, so a commit, a checkout or staging
    reruns it. These are watched only while they exist, since a missing one
    in ordinary use would rerun the script on every build.
  - the branch HEAD names: its loose ref or, for a packed branch, the
    nearest existing directory on the ref's path, where moving the branch
    creates the ref.
- **An untracked top-level entry doesn't count.** Watching the work tree's
  root would scan `target/` too, so the status is limited to the tracked
  top-level entries with `:(top,literal)` pathspecs. A new top-level file
  counts once it's staged, since staging changes the index.
- `git status` runs with `GIT_OPTIONAL_LOCKS=0`. Otherwise it could refresh
  the index it watches, and the script would rerun on every build. A second
  `cargo build` finds the crate `Fresh`.
- `tests/build_rerun.rs` checks all of this through cargo itself. It builds
  a throwaway crate with this build script in a throwaway repository, and
  rebuilds after each change: an edit and its revert, a deleted and restored
  top-level file, an untracked file inside and outside the tracked entries,
  a commit, and a moved packed branch. `tests/build_script.rs` covers the
  override and the failure.
- The work tree is ~7,600 files, so cargo's scan costs little. Any tracked
  edit reruns the script (one `rev-parse`, two listings and one `status`).
  It then rebuilds this small crate and whatever depends on it. Nothing
  depends on it yet.
- `tenmoku --version` is unchanged. That stays an open question for the
  lead.

## Fuzzing

The fuzz workspace's `[patch.crates-io]` didn't repeat the root's
`tree-sitter` patch, so fe-syntax failed to compile there
(`tree_sitter::with_wasm_interrupt` missing). That broke every target,
not just the new one. It now repeats that patch. Every target passes
`cargo check` on stable. `cargo fuzz run` itself still needs nightly.
