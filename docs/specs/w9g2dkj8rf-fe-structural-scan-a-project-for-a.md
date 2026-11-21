---
task_uid: "01m7wj2xef0yz1gpw9g2dkj8rf"
task: "T712"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`scan_finds_matches_across_files_of_the_chosen_language_only` passes: .rs and .py files both contain the text, a Rust pattern gives only the .rs ones"
    required: true
    manual: false
  - id: "C2"
    text: "`scan_respects_gitignore_and_path_filter` passes"
    required: true
    manual: false
  - id: "C3"
    text: "`prefilter_skips_parsing_files_without_the_patterns_literals` passes: counts parses through a test hook (`ScanSummary::files_parsed`)"
    required: true
    manual: false
  - id: "C4"
    text: "`pattern_of_only_metavariables_has_no_prefilter_and_reports_it` passes"
    required: true
    manual: false
  - id: "C5"
    text: "`scan_streams_batches_to_the_sink_per_file_and_stops_on_cancel` and `scan_stops_at_max_matches_and_sets_truncated` pass"
    required: true
    manual: false
  - id: "C6"
    text: "`scan_skips_files_over_the_size_cap_and_unparsable_files_and_reports_them_in_the_summary` passes"
    required: true
    manual: false
  - id: "C7"
    text: "`scan_runs_a_wasm_grammar_on_worker_threads` passes (scan_wasm.rs, the json fixture registered; pattern `{\"a\": $V}`)"
    required: true
    manual: false
  - id: "C8"
    text: "`scan_with_one_thread_is_in_path_order` passes"
    required: true
    manual: false
---

# fe-structural: scan a project for a pattern: language filter, literal prefilter, parallel parse, streamed batches, cancel, caps, per-file deadline (structural search SS7) (Mac T2020)

# fe-structural: scan a project for a pattern: language filter, literal prefilter, parallel parse, streamed batches, cancel, caps, per-file deadline (structural search SS7)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

`scan(root, &Pattern, rewrite: Option<&Rewrite>, &ScanOptions, filter: &PathFilter, cancel: Arc<AtomicBool>, sink: impl FnMut(Vec<FileResult>)) -> ScanSummary` over `fe_project::walk_files` (SS6; add fe-project as a dependency). `FileResult { path, rel, matches: Vec<MatchRow> }`, `MatchRow { range, start_line, start_col, first_line: String (truncated 512 bytes on a char boundary), line_count, captures: Vec<(String, String)>, rewritten: Option<String>, flagged: bool }`. Per file: `language_for_path(path) == pattern.language()` (compare by name) else skip without reading; size cap (default 2 MiB, `ScanOptions::max_file_bytes`); read bytes, require valid UTF-8 (skip and count otherwise; UTF-16 files are skipped in v1 and counted); prefilter in `prefilter.rs`: every `Pattern::literals()` string must occur in the bytes (memchr::memmem, add if in lock, else std find on windows); then `fe_syntax::parse_str` with a per-file deadline (ScanOptions, default 2 s) and the cancel flag, find_all, build rows. Threads: `ScanOptions::threads` default min(available, 8), 4 for a wasm language (`Language` exposes whether wasm? if not, decide by a `fe_syntax::wasm_stats` check or a plain option the caller sets; do not edit fe-syntax in this card). `max_matches` default 10_000 -> `truncated`. Summary counts: files_walked, files_of_language, files_prefiltered_out, files_parsed, parse_failed, too_large, non_utf8, matches, cancelled, truncated. Tests in the three new files named; generate fixtures in tempdirs; keep sizes small (no soak).

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

## Implementation notes

See docs/decisions/w9g2dkj8rf-fe-structural-scan-a-project-for-a.md. In short:
language by `language_name_for_path`; the prefilter requires named leaves' text
only (`prefilter_literals`), since anonymous tokens match by kind; WebAssembly
is a caller-set `ScanOptions::wasm`; deadline overruns are counted in
`ScanSummary::timed_out`, apart from `parse_failed`.
