---
task_uid: "01m4srcfywwsaevkg49k68kpzj"
task: "T79"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "A recycle retires the session's identities and reports DiagnosticsCleared while shut_down is still waiting on a still clock (tests::teardown_clears::a_recycle_clears_before_the_shutdown_wait)"
    required: true
    manual: false
  - id: "C2"
    text: "A crash found by EOF while the process runs retires the identities and reports DiagnosticsCleared while its kill is held (tests::teardown_clears::a_crash_clears_before_the_kill_ends)"
    required: true
    manual: false
  - id: "C3"
    text: "No identity is reported cleared twice, and every existing fe-lsp test passes unchanged"
    required: true
    manual: false
---

# Retire a crashed or recycled language server's identities and clear its diagnostics before waiting for the process (T145 review F6) (Mac T619)

Filed on argon from Mac T619 (card ~/coordination/argon-cards/T619.json rev 1). Mac task uid 01m4srbs7zv3gw21kcywcmrfm4. No dependencies (a review follow-up of Mac T145, F6). Title the host commit "... (Mac T619)".

## Problem

Finding F6 from the codex review of T145 (approved, follow-up, never filed). It is still on main at c65979ea.

In `crates/fe-lsp/src/server.rs`, `Actor::run` retires a crashed or recycled session's identities only after
it has waited for the old process:

```rust
End::Crash(why) => {
    self.pending = FuturesUnordered::new();
    kill(process).await;          // waits for the process to go
    self.crashed(why, at)         // end_session() is called in here
}
End::Recycle(why) => {
    self.pending = FuturesUnordered::new();
    self.shut_down(process).await; // shutdown (up to 2 s), exit, up to 1 s, then kill
    self.crashed(why, at)
}
```

Until `end_session` runs (`identities.end_session`, then a `DiagnosticsCleared` for each open document), the
old session's identities stay active. Diagnostics events already queued for the host still pass the
workbench's check (`identities.active(buffer, server)`) and are applied. So a dead or recycled server's
diagnostics stay on screen for up to 3 s during a recycle, and for as long as a killed process takes to go.
`Actor::shutdown` (the stop and kill paths) already calls `end_session` before it waits.

## Change

- In `Actor::run`, call `self.end_session()` in the `End::Crash` and `End::Recycle` arms before
  `kill(process).await` and `self.shut_down(process).await`. `crashed` keeps its own call. That second
  call must do nothing: `Document::end_session` already returns `None` once a document has ended, so the
  same document is never reported cleared twice. Check that `Identities::end_session` on a retired session
  changes nothing.
- Write the tests in a new file, `crates/fe-lsp/src/tests/teardown_clears.rs`, declared in
  `crates/fe-lsp/src/tests/mod.rs` (a test module at base). Build on `super::Clocked` (a still
  `ManualClock`) and `super::cancel::sent_definitions`, as `tests/kill.rs`'s `recycling` does:
  - Recycle: use a script whose `textDocument/definition` is never answered and with
    `answer_shutdown: false`. Open a synced document, then abandon more than `ORPHANS_MAX` definitions.
    Before the clock is advanced (so `shut_down` is still waiting on its 2 s timeout), the document's
    `DiagnosticsCleared` arrives, and `manager.active_identity(buffer, server)` is no longer the old identity.
    Then advance the clock: the session restarts, and no second `DiagnosticsCleared` comes for the old
    identity.
  - Crash found while the process is still running: add `FakeServer::close_output()` to
    `crates/fe-lsp/src/tests/fake.rs`. It closes the latest launch's transport (the client reads EOF)
    without exiting it, so the actor ends with `End::Crash` and kills it. With `fake.hold_kills(true)`, the
    `DiagnosticsCleared` and the retired identity come while the kill is held. Then `hold_kills(false)`
    lets the restart go on.
