---
task_uid: "01m6p6509t9fdzhkypd70qqf9d"
task: "T453"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-plugin regression test a_fetch_slower_than_the_call_deadline_is_charged_to_the_program_share_not_the_call (new file tests/net_deadline.rs, its own test binary, base API only, plain assert!/assert_eq! with no custom message): with host.deadline set 100 ms out, program_runs armed at 10 s and a fake backend that sleeps 300 ms, Host::fetch returns Ok, program_runs.spent() \u003e= 300 ms and host.deadline moved back by at least 300 ms. It fails at base on those assertions"
    required: true
    manual: false
  - id: "C2"
    text: "In net.rs, HostState's fetch measures its wall time, charges it with ProgramBudget::charge and pushes HostState::deadline back by the charged amount with checked_add, on error returns too. State::fetch_within(request, Option\u003cDuration\u003e) caps the backend timeout at min(request timeout or DEFAULT_TIMEOUT, program_runs.remaining()) while a call is armed. State::fetch keeps its signature, existing tests/net.rs passes unchanged, and outside an armed call nothing is capped or charged"
    required: true
    manual: false
  - id: "C3"
    text: "A second test in tests/net_deadline.rs: with program_runs armed at 50 ms, the Outgoing.timeout the backend receives is at most 50 ms; not armed, it is the plugin's own timeout"
    required: true
    manual: false
  - id: "C4"
    text: "net.rs module docs and the limits.rs CALL_DEADLINE and PROGRAM_DEADLINE docs state that fetches count with program runs against the 10 s share and not against the 100 ms call deadline"
    required: true
    manual: false
  - id: "C5"
    text: "process.rs, grants.rs, lib.rs, runner.rs and crates/fe-workbench are untouched (T1383 owns the first four), and cargo clippy --workspace --all-targets -- -D warnings is clean with the kiln-plugin and fe-workbench tests passing"
    required: true
    manual: false
---

# Give a plugin's net.fetch the same footing as a program run: its wall time moves the call's 100 ms deadline back and counts against the call's 10 s program share, with each fetch's timeout capped by what is left of it (follow-up of T1382) (Mac T1385)

## Problem

`kiln_plugin::net` `fetch` is a blocking import (kiln/crates/kiln-plugin/src/net.rs:123 `State::fetch`, reached from
`impl Host for HostState` at :211) that waits on the backend for up to `DEFAULT_TIMEOUT` (30 s, :36) or the plugin's
`timeout_ms`. Unlike `process.spawn` since T1382 (process.rs `impl Host for HostState`, ~:302-318), it is not charged
to the program budget and does not move `HostState::deadline` back. The call's 100 ms deadline keeps running while the
host waits, so a granted fetch slower than 100 ms fails the call on the same checks T1382 fixed: the epoch callback,
the pending-import poll check and the after-call `overran` check in `Guest::guarded` (runner.rs ~:510-556;
fe-workbench plugin_host.rs ~:2043-2128). Any real network request (typically well over 100 ms) therefore makes the
call fail with "a call ran past its 100ms deadline" and unloads the plugin.

T1382's spec left this as a named follow-up ("net.fetch ... is also a blocking import charged to the call deadline").

## Change

Make `fetch` work like a program run: its wall time is not charged to the call, and it shares the call's
`PROGRAM_DEADLINE` budget.

1. In net.rs, `impl Host for HostState`'s `fetch` measures the wall time of `self.net.fetch(..)`, calls
   `self.program_runs.charge(taken)` and pushes `self.deadline` back by the charged amount with `checked_add`, as
   process.rs's `spawn` does. Do this on error returns too.
2. Cap each fetch by what is left of the call's share: the request's timeout is the smaller of its own (or
   `DEFAULT_TIMEOUT`) and `self.program_runs.remaining()` when a call is armed (a share already used up fails the
   fetch at once with `Error::Failed("... timed out")`). Add a `State::fetch_within(request, share: Option<Duration>)`
   for it and keep `State::fetch` as the no-share form (tests/net.rs unchanged). Outside a call (not armed) nothing is
   capped or charged beyond today's behaviour.
3. Do not touch process.rs, grants.rs, lib.rs or runner.rs (T1383 owns them); use `ProgramBudget::charge` and
   `remaining` as they are.
4. Docs: the net.rs module docs and limits.rs `CALL_DEADLINE` / `PROGRAM_DEADLINE` docs say fetches count with
   program runs against the 10 s share and not against the 100 ms call deadline.

## Tests

- Regression, new file kiln/crates/kiln-plugin/tests/net_deadline.rs (a new integration binary: kiln-plugin's tests/
  files are flat, auto-discovered, there is no tests/mod.rs), base API only: build `HostState::new`, set
  `host.net = net::State::new(&grants, &Limits::default(), fake)` where the fake backend sleeps 300 ms then returns
  200; set `host.deadline = Some(now + 100ms)` and `host.program_runs.arm(Duration::from_secs(10))`; call
  `kiln::extension::net::Host::fetch`; assert the reply is Ok, `host.program_runs.spent() >= 300ms`, and
  `host.deadline` is at least 300 ms later than it was set. Use plain `assert!`/`assert_eq!` without a custom message.
  It fails at base on those assertions (the fetch is not charged).
- A second test in the same file: with `program_runs.arm(50ms)` and a fake that records `Outgoing::timeout`, the
  backend is sent a timeout of at most 50 ms (the cap); not armed, it is sent the plugin's own timeout.
- No test needs a network or a wasm guest; it runs on Linux and macOS.

## Out of scope

fe-net's real backend, the fe-workbench loader, and the pre-launch/grant work of T1383.
