---
task_uid: "01m46hrpv5vkjs3crzy747ewvy"
task: "T61"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project test tests::tree::open_lists_only_the_root: FileTree::open on a nested fixture loads exactly the root entries (stats().listings == 1, loaded_entries equals the root entry count), ordered directories first, then by name case-insensitively"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test tests::tree::expand_lists_a_directory_once: expand lists only that directory (listings +1, loaded_entries rises by its child count); collapse hides its rows; expanding it again does no listing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project test tests::tree::listing_runs_off_the_tree: Listing is Send (static assertion); listing(id), then Listing::run, then fill(id, result) gives the same rows as expand; listing() of a loaded directory is None Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C4"
    text: "fe-project test tests::tree::ignored_entries_and_git_are_hidden: entries matched by the root .gitignore, a nested .gitignore and an .ignore file are absent, also outside a git repo; .git is never listed; non-ignored dotfiles are listed; with show_ignored: true the ignored entries appear with ignored set and .git still does not Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C5"
    text: "fe-project test tests::tree::rows_flatten_expanded_directories: row_count() and rows(range) give depth, name, kind and expanded state in display order across nested expanded directories, and rows(range) yields exactly range.len() rows Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C6"
    text: "fe-project test tests::tree::apply_reconciles_loaded_directories: after creating, deleting and renaming files and a directory on disk, apply with their FsChange::Path entries inserts in sorted position, removes nodes with their loaded subtrees, moves renamed entries (a renamed directory comes back collapsed), returns the changed directory ids and bumps version(); applying the same batch again returns no ids and leaves version() unchanged Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C7"
    text: "fe-project test tests::tree::changes_outside_loaded_directories_do_no_io: 10,000 FsChange::Path entries under an unloaded directory and under an ignored directory cause 0 listings and 0 stats and load nothing Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C8"
    text: "fe-project test tests::tree::rescan_relists_each_loaded_directory_once: FsChange::Rescan raises stats().listings by exactly the number of loaded directories and reconciles their children with the disk (excluding a batch that holds both Path(root) and Rescan, review F6, a known limit; see Decided) Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C9"
    text: "fe-project test tests::tree::unreadable_directory_keeps_an_error: expanding a directory with mode 000 records an error on its row, and other rows and later expansions keep working (skipped when running as root) Judged with listings run and filled one at a time, in order; overlapping or delayed listing interleavings are out of scope (see Decided)."
    required: true
    manual: false
  - id: "C10"
    text: "fe-project test tests::large::a_100k_file_tree_loads_lazily: on a generated fixture of 100,000 files (100 directories x 1,000 files, made in a temp dir by fe_project::fixture), open plus one expand makes exactly 2 listings and loaded_entries equals the root entry count plus 1,000; rows(0..50) yields 50 rows"
    required: true
    manual: false
  - id: "C11"
    text: "fe-project test tests::watcher::watcher_batches_create_rename_delete: a real ProjectWatcher on a temp dir delivers batches (waited for with a bounded recv_timeout; no timing is asserted) that, applied to the tree, show a created file, then its rename, then its deletion; a path repeated within one batch appears once"
    required: true
    manual: false
  - id: "C12"
    text: "scripts/check-no-gpui.sh prints ok: fe-project, and cargo deny check licenses bans sources passes with ignore and notify added"
    required: true
    manual: false
  - id: "C13"
    text: "crates/fe-project/benches/tree.rs builds in the gate and measures open of the 100k fixture root, expand of a 10k-file directory and apply of a 1k-path batch; PERF.md gains a Project tree section with one run of these numbers (recorded, not asserted)"
    required: true
    manual: false
---

# Add fe-project: a lazy, gitignore-aware file tree kept current by a file watcher

# Add fe-project: a lazy, gitignore-aware file tree kept current by a file watcher

Milestone M3 "Browse", card 1 of the first batch (approved direction, 2026-10-05). The plan's
Architecture table: `fe-project` is the "lazy file-tree model, `ignore`, `notify` watcher, `frizbee`
finder index, ripgrep project search", GPUI-free. This card is the tree and the watcher. The finder
index (card 2) and the workbench (card 3) build on it. Project search is a later card.

