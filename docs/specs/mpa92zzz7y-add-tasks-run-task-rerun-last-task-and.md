---
task_uid: "01m6ezvdbaaerhvjmpa92zzz7y"
task: "T418"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test task_commands_tests::run_task_lists_file_then_detected_tasks (cargo nextest run -p fe-workbench) passes: tasks.run opens a picker with each task's label, its source (tasks.toml, cargo, npm, make, just) and command as detail, fuzzy filterable, file tasks first; Enter runs the task through Workbench::run_task; Escape runs nothing"
    required: true
    manual: false
  - id: "C2"
    text: "Test task_commands_tests::empty_project_says_so (cargo nextest run -p fe-workbench) passes: with no tasks the picker shows one disabled row 'No tasks: add .tenmoku/tasks.toml' and a TaskDiagnostic is listed as a warning row naming its entry"
    required: true
    manual: false
  - id: "C3"
    text: "Test task_commands_tests::rerun_last_task (cargo nextest run -p fe-workbench) passes: tasks.rerun_last runs the task last run in this window again, and with none toasts 'No task has run yet'"
    required: true
    manual: false
  - id: "C4"
    text: "Test task_commands_tests::run_build_task_uses_the_default (cargo nextest run -p fe-workbench) passes: tasks.run_build runs the default build task, and with none opens the picker filtered to group build"
    required: true
    manual: false
  - id: "C5"
    text: "Test task_commands_tests::tasks_are_rescanned_on_open (cargo nextest run -p fe-workbench) passes: editing tasks.toml between two palette opens changes the list (no stale cache; the list is read when the picker opens, off the main thread)"
    required: true
    manual: false
  - id: "C6"
    text: "Test task_bindings::cmd_shift_b_runs_the_build_task (cargo nextest run -p fe-keymap) passes: tasks.run_build is bound to cmd-shift-b on macOS and ctrl-shift-b elsewhere in the workbench context, no other binding in any context uses those keys, and default_menus_parse still passes with the Terminal menu entries 'Run Task…' and 'Run Build Task'"
    required: true
    manual: false
  - id: "C7"
    text: "command_catalog_tests and typed_command_ids_tests gain one row each for tasks.run, tasks.rerun_last and tasks.run_build and pass"
    required: true
    manual: false
---

# Add Tasks: Run Task, Rerun Last Task and Run Build Task to the palette, with cmd-shift-b, a task picker and a Terminal menu entry (Mac T1329)

## Problem
f8a/f8b can list and run tasks; there is no way to invoke them.

## Change
Three typed commands registered in workbench_commands.rs (`#[kiln::command]`, ids/titles: `tasks.run` 'Tasks: Run Task…', `tasks.rerun_last` 'Tasks: Rerun Last Task', `tasks.run_build` 'Tasks: Run Build Task'), implemented in new task_commands.rs with a picker in the style of crates/fe-workbench/src/picker.rs. Project root = the workspace root. Palette-only for the first two (no key: nvim/VS Code keep none); cmd-shift-b / ctrl-shift-b is VS Code's Run Build Task and is unused in keymaps/fastedit.toml (checked 2026-11-02). Menu: items under the Terminal menu if one exists, else View; mirror an existing entry's shape.

## Implementation notes

- There's no Terminal menu, and `tests/menu.rs` pins the menu bar's names and the first nine View items, so
  C6's "Terminal menu entries" are the View items the Change section falls back to: 'Run Task…' and
  'Run Build Task', a group of their own. `task_bindings::run_task_and_run_build_task_are_in_the_menu` pins them.
- `task_defs::DiskFiles` doesn't exist in tests, so the commands read through `project_files`, which tests
  point at a real temp folder with a seam that counts reads (`TestFiles`).
- Scope gained `task_run.rs` for its module doc, which said nothing there was a command yet.
- A group filter with no tasks says 'No build tasks: add .tenmoku/tasks.toml', since "No tasks" would be false
  in a project that has others. A pick or a default build read from a root the window has since left runs nothing.
