---
task_uid: "01m4eq6tfe336hjrkh11ws5xxq"
task: "T209"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::kill::stop_cancels_a_blocked_launch: with a launcher that blocks until its LaunchCancel fires, LspManager::stop, and likewise dropping the manager, makes launch return, the sink hears Stopped(None) rather than a launch error, and the fake records no initialize"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::kill::kill_switch_skips_the_graceful_shutdown: with a fake that never answers shutdown and its ManualClock not advanced, dropping the manager and calling KillSwitch::kill kills the process (fake.kills() == 1) and the sink hears Stopped(None)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::kill::kill_switch_ends_launches_and_restarts: after KillSwitch::kill, a server waiting out its restart backoff and one blocked in a cooperating launch both end Stopped(None) with fake.launches() unchanged, and a start made after the switch is thrown launches nothing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::kill::a_launch_ignoring_cancel_is_killed_when_it_returns: a launcher that ignores the cancel and returns Ok after KillSwitch::kill has its process killed with no initialize sent, and the sink hears Stopped(None)"
    required: true
    manual: false
  - id: "C5"
    text: "ServerLauncher::launch takes \u0026LaunchCancel; LocalLauncher returns ErrorKind::Interrupted without spawning when cancelled before the spawn (fe-lsp test tests::kill::local_launcher_honours_a_cancel); the trait doc and docs/decisions/lsp-client.md §3.1 state the host contract (honour the cancel or bound the launch time; fe-lsp can't preempt a launcher that ignores it); every ServerLauncher implementor in the workspace builds and the existing fe-lsp and fe-workbench suites pass"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test tests::kill::stop_during_initialize_is_prompt: with a fake that never answers initialize and its ManualClock not advanced, LspManager::stop, dropping the manager, and KillSwitch::kill each end the server without waiting out the 30 s initialize timeout (no shutdown is sent to an uninitialized server; it is killed), and the sink hears Stopped(None)"
    required: true
    manual: false
---

# Cancel language-server launches in progress and force-stop a manager's servers: LaunchCancel on ServerLauncher::launch and LspManager::kill_switch

# Cancel language-server launches in progress, and force-stop a manager's servers, from fe-lsp

## Problem

Codex review F1 of T143 (2026-10-08): a host `ServerLauncher::launch` that blocks past the workbench
quit hook's deadline (starting a container, say) lets termination go ahead with no forced stop, and
its server may outlive fastedit. fe-lsp can't do better today:

- `ServerLauncher::launch(&self, spec, root)` (`crates/fe-lsp/src/launcher.rs:112-116`) "may block"
  and takes no cancellation.
- The server thread (`crates/fe-lsp/src/server.rs`, `serve`, around lines 350-375) checks
  `carry.stop` only before and after `launch` returns.
- `LspManager` (`crates/fe-lsp/src/manager.rs`) has `stop`, `restart` and `Drop`, all graceful
  (§3.4: `shutdown` 2 s, `exit` 1 s, then `kill`). Nothing cancels a launch or kills a live
  server at once, and nothing survives the manager's drop, which is when the quit hook waits.

The user accepted that as a known limit in T143 (quit deadline raised to 6 s, host contract
documented on `fe_workbench::set_server_launcher`) and approved this follow-up on 2026-10-08:
a cancellation token passed to `ServerLauncher::launch` (a breaking trait change for hosts) and an
`LspManager` API to cancel launches in progress and kill live server processes. The workbench quit
hook uses it in the next card. A launcher that ignores cancellation still can't be preempted, so the
documented host contract stays.

## Change

### `crates/fe-lsp/src/launcher.rs`

- New `LaunchCancel` (`Clone + Send + Sync`, exported from `lib.rs`):
  - `LaunchCancel::new()` and `cancel(&self)`: fe-lsp makes and fires it; public so a host's tests
    can too.
  - `is_cancelled(&self) -> bool`.
  - `cancelled(&self) -> impl Future<Output = ()> + Send + 'static`: resolves once cancelled (at
    once if already). A blocking launcher waits on it beside its own work with
    `async_io::block_on(select(..))`.
- The trait becomes
  `fn launch(&self, spec: &ServerSpec, root: &Path, cancel: &LaunchCancel) -> io::Result<ServerIo>`.
  Its doc states the contract: a launch should return promptly once `cancel` fires, with
  `ErrorKind::Interrupted` or with what it started (fe-lsp kills that). fe-lsp can't preempt a
  launcher that ignores it, so such a launcher must bound its own launch time.
- `LocalLauncher` and `WithPath` (`launch_local`) check `is_cancelled()` after resolving the
  command and before spawning, and return `Interrupted` without spawning. The login-shell capture
  stays bounded by `LOGIN_SHELL_TIMEOUT` and isn't interrupted.

### `crates/fe-lsp/src/server.rs`, `crates/fe-lsp/src/restart.rs`

- Each server instance gets one `LaunchCancel`, passed to every `launch` it makes, restarts
  included.
- A launch that returns after its cancel fired is a stop, not a launch error: an `Ok` gets its
  process killed (the existing "stop that came while it launched" path), an `Err` is dropped, and
  the sink hears `Stopped(None)` either way.
