---
task_uid: "01m4eq726fbacrpta8xy8zbzxn"
task: "T210"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::lsp_quit::quit_cancels_a_blocked_launch: with a host launcher that blocks until its LaunchCancel fires, opening a Rust file in a trusted root starts a launch; shut_down_all_within returns Quit { forced: 0, left: 0 }, the launcher saw the cancel, and the fake was never initialized"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::lsp_quit::quit_kills_what_outlasts_the_grace: with a fake that never answers shutdown and the workbench's ManualClock not advanced, shut_down_all_within with a short grace returns Quit { forced: 1, left: 0 } and the fake saw one kill"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::lsp_quit::a_launcher_ignoring_cancel_is_left: a launcher blocking on a channel and ignoring the cancel makes shut_down_all_within return after grace plus force with Quit { forced: 1, left: 1 } instead of hanging; released afterwards, what it launched is killed and never initialized"
    required: true
    manual: false
  - id: "C4"
    text: "The quit hook calls shut_down_all_within(cx, SHUT_DOWN_WITHIN, KILL_WITHIN) and warns when left \u003e 0; the set_server_launcher doc and the module doc state the host contract and no longer describe it as an accepted limit waiting on a follow-up"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::lsp_quit::quit_kills_a_server_awaiting_initialize: with a fake that never answers initialize (and, separately, one that answers after longer than the grace), quitting with an open window and quitting after the last window has closed both return with the process killed and Quit { left: 0 }"
    required: true
    manual: false
---

# Force-stop every language server at quit once the graceful wait runs out

# Force-stop every language server at quit once the graceful wait runs out

## Problem

T143's quit hook (`crates/fe-workbench/src/lsp.rs`, `init`'s `cx.on_app_quit` → `shut_down_all`)
drops every workbench's `LspManager` and blocks until each `Watch` hears its servers stopped, or
until `SHUT_DOWN_WITHIN` (6 s: `LOGIN_SHELL_TIMEOUT` plus fe-lsp's §3.4 sequence plus margin). Past
that it logs "quitting with language servers still shutting down" and lets termination go ahead.
Codex review F1 of T143 (2026-10-08) found that a host launcher blocking in `launch` past the
deadline is never stopped. The user accepted this as a known limit in T143, with the host contract
on `set_server_launcher`, and approved this follow-up on 2026-10-08. The previous card gives fe-lsp
`LaunchCancel` (stops cancel launches in progress) and `LspManager::kill_switch()`.

## Change

All in `crates/fe-workbench/src/lsp.rs`:

- **Keep the kill switch.** When the workbench makes its manager, it takes
  `LspManager::kill_switch()` into that manager's `Watch`, so the switch survives the manager's drop
  in `retire_lsp`.
- **Bounded deadlines.** `shut_down_all(cx)` calls a new
  `shut_down_all_within(cx, grace: Duration, force: Duration) -> Quit` with `SHUT_DOWN_WITHIN` and a
  new `KILL_WITHIN = 1 s`. Tests call it with short values.
- **What `shut_down_all_within` does:**
  1. It retires every workbench's manager, as now. With fe-lsp's change, that stop also cancels
     launches in progress.
  2. It waits until `grace`, as `shut_down_all` does now.
  3. For each watch not yet done, it calls `KillSwitch::kill()`.
  4. It waits up to `force` more for those servers.
- **`Quit { forced: usize, left: usize }`** counts:
  - `forced`: servers still running at `grace`, which were killed
  - `left`: servers still running at `grace + force`

  The hook logs a warning when `left > 0`: a launcher that ignored cancellation, whose server may
  outlive fastedit.
- **Docs.** In the module doc and `set_server_launcher`'s doc, replace "fe-lsp can't cancel a
  launch … a limit accepted on 2026-10-08 until fe-lsp can cancel launches (a follow-up card)"
  with the contract:
  - stopping or quitting cancels a launch through its `LaunchCancel`, and quitting force-stops
    servers that outlast the graceful wait;
  - a launcher must honour the cancel or bound its own launch time;
  - one that does neither can't be preempted, and its server may outlive fastedit.

### Tests

New file `crates/fe-workbench/src/tests/lsp_quit.rs`, declared in `tests/mod.rs`. It runs headless
GPUI over a trusted `tempfile` root, with a host launcher set through `set_server_launcher` that
wraps fe-lsp's `FakeLauncher` (`test-support`). It calls `shut_down_all_within` with
`grace`/`force` of a few hundred ms, so no test waits the real 6 s.

## Out of scope

- Changing `SHUT_DOWN_WITHIN`.
- fe-lsp itself (the previous card).
- Window close, which stays graceful: only quitting force-stops.
