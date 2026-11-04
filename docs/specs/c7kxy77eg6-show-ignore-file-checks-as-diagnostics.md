---
task_uid: "01m6j5s58k8xpz66c7kxy77eg6"
task: "T437"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "ignore_diagnostics::ignore_diagnostics_tests::unused_pattern_is_dimmed_and_listed: an unused pattern in .gitignore gets an Unnecessary-tagged hint and a Problems row; a used one gets neither"
    required: true
    manual: false
  - id: "C2"
    text: "ignore_diagnostics::ignore_diagnostics_tests::duplicate_shadowed_and_invalid_have_severities: duplicate and shadowed are warnings, invalid is an error, all with source 'ignore'"
    required: true
    manual: false
  - id: "C3"
    text: "ignore_diagnostics::ignore_diagnostics_tests::save_and_tree_change_rerun_the_check: creating a file the pattern matches, then saving, clears its hint; the check runs on the background executor and a stale result is dropped"
    required: true
    manual: false
  - id: "C4"
    text: "ignore_diagnostics::ignore_diagnostics_tests::dockerignore_and_other_languages_get_none: .dockerignore and a Rust buffer get no ignore diagnostics, and closing the file clears them"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; existing diagnostics and lsp tests pass unchanged"
    required: true
    manual: false
---

# Show ignore-file checks as diagnostics: dimmed unused patterns, warnings, errors and Problems rows, off the UI thread (Mac T1365)

## Problem
Show the ignore_check findings (previous card) in ignore files like language-server diagnostics: dimmed/hinted unused patterns, warnings, errors, inline diagnostics (T1317) and Problems panel rows.

## Seams found
- Diagnostics: fe_editor::EditorModel::apply_diagnostics(server, identity, version, items, active) (crates/fe-editor/src/lib.rs); a buffer holds one DiagnosticSet keyed by one type pair, which LSP uses with (ServerId, (session, incarnation)) in crates/fe-workbench/src/lsp.rs. An ignore file has no language server, so key it with its own pair (IgnoreSource, u64 scan generation); if T1355 (DiagnosticOwner) lands first, switch to its Plugin/other variant. Diagnostic { severity, message, source: Some("ignore"), tags: [Unnecessary] for unused (dims the text), ... } is in crates/fe-editor/src/diagnostics.rs. Problems panel reads EditorModel::problems().
- Triggers: after open, on save, and on file-tree change (Workbench::fs_changed handlers in crates/fe-workbench/src/nav.rs / disk_sync.rs show the FsChange pattern); debounce ~500 ms, run check_ignore_file on the background executor with a cancel flag, drop stale results by generation. Never block the UI thread.

## Change
New crates/fe-workbench/src/ignore_diagnostics.rs (+ ignore_diagnostics_tests.rs, 'mod' lines in crates/fe-workbench/src/lib.rs, hooked from the open, save and fs_changed paths in workbench.rs and nav.rs, no change to lsp.rs). Only for editors whose language is gitignore and whose file name is not .dockerignore. Clearing: closing, renaming away from an ignore name, or a rescan that finds nothing.
Tests: fe-workbench visual tests as in refactor_preview_view_tests.rs: unused pattern gets an Unnecessary hint, duplicate and shadowed show warnings, an invalid one an error, all listed in the Problems panel; adding a matching file on disk and re-running removes the hint; a big tree (generated in a temp dir, 5k files) leaves the UI thread free (result arrives via polling, not blocking). Headless; Mac or Linux.

## Implementation notes

- `problems_panel.rs` joined the scope in revision 2, for review F1 (round 1). `sync_problems` listed only the first editor of each path, so with a language picked by hand in one split the file's row disappeared while another split still showed its findings. It now lists the first editor of the path that has any problems, once per path.
- A language server's diagnostics for an ignore file win. A model's diagnostics are keyed by one pair of types, so `ignore_diagnostics.rs` doesn't apply over a set of another owner's that has entries, and shows the check's again once that set is empty or reset.
