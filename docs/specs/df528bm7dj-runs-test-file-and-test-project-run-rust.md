---
task_uid: "01m7cfa7c7h8h98qdf528bm7dj"
task: "T359"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::test_runner (cargo nextest run -p fe-workbench, a fake cargo on PATH printing recorded libtest output) pass: a_file_run_reports_live_counts_and_finishes, results_are_recorded_in_the_history, nextest_is_used_when_the_root_configures_it, files_are_saved_first, an_untrusted_root_refuses, the_commands_have_titles"
    required: true
    manual: false
  - id: "C2"
    text: "The existing tests.* commands and the Run Test lens behave as before: cargo nextest run -p fe-workbench test_commands test_lens passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Runs: Test File and Test Project run Rust tests as Runs with live counts and per-test results recorded in the history (Mac T1575)

# Run tests as a Run: Runs: Test File and Test Project

Spec: Runs: tests (`docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`):
a test run in progress with live counts, results kept per test.

## Change

- `crates/fe-workbench/src/test_runner.rs` (new): starts a Rust test run
  through the background runner (T1572) with `LibtestParser` (T1573): the
  command comes from `test_runnables::test_command` (file) or `cargo test
  --workspace` (project); when `cargo nextest` is on `PATH` and the root has a
  `.config/nextest.toml` or `nextest` in its Cargo workspace metadata it uses
  `NextestParser` instead. It saves files first (as `test_commands.rs` does).
- The Run's summary is `Tests { passed, failed, total }`, updated as events
  arrive (`total` from `running N tests`); the finished run is recorded in the
  `History` (T1574) for the root's state directory, and the Run keeps its
  per-test results (name, outcome, output) for the detail view.
- Commands `runs.test_file`, `runs.test_project` (palette titles "Runs: Test
  File", "Runs: Test Project"). The existing `tests.*` commands, the Run Test
  lens and their terminal tabs are unchanged.
- `crates/fe-workbench/Cargo.toml` gains the `fe-runlog` dependency.

## Not in this card

The detail view (a later card), Go and pytest, rerun-failed, debugging.

Platform: **Linux-OK**: the tests use a fake `cargo` script on `PATH` that
prints recorded libtest output.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-test-runs-…` in
`crates/fe-workbench/l10n/en-US/test_runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

## Implementation notes

- The file run's command is `test_command`'s for `tests.run_file`, without
  `--nocapture` (so libtest captures each test's output and a failure's
  output is its own) and with `--no-fail-fast`. The project run is
  `cargo test --workspace --no-fail-fast`. Under nextest:
  `cargo nextest run <same args> --no-fail-fast --message-format libtest-json`
  with `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`.
- Nextest is detected in the folder the command runs in (the workspace's,
  as `test_command` picks it; the root for the project run): a
  `.config/nextest.toml` or `[workspace.metadata.nextest]`, and a
  `cargo-nextest` on the login shell's `PATH`.
- `Run` (runs.rs) is unchanged: the per-test results live beside it in
  test_runner.rs, keyed by the workbench's `Runs` entity and the run id, for
  as long as `Runs` keeps the run (`Workbench::test_results`,
  `Workbench::test_process` for the detail view). They're dropped, and their
  programs stopped, when the `Runs` goes with its workbench.
- A cancelled run isn't recorded in the history. Ignored tests aren't
  recorded (History has passed and failed only).
- `l10n-allow.toml` checks every file (`all = true`), so there's no
  migrated list to add to.
- Review round 1 (F1–F6): `run_process.rs` gains `RunSpec::merged` (stdout
  and stderr written to one pipe, so cargo's `Running` lines on stderr keep
  their place among the results on stdout), `OutputLine::ends` with
  `LineSink::fragment` for the parts of a line past `MAX_LINE` (the test
  sink puts them back together, up to 32 MiB), and `LineSink::end`, called
  once the program has exited and its output is read (the test sink flushes
  the parser's held-back failures there). History writes go through one
  queue in the order runs ended. A test run's results are dropped as `Runs`
  evicts its run. Nextest is looked up as the program will be: executable,
  relative `PATH` entries from the command's folder, the workbench's own
  `PATH` when the login shell's can't be read.
