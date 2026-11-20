---
task_uid: "01m7wj2r9465c4f2sx1sk9j9cx"
task: "T707"
rev: 2
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "`walk_files_visits_the_same_files_as_search_does_for_a_match_all_query` passes: compare walk_files paths with search results over a tempdir with .gitignore, hidden and binary files"
    required: true
    manual: false
  - id: "C2"
    text: "`walk_files_honours_the_path_filter_and_prunes_excluded_directories` passes"
    required: true
    manual: false
  - id: "C3"
    text: "`walk_files_stops_when_cancelled_and_reports_cancelled` passes"
    required: true
    manual: false
  - id: "C4"
    text: "`walk_files_opens_beneath_the_root_and_skips_a_symlink_swapped_in` passes (reuse the approach of the existing search_only_parent.rs; Unix only)"
    required: true
    manual: false
  - id: "C5"
    text: "the existing search tests in crates/fe-project/tests/replace.rs, search_filter_relative_root.rs, search_only_parent.rs and src/search_filter_tests.rs still pass unchanged"
    required: true
    manual: false
---

# fe-project: a reusable file walker extracted from the search, with the same gitignore, filter, cancel and beneath-open rules, so other scans share it (structural search SS6) (Mac T2019)

# fe-project: a reusable file walker extracted from the search, with the same gitignore, filter, cancel and beneath-open rules, so other scans share it (structural search SS6)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

Read `search_inner` and its `Shared` in crates/fe-project/src/search.rs (the walker: threads, PathFilter, `open_file_under`, regular files only, cancel flag). Extract the walking part into new `crates/fe-project/src/search/walk.rs` as `pub fn walk_files(root: &Path, threads: usize, filter: &PathFilter, cancel: Arc<AtomicBool>, visit: impl Fn(WalkedFile) -> WalkControl + Sync) -> WalkSummary` where `WalkedFile { path, rel, file: File }` (already opened beneath the root, read-only), and make `search_inner` call it WITHOUT changing any observable behaviour (same order with threads == 1, same counters). One mod line plus re-exports in lib.rs. Behaviour-preserving: if an existing test must change, stop and say so. Test in new tests/search_walk.rs.

## Helium filing note

- Lent from the Mac as y153, the user's request (structural search SS1-SS9 on a user-approved design: one
  language per search, injected languages skipped, `inside` the only relational rule; user docs help 1-2).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Windows: imports and helpers that only `#[cfg(unix)]` tests use go under `#[cfg(unix)]` too, or Windows Clippy
  fails with -D warnings (Mac T2028). Linux gates can't catch this.
- Golden and pin tests: a new command changes typed_command_ids_tests.rs and command_catalog_tests.rs; a new
  setting changes the settings schema/template goldens, settings_model.rs and the l10n files; a new binding can
  break binding-count tests in fe-gpui. If one fails that isn't in scope, widen it with `rgt task respec --scope`
  (it replaces the list, so pass the whole list) before you finish, and say why in the spec's Implementation notes.
- Criteria tests need full cargo test paths (`module::tests::name`, or `fe-x::binary name` for an integration
  test whose name exists in several binaries).
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as
  written. Run rgt finish once, and wait for its result before you report.
