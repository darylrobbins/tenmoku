---
task_uid: "01m7c5fxbt7sw4pg0esrgpcs1p"
task: "T1853"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test tests::debug_console::output_events_append_in_order_with_stderr_tinted passes (cargo nextest run -p fe-workbench debug_console), multi-line output keeping its rows and telemetry dropped"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_console::an_evaluations_result_row_expands passes, evaluated with context repl against the selected frame"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_console::history_recalls_with_up_and_down passes"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_console::debug_show_console_shows_and_focuses_then_hides passes, and the input and rows have accessible names"
    required: true
    manual: false
  - id: "C5"
    text: "Every command this card adds has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C6"
    text: "On the Mac, the audit scene crates/fe-workbench/examples/screenshots/audit/debug_console.rs renders in Yohen and Hakuji and its probes pass (recorded in the decision file); the scene must not skip on SCREENSHOTS_ALLOW_OTHER_SCALE, with mixed output"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Debug Console: adapter output by category and a repl input with history and expandable results (debugger D10)

# Debug Console: adapter output by category and a repl input with history and expandable results (debugger D10)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

Adapters send program and adapter output, and users evaluate expressions in a REPL.

## What to build (§5 "Debug Console"; §11 D10; D0's section)

- The console where D0 puts it: a tab of the Debug tool window (D9) or of the bottom
  terminal panel (§5's original place). Not a terminal session: an output list of
  `output` events by category plus a one-line input.
- Output rows coloured by category (stderr tinted), multi-line output kept.
- Input with history, `repl` evaluation against the selected frame, expandable result
  rows (the console's own list, not a popover).
- Command `debug.show_console` (again: hide the bottom panel).

Only one of `debug_panel*` or `terminal_panel.rs` should change, as D0 decides.

## Not in this card

ANSI styling and completions (later), logpoint output (D17).

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
