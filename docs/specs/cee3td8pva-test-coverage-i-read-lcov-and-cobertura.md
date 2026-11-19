---
task_uid: "01m7th4gj1qzrrnwcee3td8pva"
task: "T700"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "coverage_tests::lcov_lines_branches_and_duplicate_sf: DA gives covered and uncovered, a BRDA with a missed branch on a covered line is partial, repeated SF records merge, CRLF and unknown records are tolerated."
    required: true
    manual: false
  - id: "C2"
    text: "coverage_tests::cobertura_sources_and_condition_coverage: relative filenames resolve through \u003csources\u003e then the root, branch=\"true\" with condition-coverage under 100% and hits \u003e 0 is partial."
    required: true
    manual: false
  - id: "C3"
    text: "coverage_tests::unsupported_and_hostile_input: JaCoCo XML or gcov text gives `Unsupported coverage report`, a DOCTYPE or entity is refused, and the file/line/size caps fail the load without allocating past them."
    required: true
    manual: false
  - id: "C4"
    text: "coverage_tests::paths_resolve_under_symlinked_roots: SF and filename paths relative, absolute and under a symlinked project root all match the canonical path of the open file."
    required: true
    manual: false
  - id: "C5"
    text: "coverage_tests::discovery_order_and_setting: coverage.report (path or glob) wins; otherwise the first folder in the documented list that has a report, newest by mtime; a lone .coverage gives the `Run coverage xml or coverage lcov` message; the tree is never walked."
    required: true
    manual: false
  - id: "C6"
    text: "coverage_tests::reload_on_change_keeps_old_report_on_bad_parse: a changed mtime/len reloads on a background thread, a failed parse keeps the previous report and the error, and a report older than a file's mtime reports stale for that file."
    required: true
    manual: false
---

# Test coverage I: read lcov and Cobertura reports into a per-file line model, find them, reload when the file changes (Mac T1982)

# Test coverage I: read lcov and Cobertura reports into a per-file line model, find them, reload on change (headless)

Phase 1 of 2 of "Test coverage overlay in the gutter". No painting here: this builds the data model that cov2 draws and toggles. Pure Rust, no GPUI beyond the file watcher already in the workbench.

## Formats (standard reports only)
- **lcov** (`.info`, `lcov.info`): `cargo llvm-cov --lcov`, c8 / istanbul / nyc (`lcov.info`), `coverage lcov` (coverage.py). Records: `SF:<path>`, `DA:<line>,<hits>[,<checksum>]`, `BRDA:<line>,<block>,<branch>,<taken|->`, `LF/LH`, `end_of_record`. A line with `DA` hits > 0 is covered, 0 uncovered; a line with `BRDA` records where some branches are taken and some are `0` or `-` is **partial** (a covered line with a missed branch). Lines with no `DA` are not instrumented and have no mark. Tolerate `TN:`, `FN:`, `FNDA:`, unknown records, CRLF, and an `SF:` repeated for one file (merge: hits add, a line covered anywhere is covered).
- **Cobertura XML** (`coverage.xml`: coverage.py `coverage xml`, istanbul `cobertura`, `cargo llvm-cov --cobertura`, JaCoCo-to-Cobertura converters): `<class filename=..>` with `<lines><line number= hits= branch="true" condition-coverage="50% (1/2)"/>`; a `branch="true"` line with condition-coverage below 100% and hits > 0 is partial. `<sources><source>` prefixes make relative `filename`s absolute; try each source then the project root. No XML crate is a workspace dependency today (checked), so write a small hand scanner over the few elements needed and do NOT add a dependency; it forbids DOCTYPE/entities (no XXE, no billion laughs) and capped at 64 MiB.
- Detect the format by content (`<?xml` / `<coverage` versus `TN:`/`SF:`), not by extension.
- Reject, do not guess, other formats (JaCoCo XML, gcov, Clover): a clear `Unsupported coverage report` error naming the first line seen.

## Model (`crates/fe-workbench/src/coverage.rs`, pure)
- `CoverageReport { files: HashMap<PathBuf, FileCoverage> }`, `FileCoverage { lines: BTreeMap<u32, LineCover> }`, `LineCover::{Covered, Uncovered, Partial}`, 1-based file lines, with `fn mark(&self, path, line)`. Paths are made absolute against the report's folder and the project root; `SF:` paths relative to the project root, absolute, and Windows-drive paths all resolve; a path outside the project root is kept (the file may be open from elsewhere) but never read from disk. Match open files by canonicalised path (symlinks, macOS `/private`), case-insensitively on Windows and macOS only where the filesystem is.
- Caps (constants, tested): at most 200,000 files and 20,000,000 line records per report; past that the load fails with a message, it does not allocate without bound.

