---
task_uid: "01m7f6tw2hz8gdjbsb069c6frb"
task: "T658"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Test tests::debug_session::an_untrusted_root_lists_configurations_and_refuses_to_start_one passes (cargo nextest run -p fe-workbench debug_session), with the toast \"Trust this folder to debug its programs\" and its Trust action"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_session::a_failing_pre_launch_task_starts_no_session passes, and a detected Cargo configuration's program is read from cargo's compiler-artifact message through the task runner's read-output-at-exit seam"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_session::a_stop_opens_the_file_at_the_stopped_row_and_announces_it passes against the fake: the stopped file opens with the caret on the stopped row and \"Stopped at line N\" is announced"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_session::untrusting_stops_a_running_session passes"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::debug_session::a_debuggee_tab_never_becomes_the_last_task passes: after a debuggee tab's session ends, tasks.rerun_last still reruns the last real task, and the tab's rerun starts a new session with a new runInTerminal rather than replaying the old argv"
    required: true
    manual: false
  - id: "C6"
    text: "Tests tests::debug_session::changing_log_payloads_applies_to_the_next_session and tests::debug_session::breakpoints_are_sent_on_change_and_on_save_not_per_keystroke pass"
    required: true
    manual: false
  - id: "C7"
    text: "Test tests::debug_session::a_ptrace_refusal_names_ptrace_scope passes, with the scope read through a seam (no /proc access in the test)"
    required: true
    manual: false
  - id: "C8"
    text: "Every command this card adds has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Debug sessions in the workbench: debug.start, stop, rerun and log, folder trust, pre-launch tasks and the debuggee's terminal tab (debugger D7) (Mac T1847)

# Debug sessions in the workbench: debug.start, stop, rerun and log, folder trust, pre-launch tasks and the debuggee's terminal tab (debugger D7)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

fe-dap can run a session; the workbench must start one from a configuration, keep it
behind folder trust, run its pre-launch build, give the debuggee a terminal, and
send breakpoints as they change.

## What to build (§5, §7, §8; §11 D7; §13)

- Commands `debug.start` (its configuration picker; continue while stopped),
  `debug.stop`, `debug.rerun_last`, `debug.show_log`, in the one
  `#[kiln::command] impl Workbench` (`workbench_commands.rs`).
- **Trust** (§8): the live check just before launching on every path, its toast, the
  trust prompt's explanation gaining "and run its debug configurations"
  (`extensions.ftl`), untrusting stops a running session.
- **`pre_launch_task`** via `Workbench::run_task`, and a small read-output-at-exit
  seam in `task_run.rs`/`terminal_panel.rs` for cargo's `compiler-artifact` JSON (§7).
  Gradle and Maven tasks (T1660, landed) are ordinary tasks here.
- **`runInTerminal`** into a debugger-owned tab, `TerminalPanel::add_debuggee`
  (§5): a task tab's look, no `last_task`, never replaying the argv; its rerun goes
  through `debug.rerun_last`.
- On a stop, open the frame's file at its row (D6 adds the band and arrow).
- Breakpoints sent on change and after a save that moved rows (§5 "Sending").
- `DebugSettings` applied through `reload_settings` (`workbench_root.rs`).
- The ptrace (§10, names `ptrace_scope` and its value) and macOS signing failure toasts.
- Announcements of session changes (§13).
- **An API the later cards build on without editing this module**: an event other
  modules subscribe to for session state, stops (file, row, frame), output and
  breakpoint verification (verified, message, adapter line, kept per breakpoint for
  the session's lifetime), and a query for the control state (D2b). D6 (gutter, band),
  D7b (Runs), D9 (panels) use it; D8, D9 and D10 extend the module itself.

**Order change from §11:** §13 says engine cards don't wait on the design card D0,
so D7 no longer depends on D6; D6 comes after D7 and draws the band and arrow from
D7's session state.

## Notes on shared files

`terminal_panel.rs` and `task_run.rs` are in several ready cards' scopes (T1579,
T1595, T1596, T1597, T1774, T1808, T1813); keep the seams small and rebase over
whichever land first.

## Not in this card

Gutter, band and arrow (D6), Runs and operations (D7b), stepping commands and the
toolbar (D8), panels (D9), console (D10), Java (D21b).

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

## Implementation notes

Design calls are in `docs/decisions/sb069c6frb-debug-sessions.md`. The
module is `crates/fe-workbench/src/debug_session.rs` with
`debug_session/launch.rs` (launch arguments, cargo's artifact, refusals) and
`debug_session/picker.rs`; the tests are `src/tests/debug_session.rs`.
Review round 1 (review 1vjqnz): F1-F12 fixed, each with a regression test in
`src/tests/debug_session.rs`. F11 needed fe-dap: the task was respecced to add
`crates/fe-dap/src/session.rs`, `session/**`, `launcher.rs`, `lib.rs` and
`tests/**`, for `SessionOptions::timeouts` and the launcher's connect timeout.

Review round 2 (review ahetre): F4, F7 and F13-F16 fixed with tests. F4 needed
a raw output tap below the terminal grid: the task was respecced to add
`crates/fe-terminal/src/{session,pty_loop,lib}.rs`,
`crates/fe-terminal-view/src/view.rs` and `crates/fe-terminal/tests/output_tap.rs`.

Review round 3 (review 4g2mdp): F16, F17 and F18 fixed with tests
(`a_retained_stop_retires_the_last_place_and_is_told`,
`a_stopped_file_read_after_its_stop_ends_never_opens`,
`a_debuggee_started_just_before_the_stop_keeps_its_tab`).
