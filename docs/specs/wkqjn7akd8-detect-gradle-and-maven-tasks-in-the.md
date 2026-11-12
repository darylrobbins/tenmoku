---
task_uid: "01m70e5638m9fqwswkqjn7akd8"
task: "T1660"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "task_defs_java_tests::gradle_root_detects_wrapper_tasks, gradle_without_wrapper_uses_gradle, maven_root_detects_mvn_tasks_and_prefers_mvnw, gradle_beats_maven_for_default_build, detected_java_tasks_never_reach_a_shell and a_project_with_none_gets_none pass"
    required: true
    manual: false
  - id: "C2"
    text: "task_defs's existing tests pass unchanged and the module doc's Detection list names Gradle and Maven"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Detect Gradle and Maven tasks in the project's task list (gradlew or gradle build, test, run, clean, check; mvnw or mvn compile, test, package, verify, clean) beside Cargo, npm, make and just

# Detect Gradle and Maven tasks in the project's task list

User report (2026-11-09): Java ecosystem support isn't great. The seam exists:
`crates/fe-workbench/src/task_defs.rs` `detected_tasks` reads only the project
root and detects Cargo.toml (cargo build/test/run/clippy), package.json scripts,
Makefile targets and justfile recipes, as argument lists that never reach a shell
(module docs "Detection"; `Source` enum, `detected`, `detected_as`). The Runs
cards (T1570 model, T1571 panel, T1572 runner) build on task runs, so a detected
task shows up there for free; this card does not touch them. T1409 (run tests at
cursor) is abandoned and T1410 (Run Test lens) is finalized for Rust/Go/Python;
Java test lenses are out of scope here.

## Do

In `crates/fe-workbench/src/task_defs.rs`, after the justfile group:

- Gradle: if the root has `build.gradle` or `build.gradle.kts` (or
  `settings.gradle(.kts)`), detect `build`, `test`, `run`... as Cargo does with a
  fixed list, using the wrapper when the root holds an executable-looking
  `gradlew` (argv ["./gradlew", task]) else `gradle`. Groups: build -> Build,
  test -> Test, others none. Fixed list: build, test, run, clean, check.
- Maven: if the root has `pom.xml`: `mvn` (or `./mvnw` when mvnw exists) with
  compile, test, package, verify, clean; compile/package -> Build group, test
  -> Test. Gradle takes precedence over Maven as the first default build/test
  task when both exist (state which in the module docs).
- The default-group fallback at ~:428 (`Source::Cargo | Source::Npm`) gains
  `Source::Gradle | Source::Maven`; keep order: Cargo, npm, Gradle, Maven, Make, Just.
- Update the module-doc Detection list and `Source`'s label mapping wherever Source
  is matched exhaustively (`rg -n 'Source::Just' crates/fe-workbench`).

## Tests (new file)

`crates/fe-workbench/src/task_defs_java_tests.rs` (declared next to
task_defs_tests.rs) using the same `ProjectFiles` fake as task_defs_tests.rs:
gradle_root_detects_wrapper_tasks, gradle_without_wrapper_uses_gradle,
maven_root_detects_mvn_tasks_and_prefers_mvnw, gradle_beats_maven_for_default_build,
detected_java_tasks_never_reach_a_shell (argv list), a_project_with_none_gets_none.

## Conflicts

T1539 (title-bar Run widget), T1561 (i18n strings for tasks) and T1570 touch
`task_run.rs`/tasks UI, not `task_defs.rs`'s detection; check no ready card lists
`crates/fe-workbench/src/task_defs.rs` in its scope before starting.
Platform: Linux-OK.
