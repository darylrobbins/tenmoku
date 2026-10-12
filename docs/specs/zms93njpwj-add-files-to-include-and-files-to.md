---
task_uid: "01m4pzqw2k9zfbg7zms93njpwj"
task: "T25"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test search_panel::filter_tests::include_narrows: typing *.rs into files to include reruns the search and lists only .rs files"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test search_panel::filter_tests::exclude_lists: \"docs/, *.md\" in files to exclude leaves out both docs/ and every .md file"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test search_panel::filter_tests::invalid_glob: a[ in either field shows Invalid glob a[: in the panel's error line with no results, and correcting it reruns the search"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add files-to-include and files-to-exclude fields to the project search panel (Mac T460)

Filed on helium from Mac T460 (~/coordination/helium-cards/T460.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-i.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The previous card adds include and exclude path filters to fe-project's search (`PathFilter`, `search_filtered`),
but the project search panel (`crates/fe-workbench/src/search_panel.rs`, M3 Browse, plan line 108) has only the
query and its case, whole-word and regex toggles, so the user can't use them.

## Change

In `crates/fe-workbench/src/search_panel.rs` only:

- Two single-line text inputs under the query row, always shown, with the placeholders `files to include` and
  `files to exclude`. Each holds a comma-separated list of globs: split on commas, trimmed, empties dropped.
- The search runs through `fe_project::search_filtered` with `PathFilter::new(root, include, exclude)`. Editing
  either field reruns the search exactly as editing the query does (same debounce and cancellation of the running
  search).
- A `GlobError` shows in the panel's error line the way an invalid regex does (`Invalid glob <glob>: <error>`), with
  no results, and clears when the field is fixed.
- The fields keep their text while the panel is hidden and shown again in the window; they aren't persisted.

## Tests

New file `crates/fe-workbench/src/search_panel_filter_tests.rs`, declared from search_panel.rs with `#[path]`, using
the existing search panel test helpers (one search thread, as `tests/search_panel.rs` does).
