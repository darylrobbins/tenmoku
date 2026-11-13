---
task_uid: "01m78dyv650g1v5fdhra1yq0db"
task: "T360"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::gradle_custom_task::split_args_handles_quotes_and_flags, picker_offers_custom_row_only_with_gradle_or_maven_root, custom_row_runs_wrapper_argv_without_shell, custom_row_prefers_gradle_over_maven, empty_or_unterminated_query_is_not_runnable, rerun_last_repeats_custom_task and untrusted_root_refuses_custom_task pass"
    required: true
    manual: false
  - id: "C2"
    text: "task_commands_tests, task_defs_tests and task_defs_java_tests pass unchanged and task_commands.rs's module doc names the custom row"
    required: true
    manual: false
  - id: "C3"
    text: "The l10n lint passes: the new strings are in commands.ftl, none hard-coded"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Run any Gradle task or Maven goal from the task picker: a typed query becomes a final 'Run ./gradlew <query>' row (wrapper, gradle, mvnw or mvn as detected), run as an argv list with no shell (Mac T1744)

# Run any Gradle task or Maven goal from the task picker

User report (2026-11-12): the default Gradle build, run and test work (T1660 added
fixed `build test run clean check` / `compile test package verify clean` to
`detected_tasks`), but there is no way to run a specific task such as
`:app:bootRun` or `test --tests Foo`.

## Reuse, do not rebuild

`.tenmoku/tasks.toml` already lets a project define any task (`[[task]]`,
`command` as string or argv; `crates/fe-workbench/src/task_defs.rs`). This card adds
the ad hoc path for people who do not want to write a file entry.

## Do

In `crates/fe-workbench/src/task_commands.rs` (`TaskPicker`, `Row`, `set_listing`,
`confirm`):

- When the picker's listing holds a detected Gradle or Maven task (so a build file
  exists at the root; no extra disk read), and the query is non-empty, append a final
  row "Run `./gradlew <query>`" (or `./mvnw` / `gradle` / `mvn`, the same program the
  detected tasks use; Gradle wins when both). It is always shown, whatever the query
  matches, below task rows and above warnings. Enter on it runs a `Task` with
  `TaskCommand::Argv` of the program plus the query split on whitespace, with
  double and single quotes grouping one argument (small pure fn, no shell, never
  `sh -c`), `Source::Gradle`/`Maven`, label = the command line, cwd = root, no group.
  An empty argument list or an unterminated quote shows the row disabled with a
  reason; a first word starting with `-` is allowed (`--tests`, `-q`) and passed
  after the program as typed.
- Multi-module names work because the query is passed through verbatim
  (`:sub:test`, `:app:bootRun --args=x`); say so in the module doc.
- Running goes through `Workbench::run_task`, so trust, the terminal tab and
  `tasks.rerun_last` (which keeps the last `Task`) work with no new code. Add a
  test that rerun_last reruns the custom task.
- New l10n strings in `crates/fe-workbench/l10n/en-US/commands.ftl`
  (`workbench-task-custom-*`), none hard-coded.
- Update the module doc of task_commands.rs.

## Tests (new file)

`crates/fe-workbench/src/tests/gradle_custom_task.rs`, declared from
`crates/fe-workbench/src/tests/mod.rs`, using the harness of task_commands_tests.rs:
split_args_handles_quotes_and_flags, picker_offers_custom_row_only_with_gradle_or_maven_root,
custom_row_runs_wrapper_argv_without_shell, custom_row_prefers_gradle_over_maven,
empty_or_unterminated_query_is_not_runnable, rerun_last_repeats_custom_task,
untrusted_root_refuses_custom_task.

## Left for later (do not do here)

- Listing the project's real tasks (`gradlew tasks --all`) for completion: slow, starts
  a daemon, needs a lazy, cached, off-thread run with a timeout and a trust gate.
- A persisted recent-custom-tasks list across restarts (only the last run is kept now).
- Per-submodule detection (`:sub:task`) in `detected_tasks`; Maven `-pl` helpers.
- Offering "save as task" into tasks.toml.

## Conflicts

T1563 lists `l10n/en-US/workbench.ftl` and l10n.rs (a different file from commands.ftl);
T1570 owns `task_run.rs`, which this card does not change. Platform: Linux-OK.
