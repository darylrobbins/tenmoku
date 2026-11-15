---
task_uid: "01m7e4x833ygaeq22ny9pbghn3"
task: "T654"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Test tests::session_resume::a_step_without_continued_runs_on_its_response passes (§11 D2 C2): with no continued event, a successful next moves to Running on its response, step commands are disabled from the send on, and a variables request with the old reference answers Stale without a request"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in tests::session_resume pass (§11 D2 C3, amended by F2): a step's response then stopped ends Stopped at a new epoch; a stopped before the step's response also ends Stopped and the late response changes nothing; a variables reply delayed past a step is dropped"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in tests::session_uncertain pass (F2, §13): a failed or cancelled next, continue or stepOut leaves the session Uncertain, not Stopped; it sends no scopes, variables or evaluate while Uncertain; it moves to Stopped only on a stopped event or a successful threads/stackTrace probe the adapter answers, and then fetches under a new epoch; a probe answered with a notStopped error moves to Running"
    required: true
    manual: false
  - id: "C4"
    text: "Tests in tests::session_uncertain pass (§11 D2 C4 with F2): a next failure delayed past a later continue changes nothing; a next failure delayed past Stop changes nothing and the session still ends; a next never answered moves to Uncertain after its timeout with Continue, Pause and Stop enabled and stepping disabled, and then its late success moves to Running, its late cancelled failure stays Uncertain until a stop is proven, and a stopped event moves to Stopped"
    required: true
    manual: false
  - id: "C5"
    text: "Tests in tests::session_pause pass (§11 D2 C5): Pause on a launch that runs at once with no thread events sends pause with a thread id from the startup threads baseline, with and without supportsConfigurationDoneRequest; with no threads known Pause stays disabled with the reason"
    required: true
    manual: false
  - id: "C6"
    text: "Test tests::session_terminate::late_events_after_stop_enable_nothing passes (F11, §13): after Stop, late stopped and continued events neither change the state nor re-enable any control"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-dap passes"
    required: true
    manual: false
---

# fe-dap stop and resume: stop epochs, lazy stop data, the resume guard and Uncertain, pause, and no control re-enabled after Stop (debugger D2b) (Mac T1842)

# fe-dap stop and resume: stop epochs, lazy stop data, the resume guard and Uncertain, pause, and no control re-enabled after Stop (debugger D2b)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

Stopped data in DAP (`variablesReference`, `frameId`) is only valid while execution
stays suspended, replies arrive late, adapters disagree on event order, and a step
may never be answered. The design review left two open findings here (§13):
**F2**, a failed or cancelled resume was treated as proof the program stayed
stopped, and **F11**, late events after Stop could re-enable controls.

## What to build (§2 items 4-5; §11 D2 C2-C5; §13 F2, F11)

On D2's session:

- **Stop epochs**: `stopped` increments the epoch; sending a resume (`continue`,
  `next`, `stepIn`, `stepOut`) increments it at once; every id the UI holds carries
  its epoch and a request with another epoch's id answers `Stale` locally.
- **Lazy stop data**: `threads`, `stackTrace` (20 frames, more on demand), `scopes`,
  `variables` for non-expensive scopes, children only on request; late replies under
  another epoch dropped.
- **The resume guard and Uncertain** (§2 item 4), and `continued` handling.
- **F2 (wins over §2 item 4's "a failure response returns to Stopped" and §11 D2
  C3/C4's "moves to Stopped, with fresh data"):** after a failed or cancelled step,
  continue or similar request the session stays **Uncertain until a stop is proven**
  — a `stopped` event, or a successful `threads`/`stackTrace` the adapter answers
  while stopped. It handles the adapter's `notStopped` error (the program is
  running) and **never fetches stack or variables while the debuggee may run**.
- **Pause** targeting and the startup baseline (§2 "Pause").
- **F11:** after Stop, late `stopped`/`continued` events never re-enable controls.
- A control-state query (which of Continue, Pause, steps, Restart, Stop are enabled
  and why) that D8's toolbar and commands read, so they hold no logic of their own.

## Not in this card

Startup, configuration, restart, termination and the log (D2). Workbench UI.

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

- The design calls (F2's probe, epochs and `StopRef`, the controls query, F11,
  Pause targeting, what isn't modelled) are in
  `docs/decisions/2ny9pbghn3-debugger-stop-resume.md`.
- Code: `crates/fe-dap/src/session.rs` (the public API: `Epoch`, `StopRef`,
  `StopData`, `Resume`, `Session::{resume, pause, controls, stop_data, frames,
  scopes, variables, evaluate}`, `RequestError::{Stale, NotStopped}`, the
  `Resuming` and `Uncertain` states), `src/session/controls.rs` (the control-state
  query) and `src/session/actor/stops.rs` (the epoch, the fetch on a stop, the
  resume guard, Uncertain's probe, Pause).
- The fake gained one knob, `FaultKind::Answer(body)` (a request answered with
  the test's body), for C5's "no threads known".
- Tests: `tests::session_resume` (C1, C2), `tests::session_uncertain` (C3, C4),
  `tests::session_pause` (C5), `tests::session_terminate::late_events_after_stop_enable_nothing`
  (C6). D2's tests that listed exact command sequences now include each stop's
  fetch (`threads`, `stackTrace`, `scopes`, `variables`).
- Review round 1: a probe's stack proves a stop only with
  `SessionOptions::stack_proves_stop` (debugpy answers with a running thread's
  stack) and only with frames; stop-data requests are bound to the stop they went
  in, typed and raw; suspended threads follow `allThreadsStopped` and
  `allThreadsContinued`; `threads` replies older than the list are dropped. New
  tests in `tests::session_threads` and `tests::session_controls`.
- Review round 2: stop-data requests are bound to the epoch the host asked at
  (on the handle, before queueing); `threads` replies are tied to the epoch they
  went at and checked before they change anything; a partial `continued` before a
  `continue`'s answer uses its snapshot; thread removal reconciles the snapshot
  and the stop's shown thread; the session's own lifecycle commands are refused as
  raw requests.
