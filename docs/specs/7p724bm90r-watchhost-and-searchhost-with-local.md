---
task_uid: "01m7x6q4ykq3kyyz7p724bm90r"
task: "T724"
rev: 4
type: "refactor"
complexity: "l"
criteria:
  - id: "C1"
    text: "`WatchHost::watch(root) -\u003e Subscription\u003cVec\u003cFsChange\u003e\u003e` with ProjectWatcher as the local impl, FsChange::{Path, Rescan} unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "`SearchHost::search(root, query, options, filter) -\u003e Stream\u003cVec\u003cFileMatches\u003e\u003e` (each item one of today's sink batches, so a hosted search hands over exactly what search()'s sink got) and `files(root, filter) -\u003e Stream\u003cVec\u003cPathBuf\u003e\u003e` with search.rs (search, search_filtered, search_excluding) and the finder's file list as the local impl; both streams cancel on drop. HostSet gains no watch/search fields in this card (local_hosts() in fs_ops/host.rs is out of scope; left to 1a-wb)"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-project/tests/watch_search_host.rs: through `\u0026dyn WatchHost`/`\u0026dyn SearchHost`, a fixture gives the same matches and file list as the direct calls, and the hosted watch reports the same set of changes as the direct ProjectWatcher on one real-filesystem fixture (create, rename, removal; a set comparison, since kernel event delivery and batching are not deterministic). The deterministic, batch-for-batch watch comparison is the unit test watcher::host_tests::a_hosted_watch_delivers_the_watchers_batches_exactly, which feeds both the same synthetic events through the crate-private SOURCE hook that integration tests cannot reach. Existing fe-project, fe-nav and workbench search tests pass unchanged"
    required: true
    manual: false
---

# WatchHost and SearchHost with local impls from fe-project's watcher, search and finder, no behaviour change (remote 1b) (Mac T2045)

# WatchHost and SearchHost (remote 1b)

D § 5 (WatchHost and SearchHost rows), § 11 (Quick Open fills progressively; Find in Files streams; a new query cancels the old), § 13 phase 1b.
Read `crates/fe-project/src/{watcher,search,finder,tree}.rs` and `search/walk.rs` (T707's shared walker). Callers (project_panel.rs, search_panel.rs, quickfix_fill.rs, todo_picker.rs, lsp.rs watched files, quick_open.rs) keep calling the local code here; they switch to the window's HostSet in card 1a-wb.

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

The choices (types moved to fe-host and re-exported, `Stream`/`Subscription`
as blocking channels, batches as items, `SearchFilter` as globs, the root
named in `search`, `HostSet` left to card 1a-wb) are in
`docs/decisions/7p724bm90r-watch-search-host.md`.
