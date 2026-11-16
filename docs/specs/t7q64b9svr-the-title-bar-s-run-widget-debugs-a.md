---
task_uid: "01m7c5fdsdxy2m6tt7q64b9svr"
task: "T1851"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test tests::run_widget_debug::the_picker_lists_launch_configurations_beside_tasks passes (cargo nextest run -p fe-workbench run_widget_debug), file and detected ones, with their sources"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::run_widget_debug::debug_starts_the_chosen_configuration passes: the Debug button dispatches debug.start for the chosen launch configuration, and a choice it can't debug leaves it disabled with the reason as its tooltip"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::run_widget_debug::a_session_shows_its_controls_in_the_title_bar passes, as D0 places them, and the choice is kept per root as tasks' is"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::run_widget_debug::the_widgets_debug_controls_have_accessible_names passes; cargo nextest run -p fe-workbench run_widget_b3 passes"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac, the audit scene crates/fe-workbench/examples/screenshots/audit/debug_run_widget.rs renders in Yohen and Hakuji and its probes pass (recorded in the decision file); the scene must not skip on SCREENSHOTS_ALLOW_OTHER_SCALE"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# The title bar's run widget debugs: a Debug button and launch configurations beside tasks (debugger D8b)

# The title bar's run widget debugs: a Debug button and launch configurations beside tasks (debugger D8b)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

B3's title bar holds "run configuration with run/stop/debug" (design-system.md,
Workbench anatomy). The run widget (T1539, `crates/fe-workbench/src/run_widget.rs`)
runs tasks but can't debug.

## What to build (§13 "B3 design first"; D0's section)

- Launch configurations (D4: file, imported and detected) listed beside tasks in the
  widget's picker, with their sources.
- A Debug button that starts the chosen configuration through `debug.start`.
- While a session runs with a title bar: the controls D0 places there, dispatching
  D8's commands. §5's floating toolbar (D8) stays where there is no title bar.

Split from §11 D8 (title bar half). Keep the work in `run_widget_debug.rs` and leave
`title_bar.rs` alone (Mac T1768 and T1805 are changing it); if a slot change there is
unavoidable, block for a scope add.

## Not in this card

Commands and the floating toolbar (D8), multi-session choice (D23).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Mac** — acceptance includes a macOS look check of an audit scene (§5, §10), which review fails when made on Linux. The headless criteria can be built on Linux first.
