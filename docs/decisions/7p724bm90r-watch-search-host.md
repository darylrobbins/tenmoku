# WatchHost and SearchHost, phase 1b: choices (T724)

The decision is `docs/decisions/j8wvgszah0-remote-development.md` (D). This
card adds D § 5's `WatchHost` and `SearchHost` to fe-host, with fe-project's
watcher, search and finder as their local impls, and changes no caller.

## The traits in fe-host, the local impls in fe-project

As with `FsHost`'s tree operations (docs/decisions/5zajsn5anw-fs-host-tree.md),
the code stays in fe-project and only plain data moves:

- **Types move to fe-host**: `FsChange` (`watch.rs`; D § 5's wire type,
  unchanged), and `SearchQuery` (with `literal`), `SearchOptions`,
  `FileMatches`, `LineMatch` and `SearchSummary` (`search.rs`). fe-project
  re-exports each where it was, so no caller changes. `SearchQuery`'s
  private `matcher()` is now fe-project's free `search::matcher(query)`,
  since a crate can't add inherent methods to another's type.
- **The local impls** are `fe_project::LocalWatch` (`watcher.rs`) and
  `fe_project::LocalSearch` (`search.rs`), unit structs.
- **`HostSet` doesn't gain them yet.** Its local constructor,
  `fe_project::local_hosts()`, is in `fs_ops/host.rs`, outside this card's
  scope, and a new field would break its struct literal. Card 1a-wb, which
  builds the window's `HostSet` and switches the callers, adds
  `watch: Arc<dyn WatchHost>` and `search: Arc<dyn SearchHost>` there with
  `LocalWatch` and `LocalSearch`.

## Stream and Subscription

fe-host has no async runtime, and today's callers run the watcher, search
and finder on threads they own with a sink. So both are blocking channels,
read off the UI thread (`stream.rs`):

- **`Subscription<T>`** is a receiver and the source sending into it, held
  until the subscription is dropped. `LocalWatch::watch` is a
  `ProjectWatcher` whose sink sends each batch, unchanged, into an
  unbounded channel (a watcher's thread must never wait on its reader, as
  it never did); dropping the subscription drops the watcher, which stops
  and joins its thread as before. A watch that can't start fails
  `watch` with notify's error as an `io::Error` (its own, for an I/O error;
  `NotFound` for a missing path).
- **`Stream<T, E>`** is a bounded channel of batches and an end value.
  Dropping it sets the producer's cancel flag, which is the very flag the
  search and the finder's walk take, and wakes a producer blocked sending;
  its next `StreamSender::send` fails. `finish` reads the rest and returns
  the end, `cancel` drops and waits for it, and `ended()` is a handle that
  waits for the producer even after the stream is dropped (what the
  cancel-on-drop tests use, and what "a new query cancels the old" (D § 11)
  can wait on).
- **Batches, not items.** D § 5 abridges the search's stream to
  `Stream<FileMatches>`; each item here is one of today's batches
  (`Vec<FileMatches>`, at most 64 files), so a stream through the host
  hands over exactly what `search`'s sink got, in the same order, and a
  remote host can frame them as they are.
- **The bound** is two batches (`STREAMED`), as the finder's walk keeps two
  for a slow sink: a reader that stops holds the producer up rather than
  letting it run ahead unbounded (D § 2.4's credits will meet it there).

## SearchHost's shape

- **`search(root, query, options, filter)`**: D § 5's
  `search(query, options, filter)` with the root named, as `files` names
  it; today's three calls all take one.
- **`SearchFilter`** is fe-project's `PathFilter` as its globs (`include`,
  `exclude`), which the host compiles, plus `search_excluding`'s file
  (`excluded`). `PathFilter` holds compiled `ignore` matchers, which can't
  cross a wire and would make fe-host depend on `ignore` (see the T723
  file on why it mustn't). `LocalSearch::search` is fe-project's
  `search_inner`, the body all three calls share, with the compiled filter
  and the excluded file; with neither it is `search`, with globs
  `search_filtered`, with the file `search_excluding`.
- **The end value** is the summary, or `SearchFailed`: `InvalidPattern`
  (`SearchError`'s one variant), `InvalidGlob` (`GlobError`'s fields, now
  reported at the end of the stream, as a remote host would, rather than
  when the filter is built) and `Io` (the host couldn't run it: a thread
  that wouldn't spawn here, a lost link later). fe-host holds no UI text,
  so its `Display` is plain; `From<SearchError>` and `From<GlobError>`
  convert fe-project's, whose `Display` callers word them with.
- **`files(root, filter)`** is `FinderIndex::walk`, Quick Open's walk, in
  its own batches (1,024 paths); the walk takes no filter, so a filter's
  globs and excluded file drop paths from each batch, and a batch they
  empty isn't sent. The globs take the same files as the search walk's
  (`walk_files`), which prunes excluded directories instead: a file whose
  directory an exclude glob matches is matched through that parent. The
  end is `FilesSummary { complete }`, the walk's "ran to the end".
  `PathFilter::takes` asks each directory above a file on its own, as the
  search walk asks before entering it, so a negated glob for a file
  (`!dir/keep.txt`) can't bring it back from an excluded directory
  (`dir/`): review round 1, F1.

## Review round 1

- **F1** (medium): the file list checked only a file's own match, so
  `dir/` with `!dir/keep.txt` listed `dir/keep.txt`, which the search walk
  never reaches. Fixed as above; tested by
  `a_hosted_file_list_keeps_an_excluded_directorys_negated_files_out`.
- **F2** (low): the watch test only checked that each watcher reported the
  file. The kernel's timing coalesces two watchers' events differently, so
  `watcher_host_tests.rs` (a `#[path]` module of `watcher.rs`, added to the
  scope in rev 3) feeds a `ProjectWatcher` and a `LocalWatch` the same
  synthetic events through the crate's `SOURCE` hook and compares their
  batches exactly.

## The carried T707 finding

T707 F1 (`search/walk.rs`, parallel visitor admission after a cancel) is on
a file this card doesn't touch: `LocalSearch` calls the search as it is,
and `files` uses the finder's walk, not `walk_files`.

## Review round 2

- **F3** (low): C3 wants the comparison in the integration test itself.
  `a_hosted_watch_delivers_the_watchers_changes` now runs a `ProjectWatcher`
  and a `LocalWatch` on one fixture, makes a create, a rename and a
  removal, then writes one file last. Each watcher reports a change after
  the ones made before it, so the changes each delivered through that last
  file are the same set, whatever batches the two threads coalesced them
  into. The test asserts the sets are equal and hold every path touched.
