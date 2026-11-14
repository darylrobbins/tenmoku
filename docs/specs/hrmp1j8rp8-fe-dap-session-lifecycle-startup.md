---
task_uid: "01m7cw13kq7hyz5dhrmp1j8rp8"
task: "T646"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Tests in tests::session_startup (cargo nextest run -p fe-dap session_startup) pass against the fake, one per §2 item 3 ordering: initialized_then_configuration_then_launch_response, launch_response_then_initialized, without_configuration_done_support_configuration_ends_on_the_last_reply; plus exception_breakpoints_are_sent_only_under_the_spec_condition and a_missing_initialized_counts_configuration_complete_after_5_s"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::session_startup::configuration_done_timeout_ends_the_session_with_a_named_error passes (F10, §13): a fake that never answers configurationDone ends the session with that named error after the timeout on the injected clock, and the adapter is killed"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in tests::session_restart pass (§11 D2 C6): with supportsRestartRequest a successful restart reaches Running with a fresh baseline and re-sends breakpoints on the new initialized; a failed or unanswered restart falls back to stop and start; a restart completion after Stop changes nothing"
    required: true
    manual: false
  - id: "C4"
    text: "Tests in tests::session_terminate pass (§11 D2 C7): user Stop sends terminate, then disconnect on terminated, and kills a mute adapter 2 s after the disconnect; an adapter's own terminated gets disconnect at once and no terminate; an adapter exiting on its own ends as AdapterExited with no restart"
    required: true
    manual: false
  - id: "C5"
    text: "Tests in tests::session_requests pass: per-command timeouts (10 s, 30 s for launch, none for repl evaluate), a dropped request future sends cancel only with supportsCancelRequest, runInTerminal reaches the sink as an event whose reply carries the processId, and startDebugging is answered with an error"
    required: true
    manual: false
  - id: "C6"
    text: "Tests in tests::session_log pass (§11 D2 C8): the default log holds no argument, variable, evaluation or output value and no error text; a failed launch whose message echoes the expanded ${env:API_TOKEN} leaves only the failure and its error id; with payloads on, that token shows as ${env:API_TOKEN} in the request, the error message and any adapter echo"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-dap passes"
    required: true
    manual: false
---

# fe-dap session lifecycle: startup orderings, configuration with a configurationDone timeout, restart, termination and the redacted protocol log (debugger D2) (Mac T1841)

# fe-dap session lifecycle: startup orderings, configuration with a configurationDone timeout, restart, termination and the redacted protocol log (debugger D2)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

D1 gives fe-dap a transport and a fake. A session needs §2's lifecycle: an adapter
may answer `launch` before or after configuration, may lack `configurationDone`,
may restart, and must be torn down without hanging. The review of the design left
an open finding here (F10, §13): `configurationDone` had no timeout.

## What to build (§2 items 1-3, 6, 7; §11 D2; §13 F10)

`fe_dap::Session` and its actor (one thread per session named `fe-dap <adapter>`):

- **Initializing**: `initialize`, keep `Capabilities`, merge later `capabilities` events.
- **`launch` not awaited** before configuration (§2 item 2).
- **Configuring** on each `initialized`: per-file `setBreakpoints`,
  `setExceptionBreakpoints` only under §2's condition, then `configurationDone` when
  supported; configuration completion with and without it; the 5 s missing-`initialized`
  rule; the thread baseline `threads` once both the launch response and configuration
  completion have come.
- **F10 (§13):** `configurationDone` has a timeout; on expiry the session ends with a
  named error instead of waiting forever.
- **Restart** with its own guard (item 7), **termination** both ways (item 6), the
  2 s kill, `exited` codes, `Ended(AdapterExited)` with no automatic restart.
- **Requests**: per-command timeouts on the injected clock, `cancel` on drop.
- **Reverse requests**: `runInTerminal` as `DapEvent::RunInTerminal { …, reply }`;
  `startDebugging` refused ("child sessions are not supported", §2).
- **The protocol log**: metadata only by default, payloads opt-in as a `Session`
  option (D7 sets it from D3's setting), redaction of `env` values and `${env:NAME}`
  expansions in every mode (§2 "Output and logging", §8).

The stop/resume half of the state machine (epochs, the resume guard, Uncertain,
pause) is D2b; build the state enum so D2b adds to it. Add fake knobs only if a
criterion needs one D1 lacks (an unanswered `configurationDone`).

## Not in this card

Stop epochs, lazy stop data, steps, Uncertain, pause and F2/F11 (D2b). Adapter
specs and settings (D3). Real adapters (D16).

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

- `fe_dap::Session` (`crates/fe-dap/src/session.rs`) is the handle; its actor
  (`src/session/actor.rs`) runs one adapter per *generation* on the
  `fe-dap <adapter>` thread, and an emulated restart starts a new generation on
  the same handle. The protocol log is `src/log.rs`.
- `SessionState` has Launching, Initializing, Configuring, Running, Stopped,
  Restarting, Terminating and Ended; D2b adds its resume states. Here `stopped`
  moves to Stopped and `continued` to Running, nothing more.
- `configurationDone`'s timeout is the default 10 s; on expiry the adapter is
  killed and the session ends `Failed(SessionError::ConfigurationDoneTimeout)`.
- The fake gained one knob, `Startup::InitializedOnRelease`.
- Design calls: `docs/decisions/hrmp1j8rp8-fe-dap-session.md`.