## Discovery
`find_report(root, setting: Option<&Path>) -> Option<PathBuf>`: the setting `coverage.report` (project or user; a relative path is against the root; a glob is allowed) wins; otherwise the newest by mtime among, in order of search (stop at the first folder that has any): `<root>/lcov.info`, `<root>/coverage/lcov.info`, `<root>/target/llvm-cov/lcov.info`, `<root>/target/llvm-cov-target/lcov.info`, `<root>/target/coverage/lcov.info`, `<root>/coverage.xml`, `<root>/coverage/cobertura-coverage.xml`, `<root>/coverage/coverage.xml`, `<root>/htmlcov/..` is not a report (skip), `<root>/.coverage` is a SQLite file and is not supported (say so in the failure message when it is the only thing found and `coverage.xml` is absent: `Run coverage xml or coverage lcov`). The search never walks the tree. A `.gitignore`d report is still found. Nothing outside the root is read.

## Watch and reload
`CoverageSource` holds the current report and its file's `(mtime, len)`; `poll()` is cheap (one stat) and reloads when they change, parsing on a background thread, and keeps the previous report on a failed parse (with the error kept for the UI). Reuse the workbench's file-watch seam the way `disk_sync.rs` does (see how it avoids polling when the FS watcher is live); a test drives a `ManualClock`-style fake like the other watchers' tests. A report newer than the file's last save is not "stale"; one OLDER than the file's mtime is reported as `stale: true` for the file (cov2 decides what to show).

## Tests (new files)
`crates/fe-workbench/src/coverage_tests.rs`: lcov with branches and a merged duplicate `SF`; Cobertura with `sources`; CRLF; unsupported format; DOCTYPE refusal; caps; relative-path resolution under a symlinked root; discovery order and the `.coverage`-only message; reload-on-change keeps the old report on a bad parse. Fixture reports are small inline strings, not files.

## Linux
Yes: no platform code. Path tests that need Windows drive letters use string-level helpers, not the filesystem.

## Not here
Painting, the toggle, the setting's UI row, theme tokens, running tests with coverage (cov2 and a later card).

## Helium filing note

- Lent from the Mac as y151, the user's request (notebook cells NB1-NB3, test coverage COV1-COV3, reading mode).
- **Never touch crates/fe-workbench/src/lsp_consent*.rs** (Mac T1978 and T1701 own it).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Golden and pin tests: a new setting changes the settings schema/template goldens, settings_model.rs and the
  l10n files; a new command changes typed_command_ids_tests.rs and command_catalog_tests.rs; a new binding can
  break binding-count tests in fe-gpui. The lead pre-added the ones it knew of. If another golden or pin
  fails, widen your scope with `rgt task respec --scope` (it replaces the list, so pass the whole list) before
  you finish, and say why in the spec's Implementation notes.
- Workbench-wide settings reach live state only through workbench_root.rs reload_settings; per-editor fields
  ride EditorView::set_wrap_settings/apply_settings. If you need a file that's out of scope for that, respec first.
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as written.

## Implementation notes

- Scope widened to `crates/fe-workbench/l10n/en-US/coverage.ftl` and
  `crates/fe-workbench/src/l10n.rs`: fe-workbench's literal lint checks
  every file, so the load and discovery errors are catalogue messages
  (`workbench-coverage-*`), shown through `CoverageError`'s `Display`.
- `find_report` is as specified; `discover` is the same search returning the
  reason when there's no report (the `.coverage`-only message, a
  `coverage.report` matching nothing or outside the root).
- `CoverageSource` takes the workbench watcher's `FsChange` batches
  (`fs_changed`, with `set_watching(true)` it stops polling); wiring it to
  the workbench's watcher is card two's, as `workbench.rs` is out of scope.
- The remaining calls (search order by folder, DOCTYPE handling, merging,
  path resolution order) are in
  `docs/decisions/cee3td8pva-test-coverage-i-read-lcov-and-cobertura.md`.
- Review round 1: F1, F2, F4, F5 and F6 fixed (see the decisions doc);
  `reload_follows_setting_changes_and_moved_folders` covers F1 and F4. F3
  (define the `coverage.report` setting) disputed on the lead's decision:
  T702 (Mac T1984, COV3) owns the setting, with fe-config's settings files,
  goldens and l10n in its scope; this card takes it as an argument.
- Review round 2: F2 (an `SF:` inside an open record, empty `BRDA` fields)
  and F7 (watcher events through symlinked report folders) fixed, tested in
  `lcov_lines_branches_and_duplicate_sf`,
  `reload_on_change_keeps_old_report_on_bad_parse` and
  `reload_follows_setting_changes_and_moved_folders`.
