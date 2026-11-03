---
task_uid: "01m6ezv9m4b3j4n4atb1h9hzbn"
task: "T413"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test task_defs_tests::tasks_file_parses (cargo nextest run -p fe-workbench) passes: [[task]] entries with label, command (string run by the shell, or array run directly), cwd (relative to the project root), env table, group = build|test|run and default = true parse; a duplicate label, an empty command or a cwd escaping the root is a TaskDiagnostic naming the entry, and the valid entries survive"
    required: true
    manual: false
  - id: "C2"
    text: "Test task_defs_tests::cargo_targets_are_detected (cargo nextest run -p fe-workbench) passes: a Cargo.toml at the root yields 'cargo build' (group build, default), 'cargo test' (group test), 'cargo run' and 'cargo clippy', and a [workspace] one the same; no Cargo.toml yields none"
    required: true
    manual: false
  - id: "C3"
    text: "Test task_defs_tests::npm_scripts_use_the_right_package_manager (cargo nextest run -p fe-workbench) passes: package.json scripts become 'npm run \u003cname\u003e', or 'pnpm run', 'yarn' or 'bun run' when pnpm-lock.yaml, yarn.lock or bun.lockb/bun.lock sits beside it; a package.json that is invalid JSON or has no scripts yields none. Known limit: a script name beginning with '-' is passed to the runner as an option (not protected with the runner's positional-argument syntax); documented in task_defs.rs, follow-up card holds the fix"
    required: true
    manual: false
  - id: "C4"
    text: "Test task_defs_tests::make_and_just_targets_are_listed (cargo nextest run -p fe-workbench) passes: Makefile/GNUmakefile targets (skipping .PHONY and other dot targets, pattern rules and variable assignments) become 'make \u003ct\u003e', and justfile/Justfile recipes (skipping underscore-prefixed private ones and recipe parameters) become 'just \u003cr\u003e'. Known limit: a make target name containing literal parentheses is not detected (GNU make's archive-member syntax lib(member) shares them); noted in a comment in task_defs.rs, follow-up card holds the fix. Detection is best-effort; also known limits: just recipe headers or attributes continued across escaped newlines or open delimiters (a multiline attribute may expose a hidden recipe, a multiline header may lose a recipe), a make variable named 'define' (an assignment, not the directive) can suppress later targets, and a make target beginning with '-' is passed to make as an option; all noted in task_defs.rs, follow-up card holds the fixes"
    required: true
    manual: false
  - id: "C5"
    text: "Test task_defs_tests::file_tasks_win_over_detected_ones (cargo nextest run -p fe-workbench) passes: a tasks.toml label equal to a detected label replaces it, and the final order is file tasks first, then detected, each in file order"
    required: true
    manual: false
  - id: "C6"
    text: "No test touches the real filesystem: detection reads through a ProjectFiles trait (read_to_string, exists) that the tests fake; the real impl uses std::fs and is built only in the app"
    required: true
    manual: false
---

# Define project tasks: parse .tenmoku/tasks.toml and detect cargo, npm, make and just targets from the project files, behind a file-reading seam (Mac T1327)

## Problem
There is no task runner. User approved 2026-11-02 as a day-one gap: run project tasks from the palette, output in a panel, problem matching if cheap. This card is the data model only (f8b runs, f8c adds commands, f8d/f8e match problems).

## Change
New crates/fe-workbench/src/task_defs.rs (module in lib.rs, nothing else):
- `Task { label, command: TaskCommand (Shell(String) | Argv(Vec<String>)), cwd: PathBuf, env: BTreeMap, group: Option<Group>, default: bool, source: Source (File | Cargo | Npm | Make | Just), problem_matcher: Option<String> }`.
- `load(files: &dyn ProjectFiles, root) -> (Vec<Task>, Vec<TaskDiagnostic>)`. The tasks file is `<root>/.tenmoku/tasks.toml` (matches `.tenmoku/extensions`); schema in the doc comment. `problem_matcher` is parsed and carried as a plain string for f8d; it is not interpreted here.
- Detection is shallow (project root only, no workspace-member scanning, no recursion), plain text parsing with serde_json/toml (already dependencies) and line scanning for make/just: no new crate.
- Default-per-group: at most one default build task and one default test task; first marked default wins, else cargo/npm 'build'/'test' by name.

## Implementation notes

- `serde_json` was only a dev-dependency of fe-workbench, so rev 2 added `crates/fe-workbench/Cargo.toml` to the scope to move it to `[dependencies]`. No new crate.
- npm scripts keep `package.json`'s order through a small visitor, because `serde_json`'s default map sorts its keys and this crate doesn't turn on `preserve_order`.
- Detected commands are always `Argv`, so a name from a project's file never reaches a shell.
- `Task::cwd` is resolved to the root or a path under it, so the runner has nothing to join.
- Beyond the card: `makefile` is read too, because `make` looks for it between `GNUmakefile` and `Makefile`; a just `[private]` attribute hides a recipe, as an underscore does; an unknown entry key is a diagnostic that keeps the entry; `default` with no group is cleared with a diagnostic; a second default in a group, `run` included, loses it with a diagnostic.