## Problem

There is no project model. `crates/fastedit/src/main.rs` paints a "PROJECT" placeholder on the right.
M3 is done when "the tree handles a 100k-file repo". That needs a tree that reads a directory only when
it is expanded, hides what git ignores, and stays current while files change on disk. The UI also has
to be able to read it cheaply every frame.

## Change

New crate `crates/fe-project` (GPUI-free; `fe-project = { path = "crates/fe-project" }` in the
workspace `Cargo.toml`, with `ignore = "0.4"` and `notify = "8"` added to `[workspace.dependencies]`).
`members = ["crates/*"]` picks it up. `scripts/check-no-gpui.sh` checks every crate not in
`UI_CRATES`, so `fe-project` is covered with no script change. It must print `ok: fe-project`.

Files:

- `crates/fe-project/Cargo.toml`, `crates/fe-project/src/lib.rs`
- `crates/fe-project/src/listing.rs`: lists **one** directory with `ignore::WalkBuilder`
  (`max_depth(Some(1))`, `parents(true)` so ancestors' ignore files apply, `hidden(false)` so dotfiles
  show, `require_git(false)` so `.gitignore` applies outside a git repo too, `follow_links(false)`).
  `.git` is never listed. It returns entries (name as `OsString`, kind: file, directory or symlink,
  and an `ignored` flag when ignored entries are shown). It may cache parsed ignore matchers per
  loaded directory instead of re-reading ancestors each time; that's an implementation choice.
- `crates/fe-project/src/tree.rs`: `FileTree`
  - `FileTree::open(root, TreeOptions) -> io::Result<FileTree>` lists the root only.
    `TreeOptions { show_ignored: bool }` defaults to `false`.
  - Nodes in an arena with stable `NodeId`s. Children are sorted directories first, then by name
    case-insensitively, with ties broken by byte order.
  - The listing is split so the UI can do the IO off its thread: `FileTree::listing(id) -> Option<Listing>`
    (`None` when already loaded). `Listing` is `Send` and `Listing::run()` does the IO. Then
    `FileTree::fill(id, result)` stores it with no IO. `expand(id)` is the blocking convenience:
    listing, run, fill, then mark it expanded. `collapse(id)` keeps the loaded children.
  - A failed listing (for example permission denied) is stored on the node as an error. The rest of
    the tree is unaffected.
  - A flattened view for a virtual list: `row_count()` and `rows(range) -> impl Iterator<Item = Row>`.
    `Row` carries id, depth, name, kind, expanded, ignored, and error. The flattened order is cached
    and rebuilt only when expansion or contents change. Reading a range costs O(range), not O(tree).
  - `version()` increases whenever rows change.
  - Counters for tests and benches, `stats() -> TreeStats { listings, stats, loaded_entries }`.
    `stats` counts `symlink_metadata` calls.
- `crates/fe-project/src/watcher.rs`:
  - `FsChange` is either `Path(PathBuf)` (something at this path may have changed) or `Rescan`
    (notify flagged a rescan or dropped events).
  - `ProjectWatcher::new(root, sink: impl FnMut(Vec<FsChange>) + Send + 'static)` uses notify's
    `RecommendedWatcher`, which is recursive FSEvents on macOS. A watcher thread drains notify's
    channel into batches and dedupes paths within a batch. The tree doesn't own the watcher, so tests
    and the UI can inject batches.
  - `FileTree::apply(&[FsChange]) -> Vec<NodeId>` returns the directories whose children changed. It
    **reconciles** instead of trusting event kinds, because FSEvents coalesces and reports renames as
    unpaired paths. A path whose parent directory is loaded is re-stat'ed: present means insert or
    update, absent means remove the node and its loaded subtree. A path whose parent isn't loaded, or
    which falls under an ignored or unlisted directory, is dropped with no IO. A renamed directory
    reappears collapsed and unloaded. `Rescan` re-lists every loaded directory once.
- `crates/fe-project/src/fixture.rs`: `#[doc(hidden)] pub fn generate(root, dirs, files_per_dir)`.
  It writes a tree of empty files plus a `.gitignore` and an ignored directory, so tests, benches and
  card 2/3 tests share one generator. Keep it fast: create files directly with no writes.
