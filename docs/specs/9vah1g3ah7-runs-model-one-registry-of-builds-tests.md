---
task_uid: "01m7ac1rzrn1rf6k9vah1g3ah7"
task: "T605"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::runs_model (cargo nextest run -p fe-workbench) pass: a_task_run_appears_running_and_finishes_from_its_exit_code, closing_a_running_tab_cancels_its_run, a_rerun_is_a_new_run, kind_follows_the_task_group (Build, Test, else Script), finished_runs_are_capped_at_100_and_running_ones_stay, the_list_is_newest_first"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench task_run passes (today's task_run behaviour and tests are unchanged)"
    required: true
    manual: false
---

# Runs model: one registry of builds, tests and scripts (kind, status, title, summary, progress, age), fed by task runs (Mac T1570)

# Runs model: one registry for builds, tests and scripts

Spec: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
("Everything that runs has the same shape: a status icon, a mono title, a
one-line summary, and an age. A running item shows a thin rust progress
line"). Today a task runs in a terminal tab (`task_run.rs`, `TaskTabs`) and
nothing lists runs.

## Change (headless; no panel)

- `crates/fe-workbench/src/runs.rs` (new): `Runs`, kept by the workbench:
  `Run { id, kind: Build | Test | Script | Agent, title, status: Running {
  progress: Option<f32> } | Passed | Failed { code: Option<i32> } | Cancelled,
  summary: RunSummary (None | Exit(code) | Tests { passed, failed, total:
  Option<usize> } | Build { crates, warnings, errors }), started, finished }`.
  `begin`, `update`, `finish`, `list` (newest first), and a change event the
  panel will observe. Finished runs are capped at 100 (running ones are never
  evicted). `Agent` exists for the lane design; nothing creates one here.
  Titles and summaries are data: the panel formats text.
- `crates/fe-workbench/src/task_run.rs`: a task tab starting begins a Run (kind
  from the task's `Group`: Build, Test, else Script), its session exiting
  finishes it from the exit code, closing the tab while it runs cancels it, a
  rerun in the tab is a new Run.

## Not in this card

A panel (next card), output parsing, the background runner, agent runs, any
setting.

Platform: **Linux-OK**.

## Implementation notes

- `Runs` is an `Entity` on the workbench (`Workbench::runs`); it emits
  `RunsEvent::Changed(id)` and notifies on every begin, update and finish.
- `finish` takes a `RunEnd` (Passed, Failed, Cancelled) and an optional summary;
  a task run's summary is `Exit(code)` when the code is known. `update` and
  `finish` leave a finished or evicted run alone and return false.
- The cap drops the runs that *finished* longest ago, so a long run that began
  first and just finished is kept. The list stays in begin order, newest first.
- A tab that reruns after an unheard exit cancels its old run as the new
  one begins.
