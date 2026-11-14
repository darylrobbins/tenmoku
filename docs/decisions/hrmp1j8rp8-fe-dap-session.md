# fe-dap session lifecycle: design calls (T646, debugger D2)

The calls card D2 made where `docs/decisions/r1b9dw3g0g-debugger.md` (§2, §13)
leaves room. The code is `crates/fe-dap/src/session.rs`,
`crates/fe-dap/src/session/actor.rs` and `crates/fe-dap/src/log.rs`.

## Startup

- **`configurationDone` waits for the configuration replies.** On each
  `initialized` the session sends the `setBreakpoints` per file and the
  conditional `setExceptionBreakpoints` together, and sends
  `configurationDone` only once each of them is answered, has failed or has
  timed out (VS Code's order; §2 item 3's "then"). Without
  `supportsConfigurationDoneRequest` that same point is configuration
  completion.
- **An empty `exceptionBreakpointFilters` counts as none**, so an adapter
  with `configurationDone` and `[]` gets no `setExceptionBreakpoints`.
- **Requests made before `initialize` is answered wait** for it, and go
  after `launch`. Requests made while shutting down fail with
  `RequestError::Ended`.
- **`Launching` is the state `Session::start` returns in**; only changes
  are sent to the sink, so the first `DapEvent::State` is `Initializing`
  (or an end). An emulated restart's relaunch does send `Launching`.
- **A `stopped` during configuration** moves to Stopped, and startup
  completion then leaves the state alone: only Initializing and Configuring
  become Running when the baseline goes.

## Timeouts and unresponsive adapters

- `configurationDone` uses the default 10 s (`CONFIGURATION_DONE_TIMEOUT`).
- **A timeout on `initialize`, `launch`/`attach` or `configurationDone`
  kills the adapter at once** and ends the session with the named
  `SessionError` (`InitializeTimeout`, `StartTimeout`,
  `ConfigurationDoneTimeout`): the adapter has shown it isn't answering, so
  no `disconnect` is tried first. An error *response* to `initialize` or
  `launch` comes from an adapter that answers, so it goes through
  `disconnect` and the 2 s kill instead.
- **A host request that times out also sends `cancel`** when the adapter
  supports it, as a dropped one does; either way nothing waits for its
  reply after.
- **Timers are handled before commands** in the actor's wait, so a
  timeout that has come due always lands before a command sent after the
  clock moved (tests rely on this to prove a `repl` evaluation has no
  timer).

## Restart and termination

- **Emulated restart keeps the handle.** A failed or unanswered `restart`
  (or an adapter without it) stops the adapter as the user's Stop does
  (`terminate`, `disconnect`, the 2 s kill) in state Terminating, then
  launches a new adapter from the same `SessionOptions` on the same
  `Session`, from `Launching`. A user Stop during that stop cancels the
  relaunch and the session ends `Stopped`.
- **Shutting down forgets every request in flight**: the host's fail with
  `Ended` at once, the session's own are dropped with their timers, so only
  `terminate`'s and `disconnect`'s completions count after (that is how a
  late `restart` completion changes nothing).
- **The adapter's output closing** without an exit arms the same 2 s kill
  and fails the requests in flight.
- `disconnect` always carries `terminateDebuggee: true`.

## The protocol log

- **Metadata lines** are `request|response|event seq=… command=…` with
  `request_seq`, `success`, `error.id` (failures only), `took=Nms`
  (responses), and the body's top-level key names as `[a, b]`; `launch`
  and `attach` arguments show only `<N keys>`.
- **stderr is drained always, logged only with payloads**, redacted; an
  adapter's stdout over TCP is drained and dropped.
- **Redaction** is one pass over each string, longest expanded value
  first, so a reference put back is never rewritten again; empty values are
  ignored. Every object under a key named `env`, at any depth, has its
  non-null values replaced by `<redacted>`. In payload mode it runs on the
  JSON values before they are written out, so escaping can't hide a value.
- The ring keeps 2000 entries, the payload warning always first.

## The fake

- One knob was added, `Startup::InitializedOnRelease`: `launch` answered at
  once and `initialized` held for `FakeAdapter::release`, for the 5 s
  missing-`initialized` rule and the late `initialized` after it.
- T643's carried F10 is fixed here, since this card changes `fake.rs`: the
  fake queues what it sends and drives its writer beside reading and the
  kill control, so a stalled write or flush never hides a kill, and a
  killed fake drops its writer without awaiting `close` (a normal end still
  writes the rest and closes, unless a kill comes first). Tests:
  `tests::fake::a_kill_ends_the_fake_while_its_write_stalls`,
  `…_while_its_close_stalls`, `a_killed_fake_never_closes_its_writer`.

## Review round 1 (T646)

- **Fair waiting (F1).** Timers are polled first (each fires once, so they
  can't starve anything, and a timeout that came due still lands before a
  later command); the other seven sources (adapter messages, commands,
  terminal replies, stderr, stdout, the writer, the exit) are polled in a
  rotating order, one further on each turn. On exit, up to 256 messages the
  adapter already sent are still read. Test: `tests::session_fairness`.
- **No adapter text in the application log (F2).** `log::*` records carry
  only names, numbers and error kinds; any free text kept for diagnosis
  (a failed baseline's error, a session error's message) goes through
  `ProtocolLog::detail`: payload mode only, redacted.
- **stderr is redacted as a byte stream (F3)** before it is split into
  lines, cut at 4096 bytes or decoded: a tail that may start an expanded
  value is held back until more comes or the stream ends, and only a final
  `\r` is dropped from a line. The state lives in `ProtocolLog`
  (`stderr`, `stderr_end`).
- **A writer failure is a transport failure (F4):** outside shutdown the
  session ends `Failed(Transport)` and the adapter is killed; while shutting
  down, it stops waiting for answers and arms the 2 s kill.
- **Late lifecycle replies (F5).** A new thread baseline gives up the one
  still in flight; a configuration reply from an earlier `initialized`
  round is ignored entirely (no `Breakpoints`, no failure shown).
- **Metadata keys (F6)** are logged only when the protocol defines them for
  that command or event (tables in `log.rs`, generated from
  `protocol.rs`); others are counted (`[category, output, +1]`), and a
  command or event the protocol doesn't define here shows `<N keys>`.
- **Each `initialized` round resets configuration completion (F7)**, so a
  `launch` response during a later round waits for that round.
- **The kill deadline never moves (F8):** output closing arms the 2 s kill
  only if it isn't armed already.
- **A supported restart retires the old run's exit code (F9).**
- The fake gained two controls for these tests: `FakeAdapter::send` (an
  injected event or reply) and `close_output`.

## Review round 2 (T646)

- **Draining after exit (F10, F12).** Once the adapter's process has exited,
  the session reads its output and stderr on, alternating between them,
  until each ends or `EXIT_DRAIN` (1 s on the session's clock) passes for
  streams that stay open; there is no message-count cut-off. A stderr tail
  that may start an expanded value and never completes shows as
  `<redacted>` rather than as itself.
- **No parse-error text (F11).** A body that isn't a protocol message ends
  the session with `SessionError::Transport` holding the catalogue's fixed
  `dap-session-bad-message`, never serde's account (which quotes the
  adapter's text escaped, past redaction). The payload log keeps no
  diagnostic free text of its own (`detail` is gone): the adapter's replies
  are there whole, redacted as values.
- **The exit code is retired when `restart` is sent (F9),** whatever comes
  of it: an `exited` after that belongs to the new run.
