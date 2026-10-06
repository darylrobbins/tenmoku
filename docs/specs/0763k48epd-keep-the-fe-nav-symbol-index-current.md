---
task_uid: "01m48tt97rgakxcp0763k48epd"
task: "T104"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project test tests::finder_delta::apply_reports_present_and_removed: on a complete index, a created file is in present; a modified file is in present; a deleted file is in removed; a deleted directory puts every file under it in removed; a created directory with files puts each in present; a file created in a .gitignored directory is in neither; paths are relative, sorted and deduplicated"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test tests::finder_delta::deferred_and_rescan_deltas: a batch applied before finish gives an empty delta and finish returns its delta; a Rescan batch sets rescanned with removed equal to the paths gone and present equal to every path after the re-walk; the existing tests::finder suite passes unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-nav test tests/watch.rs::changed_files_are_reextracted: after build, appending fn added() to a.rs and applying [Path(a.rs)] gives Reindex.extract == [a.rs]; after extract and extend, definitions(\"added\") finds it and a fn deleted in the same edit is gone"
    required: true
    manual: false
  - id: "C4"
    text: "fe-nav test tests/watch.rs::removed_files_and_dirs_leave_the_index: deleting b.rs and applying its Path change removes its symbols inside plan with an empty extract list; renaming dir src/m to src/n with Path changes for both moves every symbol to src/n paths and none remain under src/m"
    required: true
    manual: false
  - id: "C5"
    text: "fe-nav test tests/watch.rs::unchanged_and_ignored_files_are_not_reextracted: a Path change for a file with the same len and modified time gives an empty extract list; a .rs file created in a .gitignored dir is not extracted; a file that becomes unreadable or binary has its symbols removed after extraction"
    required: true
    manual: false
  - id: "C6"
    text: "fe-nav test tests/watch.rs::rescan_reextracts_only_what_changed: after editing one of 50 files and deleting another, plan([Rescan]) extracts exactly the edited file and removes the deleted one"
    required: true
    manual: false
  - id: "C7"
    text: "fe-nav test tests/watch.rs::changes_during_the_build_come_from_finish: a batch planned between the first extend and finish gives an empty extract list, and finish returns a Reindex that includes the changed file"
    required: true
    manual: false
  - id: "C8"
    text: "scripts/check-no-gpui.sh prints ok: fe-project and ok: fe-nav; no Cargo.toml or Cargo.lock change; crates/fe-project/src/finder.rs walk and ignore logic unchanged apart from recording the delta"
    required: true
    manual: false
---

# Keep the fe-nav symbol index current from project watcher batches

# Keep the fe-nav symbol index current from project watcher batches

Milestone M4 "Navigate". T103 builds a `SymbolIndex` once. Files change while the editor runs (saves,
branch switches, generated code), and a stale index sends go-to-def to a definition that moved. This
card applies `fe_project::ProjectWatcher` batches to the index, re-extracting only files that
changed. It is GPUI-free; the workbench wiring is the UI card.

## Problem

A batch of `FsChange`s says only that something at a path may have changed, or `Rescan`. Turning it
into "these files appeared or changed, these went away" with the tree's ignore rules, new
directories, removed directories, case renames and rescans is exactly what T62's
`FinderIndex::apply` already does, carefully. The symbol index must not re-implement it, and it must
not hold its lock while parsing.

## Change

### `crates/fe-project/src/finder.rs` and `lib.rs`: report what a batch changed

- `FinderIndex::apply` and `FinderIndex::finish` return a `FileSetDelta { present: Vec<PathBuf>,
  removed: Vec<PathBuf>, rescanned: bool }` (relative paths, sorted, deduplicated), recorded as they
  insert and remove paths. The return value is ignorable, so existing callers compile unchanged.
  - `present`: paths inserted, or re-inserted because the batch named them or a directory above
    them (a modified file is re-inserted, so it is here).
  - `removed`: paths that were in the index before and aren't after.
  - A batch applied while building is deferred (as today) and gives an empty delta; `finish` returns
    the delta of the deferred changes.
  - A `Rescan` sets `rescanned`, with `removed` = paths gone after the re-walk and `present` =
    every path after it.
- `lib.rs`: re-export `FileSetDelta`. Keep this to a small hunk; no change to the walk or to the
  ignore logic.

### `crates/fe-nav/src/index.rs`: plan, extract off the lock, extend

- `SymbolIndex` keeps a `FinderIndex` as its file set. `extend` inserts each file's rel into it, and
  T103's `build` sink path feeds it as today.
- `SymbolIndex::plan(&mut self, changes: &[FsChange]) -> Reindex`:
  - applies the batch to the file set;
  - removes the symbols of every `removed` path at once;
  - returns `Reindex { extract: Vec<PathBuf> }`: the `present` paths whose language has symbols and
    whose `(len, modified)` differ from `file_state`, or that aren't indexed yet.
- `SymbolIndex::finish` returns the `Reindex` for changes deferred during the build.
- The caller runs `fe_nav::extract` (or a helper `extract_all(root, &paths, threads, cancel)` in
  `build.rs` if T103's workers are easy to reuse) **without** holding the index lock, then `extend`s
  the results; a path whose extraction now fails with `Unsupported`, `TooLarge`, `Binary` or
  `Unreadable` has its old symbols removed.
- A path changed again while being extracted is caught by its next batch: re-extraction compares
  `(len, modified)` again.

### Tests (new files)

- `crates/fe-project/src/tests/finder_delta.rs`, declared in `crates/fe-project/src/tests/mod.rs`.
- `crates/fe-nav/tests/watch.rs`, an integration test. It writes files into a `tempfile` dir and
  hands the index synthesised `FsChange` batches, as the watcher would. No `ProjectWatcher`
  thread, so nothing waits on FSEvents timing (see T99's flakiness).

## Out of scope

- The workbench fan-out of watcher batches to this index (the UI card).
- Re-reading unsaved buffers; the index reflects disk only.
- Ignore-file edits beyond what `FinderIndex::apply` already handles (its documented limits hold).

## Decided (lead defaults; the user may revisit)

- **Reuse `FinderIndex` as the file set** rather than re-deriving ignore rules in `fe-nav`. This
  keeps a second copy of the project's paths in memory beside quick-open's (about 10 MB at 100k
  files); sharing one `FinderIndex` is a later optimisation if the perf card shows it matters.
- Unchanged files are detected by `(len, modified)`, not by hashing contents.

## Implementation notes

- Outside the listed scope, two small additions the API needs: `crates/fe-nav/src/lib.rs`
  re-exports `Reindex`, `Extracted` and `extract_all` (the integration test names them), and
  `FinderIndex::contains(rel)` is a new accessor in `finder.rs` (no walk or ignore change).
- Putting results in is `SymbolIndex::update(Extracted)`: `extract_all` returns the extracted
  files and the failures with their `Skipped` reason. Any failure drops the file's old symbols
  (`ParseFailed` too: the index reflects only what was read). A result for a path the file set no
  longer holds (a batch planned since removed it) is dropped, not put back.
- While building, `FinderIndex::apply` still applies a batch and keeps it for `finish`; what it
  removed then (say, a file the build had already handed over) is recorded and reported by
  `finish`, since re-applying the batch there finds nothing left to remove.
