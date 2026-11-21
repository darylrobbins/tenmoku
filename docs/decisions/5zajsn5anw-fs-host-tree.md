# FsHost's tree operations, phase 1a (tree): choices (T723)

The decision is `docs/decisions/j8wvgszah0-remote-development.md` (D). This
card gives `FsHost` the operations fe-project's tree and its undo use
(D § 5's abridged `create`, `rename`, `remove`, `trash`, in their recorded
forms), with today's `fs_ops.rs` and `listing.rs` code as the local impl.

## The trait in fe-host, the local impl in fe-project

`fs_ops.rs` is ~3,300 lines of handle-relative, no-follow file code tied to
`listing.rs`'s handles and the `ignore` crate's rules. Moving it into
fe-host would move most of fe-project with it, so it stays where it is:

- **Types move to fe-host** (`crates/fe-host/src/tree.rs`), as plain data
  with no file handles, so a remote host can carry them: `EntryId`,
  `Placed`, `Created`, `EntryKind`, `Entry` (its `id` field is now `pub`,
  as a wire field must be), `ListingLost` (fe-project's `Lost`), and new
  wire types `ListRequest`, `ListReply` and `TrashRecord`. fe-project
  re-exports the moved ones where they were (`fe_project::fs_ops::EntryId`,
  `fe_project::EntryKind`, …), so no caller changes.
- **`FsHost` gains** `list_tree`, `create_recorded`, `rename`,
  `duplicate_recorded`, `copy_into_recorded`, `paste_name` (with
  `planned`: `paste_name_besides`), `move_into`, `trash_recorded`,
  `restores_from_trash`, `restore`, `in_trash`, `exists` and `truncate`
  (`open_truncated` without the handle). Each takes the root, as the free
  functions do, and keeps their beneath-root rules: the local impl calls
  them as they are.
- **The local impl is `fe_project::LocalTreeFs`** (`fs_ops/host.rs`): the
  first five methods are fe-host's `LocalFs`, the rest the free functions.
  `fe_project::local_hosts()` is a `HostSet` holding it.
- **`LocalFs` has no tree operations.** fe-host can't depend on fe-project,
  and `LocalFs` is a unit struct callers use by value, so the tree methods
  have default bodies that fail with `io::ErrorKind::Unsupported` (and
  `restores_from_trash` is `false`). `HostSet::local()` still holds
  `LocalFs`; card 1a-wb, which switches fe-workbench's callers, builds its
  `HostSet` with `local_hosts()` instead.

## Listing

`Listing::run` is unchanged in what it does: it now reads through
`Listing::read` (the old `list`, returning a `ListReply`) between the same
two ticks of the tree's clock. `Listing::run_on(host)` ticks the clock here
and has the host read: `ListRequest` is the listing less the clock (the
client's) and git's global excludes (the host's own: `LocalTreeFs` reads
them once, on its first listing, as a tree reads them once on opening).
`Listing::request` gives the request.

Review round 1 (codex wfb7xc) found the request taken on trust:

- **F1, F2.** A host now checks a request before it reads anything
  (`ListRequest::check`): `rel` must be plain names (no `..`, `.`, root or
  prefix, which could list outside the root), and `ids` one for the root
  and one for each name. Anything else fails with `InvalidInput`, a
  `fe_host::RequestError` inside. A tree never makes such a request.
- **F3.** Reading the global excludes once per host differed from a tree,
  which reads them once when it opens: a config change in between, or two
  trees on one host, could classify entries differently. So the request
  carries the tree's snapshot (`ListRequest::global_excludes`), and the
  local host lists with it. It's this machine's, so it never crosses the
  wire: a remote host lists with its own, and the local one reads its own,
  once, only for a request handed none.
- **The snapshot is opaque in fe-host** (`fe_host::Excludes`, an
  `Arc<dyn Any + Send + Sync>` the local host downcasts to fe-project's
  `Gitignore`; anything else is refused with `InvalidInput`). The first
  fix named `Arc<Gitignore>`, so fe-host depended on `ignore`. That put
  `aho-corasick` and `regex-automata` under every crate above fe-host,
  and their `PartialEq<Span> for Range<usize>` impls made
  `assert_eq!(ranges, [])` ambiguous in fe-plugin's `tests/decorations.rs`
  (E0282/E0283), failing the gate's vet and test stages. fe-host stays
  free of the dependency instead.

## Trash records

`fs_ops::Trashed` keeps its private fields and its methods
(`in_trash`, `path`, `location`, `id`) for today's callers. `TrashRecord`
is its wire form: path, place in the Trash, and `EntryId`; the
`(dev, ino)` that `Trashed` keeps beside its `EntryId` is the id's own, as
`trash_recorded` records it, so the conversions are lossless both ways.
`FsHost::restore` takes `&mut TrashRecord`, which follows the entry as
`fs_ops::restore`'s `&mut Trashed` does, on failure too.

## Carried T694 F1: full EntryId compares

With `EntryId` in fe-host, fe-project's test-only
`PartialEq<(u64, u64)> for EntryId` impls would be orphans. They are
removed, closing T694 F1: the tests that used them
(`fs_ops.rs` `undo_race_tests`, `tests/fs_ops_undo.rs`,
`tests/fs_ops_substitution.rs`, the last two added by respec) compare full
`EntryId`s from `fs_ops::entry_identity`, the helper the workbench uses on
Unix, birth time included.

## Not here

- fe-workbench's callers (card 1a-wb), and `entry_identity` on the trait:
  the workbench's Windows identity is kiln's, outside fe-project.
- The tree's own uses of the disk beyond listing (`FileTree::apply`'s
  lookups, its global excludes): `WatchHost` (phase 1b) and later.
- `tests/fs_host_tree.rs` is Unix-only, as `entry_identity` is.
