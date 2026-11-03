---
task_uid: "01m6ezvb7ty8kg4qac1tkfjg5t"
task: "T415"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test task_run_tests::shell_task_becomes_sh_c (cargo nextest run -p fe-workbench) passes: Task::command Shell(\"cargo test -q\") builds TerminalOptions { shell: Some((\"/bin/sh\", [\"-c\", \"cargo test -q\"])), cwd } (the user's login shell is not used), Argv builds the program and arguments directly, and env entries are applied"
    required: true
    manual: false
  - id: "C2"
    text: "Test task_run_tests::run_opens_a_tab_named_for_the_task (cargo nextest run -p fe-workbench) passes: running a task adds a terminal tab titled with its label, shows the terminal dock, and focuses nothing in the editor"
    required: true
    manual: false
  - id: "C3"
    text: "Test task_run_tests::rerun_reuses_the_finished_tab (cargo nextest run -p fe-workbench) passes: running the same label again after it exited replaces that tab's session in place (tab count unchanged); while it still runs, a second run asks nothing and starts a new tab titled 'label (2)'"
    required: true
    manual: false
  - id: "C4"
    text: "Test task_run_tests::exit_code_is_reported (cargo nextest run -p fe-workbench) passes: with a fake session ending in Exited(Some(0)) the tab title gains a check mark suffix and no toast; Exited(Some(2)) gives an error toast 'label failed (exit 2)' with a Show action focusing the tab; Exited(None) says 'label was killed'"
    required: true
    manual: false
  - id: "C5"
    text: "Test task_run_tests::terminal_without_task_is_unchanged (cargo nextest run -p fe-workbench) passes: terminal.new still opens the login shell and the existing terminal_panel_tests/terminal_tabs_tests still pass"
    required: true
    manual: false
---

# Run a project task in a dedicated terminal panel tab that names the task, reuses its tab on rerun and reports the exit code (Mac T1328)

## Problem
f8a lists tasks; nothing runs them. Output goes in the existing terminal panel (crates/fe-terminal already takes `TerminalOptions.shell: Option<(String, Vec<String>)>`), so no new output view is built.

## Change
- `TerminalPanel::add_task(&Task, window, cx)` in terminal_panel.rs: spawns a Terminal with the task's shell override, cwd and env, and tab title = label. The session exit event (`Exited(code)`) is observed to update the title and toast as in the criteria; closing a still-running task tab kills it as terminal.close already does.
- New crates/fe-workbench/src/task_run.rs: `Workbench::run_task(task, window, cx)` calling `show_terminal_dock` like `new_terminal` (workbench_chrome.rs) and remembering `last_task: Option<Task>` for rerun (f8c).
- Sessions are fake-able in tests the way terminal_panel_tests.rs already does; no real process spawns in a test.
No commands or palette entries here (f8c).

## Implementation notes

- `TerminalOptions` had no way to set a child's environment, so it gains `env`, applied over the
  terminal's own variables. That is why the scope grew to fe-terminal and its test literals. An
  `env` wrapper program would have changed the `/bin/sh -c` shape C1 pins.
- A toast's `action` can only run a command with no arguments, and this card adds no commands. The
  Show button is therefore a `Toast::choices` with one title. The panel asks the workbench for the
  dock through `TerminalPanelEvent::ShowRequested`. Show finds the tab by an id unique to the tab and
  kept by a rerun, so it still works after a rerun, and does nothing once the tab has closed, even if
  its name has been reused. It also activates the task's window, since the toast may be in another.
- A session only starts when laid out, so task tabs behind the active one that haven't started are
  laid out beneath it for the frame that starts them. A task runs when asked, not when next looked at.
- `last_task` lives on `TerminalPanel`, read through `Workbench::last_task`, and a new root clears it.
- Beyond the spec, as `task_defs` requires of whoever runs a task: `run_task` refuses in an untrusted
  root, with a toast offering `workbench.trust_folder`.
- Also beyond the spec: a task with a missing `cwd` gets a toast and never falls back to the home or
  root directory as a shell does. A program that can't start gets `Couldn't start <label>` and leaves
  no tab, rather than the `was killed` of `Exited(None)`.
- A lone task tab is listed, since the list is the only place that names it.
- In tests, `fake_sessions` keeps a session's view out of the frame. A view only starts its process
  when laid out, so none starts.
- Known limit: a task inherits the app's environment. `/bin/sh` isn't a login shell, so an app started
  from the macOS Finder has a short `PATH` and may not find `cargo`.
