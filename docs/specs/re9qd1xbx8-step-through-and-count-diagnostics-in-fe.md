---
task_uid: "01m4rjfjg44ahye2re9qd1xbx8"
task: "T52"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "diagnostics_nav_tests::next_previous: next_diagnostic forward and backward skip a diagnostic at from, wrap once around the buffer, prefer the most severe at one start, and are None without diagnostics"
    required: true
    manual: false
  - id: "C2"
    text: "diagnostics_nav_tests::counts: diagnostic_counts counts each severity, a diagnostic without severity as an error"
    required: true
    manual: false
  - id: "C3"
    text: "diagnostics_nav_tests::grouped: group_problems sorts files by path, drops empty files, and orders a file's problems by row, column, then severity"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-editor and cargo clippy -p fe-editor --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Step through and count diagnostics in fe-editor: next and previous diagnostic with wrap, per-severity counts and problems grouped by file (Mac T514)

Filed on argon from Mac T514 (card ~/coordination/argon-cards/T514.json rev 1). Mac task uid 01m4rheyqd5p721r4txp86wsc1. Title the host commit "... (Mac T514)".

## Problem

Diagnostics show in the editor only; there's no way to step through them, nor a list of them (day-one audit
2026-10-12: "only config_problems; T394 abandoned"). The Problems panel and `]d`/`[d` need headless pieces first.

## Change

In `crates/fe-editor/src/diagnostics.rs` (public module; lib.rs untouched):

- `impl EditorModel` there: `next_diagnostic(&self, from: usize, forward: bool) -> Option<Range<usize>>`: the
  first diagnostic starting strictly after `from` (forward) or strictly before it (backward), wrapping around the
  buffer once, as nvim's `vim.diagnostic.jump` with `wrap = true`; ties at one start take the most severe.
- `DiagnosticCounts { errors, warnings, infos, hints }` and `EditorModel::diagnostic_counts(&self)`; a diagnostic
  with no severity counts as an error, as fe-editor already treats it.
- `group_problems(files: Vec<(PathBuf, Vec<Problem>)>) -> Vec<FileProblems>`: one group per file sorted by path,
  files without problems dropped; within a file, by row, then column, then severity. `Problem { range, row, column,
  severity, message, source }`, built from a model's `diagnostics()` by `EditorModel::problems(&self)`.

## Tests

New file `crates/fe-editor/src/diagnostics_nav_tests.rs`, declared from diagnostics.rs with `#[path]`.
