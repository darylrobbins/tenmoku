---
task_uid: "01m55sbhrnr87gz30jhytxkna4"
task: "T166"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::stdout_stall::big_requests_and_replies_cross_without_a_stall passes: eight 256 KiB requests to a server that reads its input only while its output isn't blocked (rendezvous hand-off, UnixStream pair, own ServerLauncher) are all answered within WAIT; it fails at the base (checked on main 40608138 with the scratch copy: no answer in 10 s, while a decoupled server answers in 0.2 s)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test server::pump_tests::the_output_backlog_bound_is_an_error passes: with a 1 KiB bound and the reader unpolled, the pump stops reading past the bound, and the reader yields the queued bytes, then the backlog error, then nothing; the production bound is 64 MiB and is documented on the pump"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp's existing tests pass unchanged, tests::cancel and tests::lifecycle's per-poll write counts included (the pump counts against IoBudget)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep reading a language server's stdout while a write to it is blocked, so fe-lsp can't deadlock with a server that stops reading until its output drains (rust-analyzer's lsp-server) (Mac T921)

## Problem

fe-lsp can deadlock with a server that, like rust-analyzer's `lsp-server`, stops reading its input while
its output is blocked. Flagged as an open risk at T138 (memory note: "while parked in `feed`, async-lsp
doesn't read the server's stdout"), and again by the LSP gap audit (2026-10-17, main c7b6a04a: "stdout
stall risk still open ... I couldn't confirm whether it can trigger in practice").

**Confirmed at main 40608138** with a scratch test (copy beside this card: `stdout-stall-repro.rs`):

- The test server reads its input on one thread. It hands each message to its main thread over a
  rendezvous channel (`sync_channel(0)`, as lsp-server's `bounded(0)`), and the main thread writes each
  reply (256 KiB) to stdout before taking the next.
- The transport is a `UnixStream` pair through a custom `ServerLauncher`.
- Eight 256 KiB requests sent at once: no answer in 10 s ("no answer in 10s", FAIL at 10.02 s).
- Controls: the same with 1 KiB messages answers all 8 in 7 ms. The same with the server's reader
  decoupled (a deep channel) answers all 8 in 226 ms.

The mechanism is async-lsp 0.2.4's `MainLoop::run` (`src/lib.rs:533-580`). With the previous message's
write still pending, a new outgoing message makes it `outgoing.feed(msg).await`. `feed` waits for that
write to finish, and `incoming` isn't polled meanwhile. The server is blocked writing its stdout, so it
reads none of its stdin, so that write never finishes. fe-lsp runs the loop as
`join3(main_loop, actor, stderr)` (`crates/fe-lsp/src/server.rs:634-638`), with nothing else reading
stdout. Big messages are ordinary: a `didOpen` or full `didChange` of a large file, and large completion,
symbol or diagnostic replies.

## Change

`crates/fe-lsp/src/server.rs`: read the server's stdout continuously, whatever the main loop is doing.

- A fourth joined task (beside `main_loop`, the actor and stderr) pumps the transport's `output` into an
  in-process byte queue. The `AsyncRead` handed to `MainLoop::run_buffered` reads from that queue: it is
  `Pending` while the queue is empty, and is woken when bytes arrive.
- EOF and errors pass through in order, after the queued bytes, so crashes and exits are seen as today.
- The pump counts against the same `IoBudget` as the `Metered` reader and writer (`server.rs:1912-1950`),
  so one poll stays bounded (see the T138 notes in the `IoBudget` docs).
- **Bound.** While more than 64 MiB that the main loop hasn't read sits in the queue, the pump stops
  reading. The reader the main loop sees then yields an `io::Error` ("server output backlog over 64 MiB"),
  which ends the session through the existing transport-error path (crash accounting and restart, as a
  broken pipe would). This is documented on the pump.
- Nothing else changes: the order of messages, cancellation, the `QUEUED_MAX` window and shutdown.

Test: new `crates/fe-lsp/src/tests/stdout_stall.rs`, declared in `crates/fe-lsp/src/tests/mod.rs`. Build
it from `stdout-stall-repro.rs` without the scratch `PAD`/`DECOUPLED` env switches:
`big_requests_and_replies_cross_without_a_stall` sends eight 256 KiB requests to the coupled server (a
`UnixStream` pair with the OS's default buffers, through a `ServerLauncher` of its own, and a drop guard
that shuts the socket so a failing run ends) and expects all eight answered within `WAIT`. It uses only
public fe-lsp API and the `tests/mod.rs` helpers, so it compiles and fails at the base. The bound is
tested on the queue itself, with the bound a constructor parameter. That test goes in a `#[cfg(test)] mod
pump_tests` inline in `server.rs`, not in `stdout_stall.rs`: overlaid test files must compile against the
base, and the queue is new. `server::pump_tests::the_output_backlog_bound_is_an_error`: with a 1 KiB bound
and a reader that isn't polled, the pump stops past 1 KiB. The reader then yields the bytes queued, then
the backlog error, and no more.

Linux-OK: `UnixStream` pairs behave the same on Linux, whose default socket buffers are larger, and
256 KiB messages still exceed them.

## Implementation notes

- `Backlog` (server.rs) holds the queue; `Backlog::split(output, max, budget)` returns the loop's
  reader (`Drained`) and the pump future, joined last in `join4` so the loop takes the refilled budget
  first each poll. The pump reads at most 64 KiB at a time, and never more than a byte past `max`, so
  the queue holds at most `max + 1` bytes before the error.
- The pump ends when the loop's reader is dropped (the loop ended, or the actor ended the session),
  as well as at the output's end or error, so it never spins on a spent budget once the loop is gone.
- The backlog error reads "server output backlog over 64 MiB" (in bytes below 1 MiB, as in the test).
- Besides the criteria's tests, `pump_tests` checks that the end and errors follow the queued bytes,
  that the pump's reads spend the budget, and that dropping the reader stops the pump.
- C1 checked at the base too (this branch's test with the base's server.rs): "no answer in 10s".
- Review round 1 (F1): a loop waiting in `feed` never reads the backlog error from its reader, so on
  overflow it stayed blocked, and with a socket transport nothing exited. The pump now also flags the
  overflow to `Overflowed`, which the session's wrapper of the loop polls before the loop itself: it
  ends the loop with the backlog error at once, and the actor takes the crash path (kills the server,
  restarts). `tests::stdout_stall::an_output_backlog_past_its_bound_ends_the_session_while_a_write_waits`
  floods 66 MiB while the client's big requests wait on a full socket and expects a restart whose log
  names the backlog; with the wrapper's check disabled it times out.