- The thread also waits on its manager's kill switch (below): `serve`'s `after` wait, the session's
  `select_biased!` in `serve`, both waits in `shut_down`, and `Carry::wait_out`'s restart backoff.
  When it's thrown, the thread kills the process (`kill`, which waits for `exited`) at once, without
  `shutdown`/`exit` or a further wait, launches and restarts nothing more, hands its documents over
  as a stop does, and sends `Stopped(None)`.

### `crates/fe-lsp/src/manager.rs`

- `Handle` keeps its instance's `LaunchCancel`. `Handle::stop` (so `stop`, `restart`'s stop of the
  old instance, settings shutdowns and `Drop`) fires it as it sends the stop: a stop already wins
  once the launch returns, and this lets a cooperating launcher return early.
- `LspManager::kill_switch(&self) -> KillSwitch`. `KillSwitch` is `Clone + Send + Sync`, exported
  from `lib.rs`, and outlives the manager: it's an `Arc` shared with every server thread the
  manager starts.
  - `KillSwitch::kill(&self)`: for every instance the manager started whose thread hasn't ended
    (running, launching, restarting, or still shutting down after a stop or the manager's drop), it
    fires its `LaunchCancel` and makes its thread force-stop as above. It returns at once, without
    waiting.
  - Once thrown it stays thrown: an instance the manager starts afterwards launches nothing and
    sends `Stopped(None)`.

### Callers and docs

- Every `ServerLauncher` implementor takes the new parameter:
  - `FakeLauncher` (`tests/fake.rs`)
  - test launchers in `tests/launcher.rs`, `tests/lifecycle.rs`, `tests/status.rs` and
    `tests/registry.rs` (`MissingOrFake`, `HeldLaunch`, `FailsFirst` and the direct `launch` calls)
  - T143's `Switch` in `crates/fe-workbench/src/tests/lsp.rs`

  These are signature-only edits, with no behaviour change.
- `docs/decisions/lsp-client.md` §3.1 shows the new signature, `LaunchCancel` and `KillSwitch`, and
  the host contract.

### Tests

New file `crates/fe-lsp/src/tests/kill.rs`, declared in `tests/mod.rs`, using `FakeServer` with a
`ManualClock` so nothing waits on real time. Its launchers wrap the fake's: one blocks until
`cancel.cancelled()` resolves, one blocks on a channel and ignores the cancel.

## Out of scope

- The workbench quit hook (the next card, which depends on this one).
- Interrupting the login-shell capture.
- Any new `ServerStatus` or event.

## Implementation notes

- `LaunchCancel` is two one-way flags (`launcher::Flag`, an `AtomicBool` plus a
  oneshot whose sender drops when it fires, shared): the instance's own, which
  `Handle::stop` fires, and the manager's kill switch, which every instance's
  cancel is linked to. So `KillSwitch::kill` fires one flag and needs no list
  of live instances: each thread sees it through `Carry::kill` (a fused wait
  plus a sync `thrown()`), and each launch through its `LaunchCancel`.
- C6 reverses the old rule that a stop while starting waits for the
  `initialize` answer: a stop, the manager's drop (stop, or the command
  channel ending) and the kill switch now end `initialize` at once and kill
  the process (`End::Kill`). `Actor::stopping` is gone with it. The lifecycle
  test that pinned the old rule, `a_stop_while_starting_shuts_down_ahead_of_queued_requests`
  (shutdown sent after the delayed answer, no kill), is now
  `a_stop_while_starting_kills_it_ahead_of_queued_requests` (killed with no
  clock time passing, no `shutdown`, none of the backlog sent, queued requests
  `ServerDown`). §3.4 of the decision doc says so.
- A thrown switch inside the graceful shutdown: `shut_down` checks `thrown()`
  first, then races the switch against the `shutdown` reply and the post-`exit`
  wait, killing at once when it fires. In the session loop it maps to the
  ordinary stop, whose `shut_down` then kills.
- Review round 1 (9v9pm7): F1, the post-`exit` wait now takes the switch ahead
  of the process's exit. F2, `drain_stderr`'s post-session reads stop once the
  switch is thrown, and `Server::session` hands the session's process back
  (`Watched`, which notes whether it was killed), so `Server::serve` kills it
  (`reap`) if the switch is thrown before the thread ends or launches again and
  nothing killed it yet. Tests: `kill_switch_wins_over_an_exit_it_comes_with`,
  `kill_switch_ends_a_crash_cleanup`, `kill_switch_reaps_after_a_graceful_exit`.
- Review round 2 (rb70zh): F3, `Watched` now holds the switch and reaps on
  drop, and `Server::serve` keeps the last session's process (`last`) until
  the thread ends, so a switch thrown from any status callback (the
  post-backoff `Starting`, a give-up's `Stopped(Some)`) still kills what's left.
  Tests: `kill_switch_thrown_as_a_recycled_server_restarts_reaps_it`,
  `kill_switch_thrown_as_a_recycled_server_gives_up_reaps_it`. F4: the
  simultaneous-exit test now asserts the server thread is still held in the
  stepped `exit` write after both the exit and the switch (a stepped write
  blocks the thread the actor shares, so the actor can't see either first).
- Review round 3 (hvqp4c) F5, and the human rejection st9hgm: the last
  session's `Watched` is dropped (`last = None`) just before the replacement is
  launched, so its guard covers the backoff and status callbacks but not the
  replacement session, when the old group id may have been reused. Test:
  `kill_switch_spares_a_replaced_sessions_process` (graceful recycle, a running
  replacement, then a kill: one kill, the replacement's).