- Tests in new files: `crates/fe-project/src/tests/mod.rs` (declared `#[cfg(test)] mod tests;` from
  `lib.rs`), `tests/tree.rs`, `tests/watcher.rs` and `tests/large.rs`. The 100k-file test generates its
  fixture in a `tempfile` temp dir, with a single test doing it, so `nextest`'s process-per-test doesn't
  repeat the work.
- Bench `crates/fe-project/benches/tree.rs` (criterion, `harness = false`): `open` on the 100k
  fixture's root, `expand` of a directory of 10k files, and `apply` of a 1k-path batch into loaded
  directories. Record one run's numbers in a new "Project tree (`cargo bench -p fe-project --bench
  tree`)" section of `PERF.md`. No budget is asserted.

Respect the workspace lints: `unreachable_pub` is warned and the gate runs clippy with `-D warnings`,
so items in private modules are `pub(crate)`.

## Out of scope

- The finder index (card 2), project search, and any UI.
- Re-filtering loaded directories when a `.gitignore` changes. A `.gitignore` edit is treated like any
  other path. A later card can re-list the directories under it.
- Linux/Windows watch limits (inotify watch counts). macOS is first.

## Decided (lead defaults; the user may revisit)

- Gitignored entries are hidden by default (`show_ignored: false`); the flag keeps the dimmed
  VS Code/Zed style possible later.
- `.gitignore` applies outside git repos too (`require_git(false)`).
- (After review rounds were exhausted, with the user's agreement.) Four review findings are accepted as
  known limits of this card and moved to a follow-up card, so the review cycle ends here:
  - F5: an overlapping child listing rejected after a newer stored listing, and the root-recovery
    marker on the root itself, can let an older parent snapshot drop a restored child until the next
    event or rescan for it.
  - F6: a batch with both `Path(root)` and `Rescan` after an ancestor of the project is deleted keeps
    the deleted rows.
  - F7: the ignore matcher consults `.gitignore` and `info/exclude` above the nearest repository.
  - F8: a linked worktree's `.git` gitfile isn't followed, so the common repository's `info/exclude`
    isn't loaded.
  C3 and C8 are judged without these cases. Record them under a "Known limits" heading in
  `## Implementation notes`.
- (User decision, after the rev-3 review raised a further race, F1: a delayed listing can use ignore
  context that a rescan made stale.) **Concurrency races are out of scope for this card.** Every
  criterion is judged with listings that run and fill one at a time, in order. Any interleaving of
  overlapping, delayed or rejected listings with events, rescans or root recovery (F1, F5, F6 and
  any like them) is a known limit, fixed in the follow-up card T88. Reviews of this card shouldn't
  raise new findings of that kind; list any under "Known limits" instead.

## Implementation notes

