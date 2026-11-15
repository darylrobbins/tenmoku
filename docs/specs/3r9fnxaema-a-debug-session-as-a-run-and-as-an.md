---
task_uid: "01m7f6twhh4ft0rh3r9fnxaema"
task: "T659"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test tests::debug_runs::a_session_is_a_run_from_start_to_end passes (cargo nextest run -p fe-workbench debug_runs): starting a configuration adds a Run titled by it (the kind D0 names), running while it runs or is stopped, ended with the debuggee's exit code"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_runs::a_failed_launch_ends_its_run_failed passes, with the failure's message as its summary"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_runs::launching_is_an_operation_until_configured passes: from the pre-launch task through configuration completion the session is one operation in the status bar's operations list, labelled as D0 says, and it ends on failure too. Known limit (helium lead, review round 2 F1): the end of configuration is read from the session state the workbench has today; a Stopped event before configuration completes may end the operation early. A configuration-complete signal from fe-dap (crates/fe-dap/src/session.rs, crates/fe-workbench/src/debug_session.rs) is a follow-up card, not this one"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_runs::the_runs_panel_draws_a_debug_run passes, with the accessible name D0 gives it and D0's icon where kiln-ui's bundled icon set has it. Known limit (helium lead, review round 2 F3): the set has no pause glyph (kiln/crates/kiln-ui/src/file_icons.rs), so a stopped row keeps the muted bug icon; adding Pause to kiln-ui and drawing stopped rows with it in the accent colour is a follow-up card, not this one"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# A debug session as a Run and as an operation in the status bar (debugger D7b) (Mac T1848)

# A debug session as a Run and as an operation in the status bar (debugger D7b)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **s**.

## Problem

B3 has one shape for everything that runs (design-system.md "Runs"); §13 asks that a
debug session show in the Runs panel and in the status bar's operation in progress.

## What to build (§13 "B3 design first"; D0's Debugger section)

- Register each session with the runs model (`runs.rs`, T1570 landed): a run of the
  kind D0 names (a new `RunKind::Debug` if D0 says so), its title, status and exit.
- An operation (`operations.rs`, T1585 landed) while the session launches: the
  pre-launch task, the adapter start and configuration.
- The Runs panel's row for it (T1571's `runs_panel.rs`, which must land first).
- Follow sessions through D7's session events; `debug_session.rs` is not in scope.

Split from §11 D7 to keep D7 inside an l.

## Not in this card

The Runs panel itself (T1571), the status bar's operations view (T1586).

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

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).
