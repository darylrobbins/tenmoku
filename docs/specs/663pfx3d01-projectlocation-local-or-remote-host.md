---
task_uid: "01m7x6q5xeyjr9q9663pfx3d01"
task: "T725"
rev: 5
type: "refactor"
complexity: "l"
criteria:
  - id: "C1"
    text: "`ProjectLocation::{Local(PathBuf), Remote{host: HostId, path: RemotePath}}` lives in fe-host; `parse` accepts exactly `ssh://[user@]host[:port]/abs/path` with percent-encoding for non-URL-safe bytes, and display round-trips it (unit tests in fe-host, including a non-UTF-8 path byte)"
    required: true
    manual: false
  - id: "C2"
    text: "`RecentList` stores locations; a file written by today's build still loads unchanged, and a remote entry is stored as its URI (crates/fe-config/tests/recent_locations.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "reopen.rs's launch_locations_in takes and returns Option\u003cProjectLocation\u003e roots and a remote root skips the local is_dir check; the app's own reopen entry point (launch_windows_in, called only from main.rs, which this card does not touch) keeps PathBuf roots until follow-up T734, a Known limit; ipc.rs's Target may carry a URI and `tenmoku ssh://…` is parsed by cli.rs and handed off like a path (crates/fastedit/tests/location_handoff.rs); with no remote host support yet, opening one reports a plain \"remote folders aren't supported yet\" error"
    required: true
    manual: false
  - id: "C4"
    text: "Recent picker and start page show remote entries as `app — neon`; local entries look exactly as before"
    required: true
    manual: false
---

# ProjectLocation: Local or Remote{host, path} with its ssh:// text form, persisted in recents, reopen and the IPC handoff, local behaviour unchanged (remote 1f) (Mac T2052)

# ProjectLocation (remote 1f)

D § 6 (the type, text form, "inside a window, paths stay PathBuf", the threading table), § 13 phase 1f. Design § 1 (ways in, recents rows).
This card does recents, reopen, IPC and the CLI parse. The layout, backup, undo and trust keys and the window title are card 1f-keys. Read `crates/fe-config/src/recent.rs`, `crates/fastedit/src/{reopen,ipc,cli}.rs` first. Opening a remote location for real is card 5a.

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

- `fe_host::ProjectLocation` (`crates/fe-host/src/location.rs`) with `HostId` (kind, user, host,
  port; `destination()` is `[user@]host[:port]`) and `RemotePath` (absolute bytes, no NUL).
  `parse` takes only `ssh://[user@]host[:port]/abs/path`: user and host of `[A-Za-z0-9._~-]` not
  starting with `-` (never an ssh option), or a bracketed IPv6 address; port 1–65535 without a
  leading zero; no `?` or `#`; path bytes outside RFC 3986's unreserved, sub-delims, `:`, `@` and
  `/` must be `%XX`. Display writes that canonical form (upper-case hex). `from_text`/`text` are
  the stored form: a parsing `ssh://` text is remote, anything else a local path, unchanged.
  `hosts()` is the seam card 5a fills: `Err(RemoteUnsupported)` for a remote location.
- fe-config re-exports the location types from `recent`, so fastedit (whose Cargo.toml is out of
  scope) reaches them without a new dependency. `RecentList::folders` is `Vec<ProjectLocation>`,
  serialized as strings exactly as the paths were; files stay `PathBuf`. `ProjectLocation:
  PartialEq<Path/&Path/PathBuf>` keeps the existing tests' path comparisons compiling.
- main.rs is out of scope (T729, T662 hold it), so `reopen::launch_windows_in` and
  `cli::Target {path, line, col}` keep their shapes. `launch_locations_in` carries
  `Option<ProjectLocation>` and takes whether a remote last-quit window reopens (no `is_dir`);
  `launch_windows_in` says no, keeping the entry, and writes
  `tenmoku: <uri>: remote folders aren't supported yet` on stderr. A remote `Target`'s path holds
  its `ssh://` text as given (not made absolute, no `:line`).
- Where a remote target is reported (review round 1, F1): the launched process itself, at the top of
  `ipc::start` (`start_telling`), on its own stderr, before it hands over, detaches or runs alone,
  so the terminal sees it whichever way the launch goes (a detached server's stderr is null, a
  running instance's isn't the terminal's). The running instance shows it in the app too
  (`Workbench::notify`, the URI with the error as detail), and a request of remote targets alone
  opens no window (F3). `Cli::plan` only logs. Format strings hold no words, so no l10n key.
- Known limit: a no-argument launch's reopen runs in the detached server, so its stderr line for a
  skipped remote window is lost there (it's logged). Surfacing it in a window needs main.rs, which
  T729 and T662 hold; card 5a replaces the skip with Connecting anyway.
- F2 (review round 1) disputed: `launch_windows_in` keeps `Option<PathBuf>` because its caller is
  main.rs (out of scope by the lead's decision) and `Launch::root` is a `PathBuf`; the root type is
  threaded through `launch_locations_in`, which `launch_windows_in` is built on.
- F4: a bracketed host must parse as `std::net::Ipv6Addr` (no zone).
- The recent picker and the Start page show a remote folder as `app — neon` (the Start page with
  its host parent folder as detail) and choosing one shows the existing
  `workbench-project-cant-open-folder` toast with the error as detail. The Start page row's
  invocation carries `location` instead of `path`.
- Not here: the host glyph in Open Recent (design § 1), recording remote windows' files, Connecting
  on reopen (5a), layout/backup/undo/trust keys and the window title (T726).

## Known limits

- The app's own reopen entry point, `reopen::launch_windows_in`, keeps `Option<PathBuf>` roots: its
  only caller is main.rs, which this card doesn't touch. `launch_locations_in` carries
  `Option<ProjectLocation>`, and `launch_windows_in` is built on it, skipping remote windows with
  the stderr report. Threading the location through main.rs is follow-up T734 (reopen.rs and
  main.rs, after T725).
- A no-argument launch's reopen runs in the detached server, whose stderr is null, so the line for
  a skipped remote window is only logged there (T734 or card 5a).
