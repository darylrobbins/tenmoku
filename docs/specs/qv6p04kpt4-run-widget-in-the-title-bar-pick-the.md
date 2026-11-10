---
task_uid: "01m6vvtjd4hb1m1dqv6p04kpt4"
task: "T1539"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::run_widget_b3 (cargo nextest run -p fe-workbench) pass: with tasks, the widget shows the chosen task (remembered per project), play runs it, stop shows while it runs and stops it, a failed run shows the error glyph; with no tasks nothing is drawn"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (only if the golden scene detects a task; otherwise no golden changes)"
    required: true
    manual: false
  - id: "C4"
    text: "Tests in tests::run_widget_b3 also cover review r1's findings: after the project root changes, the widget lists and runs the new root's tasks with its cwd (F1); the chosen task survives a title bar reinstall and a layout save and restore (F2); adding, editing or removing a task definition updates the list and what play runs, with no run in between (F3); a run of the chosen task started by tasks.run, run_build or rerun_last shows as running and can be stopped from the widget, and a rerun keeps it tracked (F4)"
    required: true
    manual: false
---

# Run widget in the title bar: pick the project's task, run it, stop it

# Run widget in the title bar: the default task with run and stop

Spec: `docs/design/design-system.md#workbench-anatomy` (Title bar: run
configuration with run/stop/debug). Tasks exist (`task_defs.rs`,
`task_run.rs`); this is a control for them. Debug waits for the debugger
design (T1415; "Debugger" is still to design).

## Change

- New `crates/fe-workbench/src/run_widget.rs`: in the title bar's right group,
  before search: the selected task's label ▾ (opens the task picker; the
  choice is remembered per project in the workspace state), a play button
  (runs it; the key from `task_commands.rs`), and a stop button while it runs.
  A running task shows the spinner icon; a failed last run the error glyph.
- With no tasks the widget shows nothing (so the golden scene, which has no
  tasks, only changes if it detects one: check).
- Wired in `crates/fe-workbench/src/workbench_chrome.rs`.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `run_widget.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/run.ftl` (keys `workbench-run-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (the title bar golden).

## Implementation notes

- The widget is an entity held by the title bar's right slot (`run_widget::install`, called from `set_title_bar`), so it needs no new Workbench fields. It runs through `Workbench::run_task`, follows its session's exit itself, and stops by closing that session.
- The choice is remembered per root for the life of the window (in memory); nothing is persisted across restarts.
- The demo project has a Cargo.toml, so the scene detects cargo tasks and the five goldens changed (checked: `cargo build ▾ ▷` before search).

Revision 3 (review r1): the choice moved into the workbench's saved layout (`run_tasks`, by root), so it survives a title bar reinstall and a layout restore; the widget follows `RootChanged` (old tasks dropped at once, nothing shows or runs until the new root's are read) and the project panel's file watching for task-defining files; play and the picker read the tasks afresh; status and stop come from the terminal panel's task tabs (now keyed by label and cwd), so runs from `tasks.run`, `tasks.run_build` and `tasks.rerun_last` show and stop, and a rerun stays tracked.