- **Listings read through directory handles, not `ignore::WalkBuilder`.** Review required a
  listing's enumeration and ignore-file reads to be anchored to checked directory handles, so a
  path swapped for a symlink in the middle of a listing can never show or apply anything outside
  the project. `WalkBuilder` opens everything by path, so it can't do that. `Listing::run` opens
  `/` and then each directory down to the listed one with `openat(O_NOFOLLOW | O_DIRECTORY)`,
  including the root's ancestors. Directories above the listed one are opened search-only
  (`O_SEARCH` on macOS). On the way it checks the root and each directory against the identity the
  tree has for it, and then reads the listed directory's entries with `fdopendir`/`readdir` on its
  handle. It reads `.ignore`, `.gitignore` and `.git/info/exclude` through the handle of each
  directory from `/` down. Afterwards the path is opened and checked again; if it no longer leads
  to the directory, the result says which name stopped leading to it (see "Listings racing
  `apply`").
- **The rules are the `ignore` crate's**, combined as `WalkBuilder` combines them with the spec's
  configuration (`parents(true)`, `hidden(false)`, `require_git(false)`, `follow_links(false)`,
  one level deep). Each file is parsed by its `gitignore::GitignoreBuilder`. The nearest matching
  `.ignore` rule decides, else the nearest `.gitignore` rule, else the nearest
  `.git/info/exclude` rule, else git's global excludes. The global excludes are read once per
  tree, relative to the current directory, as the walker reads them. Unit tests use none, so they
  don't depend on the machine's git configuration;
  `git_global_excludes_rank_below_the_projects_ignore_files` checks where they rank.
  `listings_and_apply_filter_as_the_ignore_walker_does` compares listings and `apply` with
  `WalkBuilder` configured that way. Its rules cover negations, anchored, directory-only and
  nested rules, `.ignore` over `.gitignore`, and git's exclude file, with and without
  `show_ignored`. There are two deliberate differences. Ignore files are opened with `O_NOFOLLOW`
  (git has opened in-tree `.gitignore` files that way since 2.32), so a symlinked one is skipped.
  They are also opened non-blocking and used only if they are regular files, so a FIFO is never
  waited on.
- **Ignore checks in `apply`** use the same rules. Each `apply` reads them again through the
  handles it opened, once per directory per batch, so no cached rule can go stale: a replaced root
  or directory, or an edited ignore file, is read anew by the next batch. They are consulted only
  for an entry found in its opened directory. Entries already loaded are not re-filtered when
  rules change (out of scope).
- **Ignored entries when shown:** a listing flags each entry the rules ignore. Everything under an
  ignored directory is flagged ignored, as git treats it, even where no pattern matches the child
  itself.
- **Which paths cost IO:** `apply` walks a path down through listed directories using only the
  tree. A path is stat'ed only when its parent is a listed directory. Below an unlisted directory
  it marks that directory (next point) and stops. Below a name the tree doesn't have (unknown,
  ignored or `.git`) or a file, it is dropped, with no opens, stats or ignore-file reads (the
  tests count all of these in a crate-private `io_calls`). The one exception is a name that
  differs from a node's only in case, which re-lists the parent (a case-only rename; see below).
- **A directory's own path:** if it names a listed directory that is still there as itself, that
  directory and every loaded directory below it are listed again at the end of the batch. It may
  have been moved away, changed and moved back between two looks, and then its changes were
  reported under the other name. FSEvents reports a directory's own path only for changes to the
  directory itself (renames, permissions), never for changes inside it (checked with the real
  watcher), so this costs IO only for those. The root's own path does the same for everything (a
  rescan); the root may also be gone or another directory. Directories below that aren't loaded
  are marked as owing a listing, in case one of them is running.
- **`apply` holds few directories open:** it handles a batch's paths in sorted order. Before each
  path it closes the directories not on that path's way. A batch therefore holds at most one
  path's worth of directory handles, plus those above the root, however many loaded directories
  it touches. A listed directory that `apply` can't reach in a batch, but whose node is still
  there afterwards, is re-listed at the end of the batch, after the handles are closed, since the
  changes in it couldn't be looked up. That covers a directory (or one above it) gone only for a
  moment, and one that can't be read (permissions, or no descriptor left): the listing records the
  error on its row, or recovers.
