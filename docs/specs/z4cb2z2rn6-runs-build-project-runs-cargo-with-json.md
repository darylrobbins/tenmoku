---
task_uid: "01m7n28sqbw9b76fz4cb2z2rn6"
task: "T676"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::build_runner (cargo nextest run -p fe-workbench, a fake cargo printing recorded JSON) pass: the_summary_follows_artifacts, compiler_messages_become_problems_and_replace_the_last_builds, a_root_without_cargo_toml_says_so, files_are_saved_first, an_untrusted_root_refuses"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench task_run passes (task tabs still feed Problems as before)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Runs: Build Project runs cargo with JSON messages as a Build Run and sends compiler messages to the Problems panel (Mac T1579)

# Build as a Run: Runs: Build Project with the crate timeline and problems

Spec: Runs: build; and Problems (a build's diagnostics are problems).

## Change

- `crates/fe-workbench/src/build_runner.rs` (new): `runs.build` (palette
  "Runs: Build Project") runs `cargo build --workspace
  --message-format=json-render-diagnostics` through the background runner
  (T1572) with `CargoParser` (T1578), as a Build Run. The Run's summary is
  `Build { crates, warnings, errors }`, updated as artifacts arrive; progress
  stays unknown (indeterminate). The raw rendered diagnostics are kept for the
  Output view.
- Compiler messages with a file and span become problems in the Problems panel
  as the run's source, replacing the last build's, the way a task tab's do
  (`crates/fe-workbench/src/task_run.rs`, `dock::problems_panel::SourceProblem`).
- Saves files first; only in a trusted root.
- Rust projects only (a `Cargo.toml` at or above the root); elsewhere the
  command says so.

## Not in this card

The waterfall (a later card), `cargo check`/`clippy` variants, a build setting.

Platform: **Linux-OK**: tests use a fake `cargo` script that prints recorded JSON.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-build-runs-…` in
`crates/fe-workbench/l10n/en-US/build_runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

## Helium filing note

Base: needs T1578 (fe-runlog CargoParser) from mac/main fe6acf280; start only from a helium main containing it, else block and say so. Scope adds lib.rs (module + init), src/tests/mod.rs (the tests::build_runner module), src/l10n.rs (resources!) and l10n-allow.toml (migrated list), which the card needs but didn't list. Kill only your own PIDs, never pkill by name.

## Implementation notes

- `build_runner.rs` follows `test_runner.rs`: trust and root checked before
  saving, the plan worked out off the main thread, the run kept in a global
  keyed by the workbench's `Runs` and dropped as `Runs` drops it. Stdout and
  stderr are read apart (`merged: false`) into `CargoParser::feed_stdout` /
  `feed_stderr`; `CARGO_TERM_COLOR=never` keeps the lines plain.
- `CargoParser` gives its counts only from `finish`, so the summary's
  mid-run warnings and errors are counted by the sink from stderr's rendered
  headers (the parser's header and summary rules, copied); `crates` is the
  distinct packages whose `compiler-artifact` has come. At the end the
  warnings and errors are the parser's.
- The build's problems are one Problems-panel source, its key from
  `TaskTabs::new_key` so it never clashes with a task tab's; a build lists
  only if no build started after it has listed. A cancelled build lists
  nothing, leaving the last build's. Messages with a code read as a task
  tab's rustc problems do (`workbench-lsp-tasks-problem-code`).
- `l10n-allow.toml` has `all = true`, so every file is checked already: no
  migrated-list entry is needed.
- `Workbench::build_process` (the Output view's lines) and
  `Workbench::build_timeline` (for the waterfall card) expose the kept run.
