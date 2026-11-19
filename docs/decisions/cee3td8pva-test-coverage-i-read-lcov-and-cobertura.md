# Test coverage I: design calls (T700)

The model is `crates/fe-workbench/src/coverage.rs`; its module docs are the
reference. These are the calls the spec left to the worker, for card two
(painting and the toggle) to build on.

- **The API.** `parse_report(reader, report_dir, root, &Limits)` and
  `load_report(path, root)` give a `CoverageReport`, whose `file(path)`,
  `mark(path, line)` and `file_state(path, file_modified)` take a canonical
  path. `find_report(root, setting) -> Option<PathBuf>` is the spec's;
  `discover` is the same search returning `Result<PathBuf, CoverageError>`,
  so the UI can say *why* there's no report (the `.coverage`-only message,
  a setting matching nothing). `CoverageSource` follows the report:
  `poll(now)`, `fs_changed(&[FsChange])`, `set_watching(live)`,
  `set_setting(..)`, then `report()` and `error()`.
- **Errors are localized.** fe-workbench's literal lint checks every file,
  so `CoverageError`'s `Display` goes through `tr!` with keys in a new
  `l10n/en-US/coverage.ftl` (the scope was widened to it and to
  `src/l10n.rs`, which lists it). Card two shows `error.to_string()`.
- **Search order, by folder.** The spec's list interleaves folders
  (`lcov.info` at the root comes first and `coverage.xml` at the root sixth)
  and says to stop at the first folder that has any report and take the
  newest. The search goes folder by folder in order of first appearance:
  the root (`lcov.info`, `coverage.xml`), `coverage/` (`lcov.info`,
  `cobertura-coverage.xml`, `coverage.xml`), `target/llvm-cov/`,
  `target/llvm-cov-target/`, `target/coverage/`. In a folder the newest by
  mtime wins, and on a tie the one earlier in the list.
- **`coverage.report` that matches nothing is an error**, not a fallback
  to the search: a setting the user wrote should never be silently passed
  over. One outside the project root is refused, by its normalised path
  and by the canonical root, so `..` and absolute paths elsewhere can't
  read outside. A glob's wildcards (`*`, `?`, `[..]`) match within one
  path component each; `**` is taken as `*`, since recursing would walk the
  tree. Only folders the pattern names are listed, every entry of each
  looked at; more than 10,000 matches fail the search (review round 1, F5)
  rather than choose from some.
  A report reached through a symlink out of the project is skipped, by
  discovery and by the setting alike.
- **Relative paths, in order.** lcov: the root, then the report's folder
  (cargo llvm-cov, c8 and coverage.py write paths relative to where they
  ran, the root). Cobertura: each `<source>` (itself made absolute against
  the root), then the root, then the report's folder. The first candidate
  that exists inside the project wins; when none does, the first. A
  candidate outside the project is never stat'ed.
- **Canonical paths, cheaply.** Inside the project each file's folder is
  canonicalised once (cached), and a file that is itself a symlink is
  canonicalised whole, so a report of 200,000 files costs a stat each, on
  the loading thread. Outside the project a path is kept as written, with
  `.` and `..` taken out lexically. Windows drive and UNC paths read on
  another system (`C:\x`, `C:/x`, `\\server\x`) are absolute: kept, never
  joined to a base.
- **Case folding.** Linux never folds. macOS asks, once per load, whether
  the root looked up in the other case is the same inode; Windows assumes
  NTFS's default. Where folding, lookups that miss by exact path try a
  lowercased index.
- **lcov is strict about what it reads** (review round 1, F2). Unknown
  records are skipped, but a `DA` or `BRDA` that doesn't parse or lacks a
  field (`BRDA`'s block and branch included), one outside a file's record,
  an empty `SF:`, an `SF:` opening a record before the last one ended, or a
  file's record never ended by `end_of_record` fails the load (round 2,
  F2): a report caught halfway through its
  write keeps the previous one. `DA:0,..`, a placeholder some tools write,
  is no error and has no mark.
- **Merging.** lcov branches are keyed by line and `(block, branch)`, so a
  branch taken in any record is taken; DA hits add, saturating. Cobertura
  has no branch identity: only a class's own `<lines>` are read, never a
  `<method>`'s copies, which can disagree on conditions (review round 1,
  F6); two classes sharing a file give a line that's partial only if
  neither reports full condition coverage. Condition coverage reads the `(met/all)` count, else the
  percentage. A line is uncovered at 0 hits, partial at > 0 hits with a
  missed branch, else covered; a `BRDA` on a line without `DA` gives no
  mark.
- **DOCTYPE.** Every DOCTYPE is refused, as the spec says, which also
  refuses old Java Cobertura reports that carry
  `<!DOCTYPE coverage SYSTEM "...coverage-04.dtd">`: a known limit. A
  DOCTYPE naming another root element (JaCoCo's `report`) reports the
  format as unsupported instead, since that's the more useful message.
  Clover is told from Cobertura by its `clover` attribute or a `<project>`
  under `<coverage>`.
- **Caps.** 200,000 files and 20,000,000 line records (`DA`, `BRDA`,
  `<line>`), counted before anything is stored. XML is read whole, so it's
  capped at 64 MiB by reading at most one byte more; lcov is streamed a
  line at a time, each line at most 1 MiB, so its only size bound is the
  record cap. `Limits` lets the tests use small caps with endless readers.
- **Watching.** `CoverageSource` can't reach the workbench's watcher here
  (that wiring is card two's, in `workbench.rs`), so it takes the same
  `FsChange` batches `disk_sync.rs` does. With `set_watching(true)` a poll
  does nothing until a batch names the report, a path in a place searched
  or a folder on the way to one (a folder of reports moved in whole), or
  is a rescan; then the search runs again and the report is stat'ed. The
  followed report's own change searches again too, since it can change
  which report is newest (review round 1, F4). Watchers name paths with
  symlinks resolved, so each searched folder (up to a glob's first
  wildcard) is also matched at its canonical path, and a file named as a
  report is (`lcov.info`, the glob's last part) anywhere in the project
  searches again, for folders linked from a glob's matches (round 2, F7). Without a live
  watcher it stats every 2 s and searches every 10 s. A changed `(mtime,
  len)` starts a load on its own thread; a failed load keeps the report and
  records the error, and the failing file isn't parsed again until it
  changes. A setting change drops a load still running, so the old
  setting's report never lands after it (review round 1, F1). A report file that vanishes takes its marks with it.
- **Staleness** is `report mtime < file mtime`, from the report file's
  metadata as it was opened; the caller passes the file's mtime (the
  editor's disk stamp). Equal times aren't stale.
- **`coverage.report` isn't defined here** (review round 1, F3, disputed
  by the lead's decision): T702 (COV3) adds it, with `coverage.show_on_open`,
  to fe-config's settings model, schema, template and catalogue, and hands
  its value to `CoverageSource::new` / `set_setting`.