- **Listings racing `apply`:** the tree has a clock shared with its `Listing`s. It ticks at the
  start and end of every `Listing::run`, and at each observation `apply` makes, timed after the
  last IO the observation relies on (the lookup, and the ignore files that classify it). Each
  directory records three times:
  - when the run of its stored listing started (`listed_seq`);
  - when `apply` last dropped a change because the directory was unlisted (`dropped_seq`, also set
    for paths deeper below it and for its own path, by rescans for the unlisted directories below
    each one they list, and for everything when a cut-off root is reached again);
  - when its children, or anything loaded below them, were last brought up to date
    (`applied_seq`). This is set by each lookup `apply` makes, by each listing of it stored, as of
    when that listing ended, and by each listing of a directory below stored or removing a node.
    Every dropped change sets it too, being news that the directory is there. It is passed up to
    every directory above.

  A result whose run started before the stored listing's, or before `applied_seq`, may be older
  than what is known. A listed directory discards it, since it could undo newer news below:
  remove a subdirectory `apply` has since updated or seen a change under, bring back one a listing
  has since found gone, or overwrite what an overlapping listing read later. A directory not
  listed yet stores it. Either way the directory owes a refresh (`listing(id)` keeps returning
  `Some`), unless the run ended before the stored listing started. So of listings finishing in
  any order, with or without an `apply` between, the newest is kept, and nothing newer is lost
  without another listing owed. A result that may have missed a dropped change or a `Rescan`
  (`max(dropped_seq, rescan_seq) > listed_seq`) is stored and owes a refresh too. Timing
  observations after they are made, rather than once per batch, makes a listing that started in
  the middle of an `apply` count as older than what that `apply` saw after it started. Several
  tests start listings at fixed points inside `apply` and inside a rescan to check this.

  A result saying the directory was gone or replaced is discarded if anything looked at the
  directory, or at its parent, since its run started (as `Rescan` stores a listing). The node
  stays and `listing(id)` still offers it; the watcher's event for the path removes the node if
  the directory is really gone. Otherwise the node is removed. On its way down a listing checks
  the root and each directory against the identity the tree has for it, so it can say which name
  stopped leading to the directory: its own, or one above it (missing, another directory, or a
  symlink). In the second case the node is never removed: a directory above that was gone or
  replaced for a moment comes back with the same identity, and its events never restore what was
  removed below it. The node keeps what it had loaded (one with nothing loaded keeps the error,
  and can be listed again). If that directory is really gone, its own events remove it, subtree
  and all; if it came back, its own path lists it and everything loaded below it again (see "A
  directory's own path").
- **Directory identity:** nodes keep a directory's `(device, inode)` as `stat` reports it. It comes
  from the opened directory (`fstat`) or an entry looked up in its opened parent
  (`getattrlistat`/`fstatat`), never from `readdir`'s `d_ino`, which at mount points and macOS
  firmlinks (`/Users`) is the covered directory's. A listing reads its subdirectories' identities
  through the directory it opened. A directory at a known name with another identity (a swap
  through a temporary name, a delete and re-create) replaces the node, collapsed and unlisted. That
  holds in `apply` and in listing reconciliation (so `Rescan` too).
- **The root:** its identity is adopted from each listing of it rather than checked. A root
  replaced by another directory is therefore re-listed and its children reconciled. The re-listing
  comes from `Rescan`, from the root's own path, or from `apply` finding another directory when it
  opens the root. A deleted root, or one whose own name now holds a symlink, fails its listing. A
  failed root listing leaves it unlisted with the error. Paths other than the root's own are then
  dropped and mark it, `listing(root)` offers it again, and `Rescan` always lists the root.
- **A root cut off above it:** FSEvents reports nothing when a directory above the watched root
  moves (notify doesn't ask for `WatchRoot`; checked). So a listing that can't reach the root as
  the tree has it (past a directory above it, or because the root itself is another directory
  now: `Lost::Root`) leaves what is loaded as it was, rather than dropping everything, and
  `listing(root)` offers the root. Every such result is recorded with the time its run ended,
  even one `fill` then discards. Changes made meanwhile go unreported, so the root counts as
  found again only through a listing of it that is stored. Then every directory, loaded or not,
  owes a listing, and so does the root if its listing started before the last result that
  couldn't reach it ended. `apply` reaching the root again does the same, listing everything in
  that batch. A result that couldn't reach the root but ended before the last stored listing of
  the root started is old news. One exception: if the root's own path is reported while it can't
  be reached that way, the root went with what was above it (a moved directory above reports
  nothing, a deleted one reports everything below), and the tree drops everything, as for a
  deleted root. A case-only rename of the root itself silences the FSEvents stream; the tree
  can't tell, and that is the watcher's limit.
- **No following substituted symlinks:** `apply` opens directories the same way as listings: the
  root from `/`, each directory below it in its opened parent, once per batch. It checks each
  against the node's identity, then looks entries up in the opened parent with
  `AT_SYMLINK_NOFOLLOW`/`FSOPT_NOFOLLOW`. If a directory no longer opens as the one the tree has,
  its name is reconciled in its parent instead. A directory (or an ancestor) replaced by a symlink
  so becomes a symlink node, and nothing outside the project is opened, stat'ed or read. Because
  entries and ignore files are read through handles opened that way, a swap after the checks
  changes nothing either. `listings_read_the_directory_they_checked` and
  `apply_reads_rules_through_the_directory_it_checked` swap directories for symlinks (or other
  directories) at fixed points inside a listing and inside `apply`, in both listing modes. This
  holds on every Unix, not only macOS.
- **Test hooks:** `listing::checkpoint` marks those fixed points: a listing that has opened and
  checked its directory, a listing that has read it, `apply` having looked an entry up but not yet
  read rules, and `apply` having failed to reach a directory but not yet looked its name up in its
  parent. Tests set a thread-local hook there; outside tests it does nothing.
- **Case-only renames:** on APFS a lookup of `Foo` succeeds after `Foo` is renamed to `foo`, so a
  plain stat can't tell which spelling is on disk. `getattrlistat` returns the on-disk name. Some
  cases make `apply` re-list the parent once per batch (counted in `listings`):
  - the on-disk name differs from the path's;
  - the tree holds a sibling whose name differs only in case. This is checked also for a path below
    the renamed name, before that path would be dropped as ignored or `.git`;
  - a directory `apply` opens on the way, under its old spelling, now has a different name in its
    parent.

  Either name of a rename may arrive alone, in either order, also only as a path below it, and
  repeating a batch changes nothing. These checks run only where the root's volume ignores case
  (`fpathconf(_PC_CASE_SENSITIVE)`, checked when the tree is opened). On a case-sensitive volume
  `Build/` and an ignored `build/` are different directories, and paths below `build/` are
  dropped without IO like any other. Off macOS (no `getattrlist`) case is assumed to matter, so
  the test is macOS-only.
- **Errors:** a failed listing leaves the directory unlisted with the error on its row, so
  `listing(id)` (and `expand`) try again. A loaded directory that fails a rescan or a re-listing
  drops its children. An entry `apply` can't look up for a reason other than its being gone
  re-lists its directory, as a directory it can't open does.
- **Node ids** carry a generation, so an id held across a removal never resolves to another node.
- **Rows** are rebuilt in full (O(visible rows)) after a mutation only when it changes something
  visible; reading a range is a slice.
- **Bench fixture:** 10 directories of 10,000 files (100,000 files), so one fixture serves both the
  100k-file `open` and the 10k-file `expand`. The C10 test uses 100 x 1,000 as specified. Creating
  files on two threads was fastest on APFS (one: ~5 s, two: ~3 s, four or more: slower; hard links
  to one inode: ~28 s).

### Known limits

Review findings accepted as known limits of this card, with the user's agreement, once the review
rounds ran out (see Decided). Concurrency races are out of scope for this card: every criterion is
judged with listings that run and fill one at a time, in order. The race findings (F1, F5, F6, and
any like them) are fixed in the follow-up card T88.

- **F1:** a listing keeps the ignore context of its directory from when it was requested. With
  `show_ignored`, a rescan that changes an unloaded directory's ignored state before that listing
  runs leaves its children's ignored flags as they were, and the result is accepted as fresh with
  nothing owed. A fix would carry the flag in the result and have `fill` discard it, owing a
  refresh, when it no longer matches the node.
- **F5:** an overlapping child listing rejected after a newer stored listing, and the root-recovery
  marker on the root itself, can let an older parent snapshot drop a restored child until the next
  event or rescan for it. The child's refresh is owed, but neither marker is passed up to the
  directories above it.
- **F6:** a batch with both `Path(root)` and `Rescan` after an ancestor of the project is deleted keeps
  the deleted rows. The `Rescan` branch runs without the root-path signal, so the root's listing
  counts as cut off above rather than gone.
- **F7:** the ignore matcher consults `.gitignore` and `info/exclude` above the nearest repository.
  `ignore::WalkBuilder` 0.4.33 does the same with the spec's `require_git(false)`; this was checked
  against the walker, which hid `keep.txt` in a nested repository under an ancestor's `*.txt`.
- **F8:** a linked worktree's `.git` gitfile isn't followed, so the common repository's `info/exclude`
  isn't loaded. The walker, configured as the spec has it, doesn't load it either: it resolves
  gitfiles only with `require_git(true)`.
