---
task_uid: "01m4pzqvwe8w2kpjgt41251w68"
task: "T24"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-project test search::filter_tests::include_narrows: in a fixture with src/a.rs, src/b.txt, docs/c.rs and a gitignored target/d.rs, include *.rs finds a.rs and c.rs only (not the gitignored d.rs), and include src/** finds a.rs and b.txt only"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test search::filter_tests::exclude_prunes_and_wins: exclude docs/ skips docs/c.rs and files_searched shows nothing under docs was read; a file matching both an include and an exclude glob is skipped"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project test search::filter_tests::errors_and_defaults: PathFilter::new with the glob a[ returns a GlobError naming it; search_filtered with PathFilter::default() returns the same matches as search, sequentially and in parallel"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-project, cargo test -p fe-nav and cargo clippy -p fe-project --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Filter project search by include and exclude globs in fe-project, pruning excluded directories and never re-including gitignored files (Mac T453)

Filed on helium from Mac T453 (~/coordination/helium-cards/T453.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-i.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

Project search (M3 Browse, plan line 108; `crates/fe-project/src/search.rs`) always searches every file the
listing walker yields (`listing::walk_builder`, used at :425 and in `search_parallel`). There is no way to search
only `*.rs`, only under `crates/fe-vim/`, or to leave out `tests/` or generated files, so a common query in a large
repo returns thousands of hits from the wrong places and `max_matches` (10,000, :236) truncates the ones wanted.
`SearchOptions` (:224) has only `max_matches` and `threads`. No card covers path filters (2026-10-11 scoping pass).

## Change

In `crates/fe-project/src/search.rs` (exports in `crates/fe-project/src/lib.rs`):

- `pub struct PathFilter` with `PathFilter::new(root: &Path, include: &[&str], exclude: &[&str]) ->
  Result<PathFilter, GlobError>` and `Default` (no filtering). `GlobError` names the glob and the parse error. Globs
  are gitignore-style and relative to the root, as ripgrep's `-g` writes them: `*.rs` matches at any depth,
  `crates/fe-vim/**` under that directory, a trailing `/` only directories. Build each list with
  `ignore::gitignore::GitignoreBuilder` (already a dependency); matching is case-sensitive.
- `pub fn search_filtered(root, query, options, filter: &PathFilter, cancel, sink)`, with `search` (and the
  `excluded`-file variant, :358) calling the shared inner function with an empty filter, so no existing caller or
  `SearchOptions` literal changes.
- Exclude: a file or directory matching any exclude glob (itself or through a parent) is skipped, and an excluded
  directory is not walked (pruned in the walker, `WalkBuilder::filter_entry`, in both the sequential and parallel
  walks). Include: when the list isn't empty, only files matching at least one include glob (itself or through a
  parent) are searched; directories are always walked unless excluded. Exclude wins over include.
- Filters only narrow: gitignored and hidden files stay skipped exactly as today. Do **not** use
  `ignore::overrides`, whose whitelist globs re-include gitignored files.

The search panel's fields are the next card.

## Tests

New file `crates/fe-project/src/search_filter_tests.rs`, declared from search.rs with `#[path]` (so
`src/tests/mod.rs` is untouched).
