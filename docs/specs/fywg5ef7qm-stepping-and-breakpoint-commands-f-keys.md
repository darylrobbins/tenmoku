---
task_uid: "01m7c5f9rr052jk2fywg5ef7qm"
task: "T1850"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::debug_commands (cargo nextest run -p fe-workbench debug_commands) pass, one per command against the fake: debug.continue, debug.pause, debug.step_over, debug.step_into, debug.step_out, debug.restart, debug.run_to_cursor (a one-shot breakpoint removed at the next stop whatever its reason), debug.start_with_args, debug.show_session, debug.toggle_breakpoint, debug.toggle_breakpoint_enabled, debug.remove_all_breakpoints"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in crates/fe-keymap/tests/debug_bindings.rs (cargo nextest run -p fe-keymap --test debug_bindings) pass: §6's F-key table, and f10_opens_the_menu_bar_on_linux_with_no_session_and_steps_over_while_stopped (likewise f11 and fullscreen), through the debug_stopped context key"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_toolbar::disabled_states_follow_the_session passes: Stopped, Resuming, Running and Uncertain, read from D2b's control-state query"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_toolbar::the_toolbar_shows_without_a_title_bar passes: on Linux, which has no title row, and on macOS without a unified title bar; with a title bar it follows D0's placement"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::debug_toolbar::buttons_have_accessible_names passes"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-gpui command_tests tests passes with the new bindings in the binding counts and the DEDICATED table"
    required: true
    manual: false
  - id: "C7"
    text: "Every command this card adds has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C8"
    text: "On the Mac, the audit scene crates/fe-workbench/examples/screenshots/audit/debug_toolbar.rs renders in Yohen and Hakuji and its probes pass (recorded in the decision file); the scene must not skip on SCREENSHOTS_ALLOW_OTHER_SCALE"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Stepping and breakpoint commands, F-keys while stopped, and the floating debug toolbar where there is no title bar (debugger D8)

# Stepping and breakpoint commands, F-keys while stopped, and the floating debug toolbar where there is no title bar (debugger D8)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

A stopped session needs stepping, breakpoint commands and keys, and visible controls.

## What to build (§5 "Debug toolbar", "Run to cursor"; §6; §11 D8; §13 Q4, B3)

- Commands `debug.continue`, `debug.pause`, `debug.step_over`, `debug.step_into`,
  `debug.step_out`, `debug.restart`, `debug.run_to_cursor`, `debug.start_with_args`,
  `debug.show_session` (one entry in v1), `debug.toggle_breakpoint`,
  `debug.toggle_breakpoint_enabled`, `debug.remove_all_breakpoints`.
- The `debug_stopped` context key at the workbench root's `key_context`, and §6's
  F-key table in `crates/fe-keymap/keymaps/fastedit.toml`. **Q4 (§13):** on Linux
  F10 and F11 step only while stopped and keep the menu bar and fullscreen otherwise.
- **The floating toolbar** (§5) at the top centre of the pane area, **where there is
  no title bar** (Linux, and macOS without the unified title bar). With a title bar
  the controls go where D0 puts them (D8b builds the run-widget part). Each button
  dispatches a `debug.*` command; disabled states come from D2b's control-state query.
- Icons from Lucide added to kiln-ui's set if D0 calls for ones it lacks (play,
  square, redo and bug are there; keep the 1.75 stroke).

Scope traps (§6): new bindings break the fe-gpui binding-count tests and the
`DEDICATED` table; each command needs catalog rows.

## Not in this card

The run widget's Debug button and launch-config list (D8b), panels (D9), console
(D10), preset keys (D12).

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
