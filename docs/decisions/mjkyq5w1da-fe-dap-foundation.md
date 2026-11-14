# fe-dap foundation (debugger D1): design calls

Card T643 (Mac T1840), under `docs/decisions/r1b9dw3g0g-debugger.md` §2 and §13.
These are the calls the record left open, made while building `crates/fe-dap`.

## Layout

`src/{lib,transport,protocol,launcher,clock,fake,l10n}.rs`, `src/bin/fe-dap-fake.rs`,
`src/tests/{transport,protocol,fake,launcher}.rs`, `tests/l10n.rs`, as the spec
suggested. Later cards add `session*`, `registry*` and the like beside them.

## Transport

- `FrameReader::read_body` is cancel-safe (bytes read stay buffered), so a session
  actor can `select` on it.
- Besides the 64 MB body limit (`MAX_BODY`), the header section is capped at 8 KiB
  (`MAX_HEADER`), so a peer that never sends `\r\n\r\n` can't grow the buffer.
- A missing or non-numeric `Content-Length`, or a header line with no `:`, is a
  protocol error. A duplicate `Content-Length`: the last one counts.

## Protocol

- Objects are declared through one `object!` macro: required fields, then optional
  ones (each `Option`, skipped when `None`), camelCase in JSON, and a flattened
  `extra: Map`. Every type is `Default`, so a caller fills what it needs.
- Open string enumerations (`reason`, `category`, `context`, …) are `String`.
- `Request`/`Event` traits tie a command or event name to its types; the markers
  live in `protocol::{request, event, reverse}` so `Initialize` and `Initialized`,
  `Thread` and the `thread` event, never clash. `source` is `request::SourceRequest`.
- `next` and `stepOut` share `StepArguments`; `restartFrame` is typed too, since §2
  lists it among the resumes.
- `Capabilities::merge` applies a `capabilities` event.
- The fields the specification lets be any JSON value (a request's `arguments`,
  a response's or event's `body`, `Source.adapterData`, `terminated`'s `restart`,
  `output`'s `data`) keep a present `null` as `Some(Value::Null)`, so it is written
  back; typed arguments and bodies read a `null` as `{}`. Other optional fields are
  typed (a string, a number), where the specification allows no `null`, and read
  one as absent.

## Launcher

- `AdapterSpec` carries all of §3's fields now (`languages`, `types`,
  `launch_defaults`, `init_commands`), so D3's registry fills it without touching
  `launcher.rs`.
- `LocalLauncher` resolves a bare command itself on absolute `PATH` entries only
  (fe-lsp's rule: a `.` or empty entry would resolve in the possibly untrusted
  working directory). The host passes the login shell's `PATH` with `with_path`
  (fe-lsp's `LoginPath`), per §13's amendment; fe-dap doesn't depend on fe-lsp.
  Sharing that code is the follow-up §2 names.
- Over TCP the adapter's stdin is null and its stdout is handed on as
  `AdapterIo::stdout`; the spec needs at least one argument holding `${port}`, or
  the launch is refused as `InvalidInput`.
- **Known limit: the port.** The free port comes from binding `127.0.0.1:0` and
  releasing it, so another local program could take it before the adapter binds,
  and the launcher would connect to that program instead. Every TCP-mode DAP client
  has this race; closing it needs the adapter to report its own port (D21's
  server-provided port) or a listen-and-accept "reverse" mode (§2, later).
- A failed or cancelled connect kills the adapter's whole group, even when the
  adapter itself has already exited.
- The 5 s connect deadline is one sleep on the clock for the whole loop: it ends an
  attempt still in flight, no attempt starts after it has passed, and a connection
  completing in the same poll as the deadline (or a cancel) is refused: the deadline
  and the cancel are polled first, and the clock checked again before a connection
  is kept.
- While it waits for a TCP adapter to listen, the launcher drains the adapter's
  stdout and stderr (keeping up to 1 MiB of each, `STARTUP_OUTPUT_LIMIT`, and
  dropping the rest with a warning), so an adapter that writes before listening
  never blocks on a full pipe. The drain yields after every read, so a pipe that is
  always readable can't keep the other stream, the connection, the deadline or a
  cancel from being polled. The kept bytes come first on `AdapterIo::stdout` and
  `stderr`.
- Process groups are Unix only, as fe-dap's record says (§10). On Windows the
  adapter runs without a job object for now.

## The fake

- Every §2 knob maps to a `Script` field or a `Fault` (the table in `src/fake.rs`'s
  module docs). Faults pick one request by command and occurrence, so any request
  (not only resumes) can fail, go unanswered, be held or end the adapter, and a held
  reply goes on a `Trigger`: the next resume, a terminate or disconnect, a `cancel`
  naming it, or the test's `release()`.
- A held resume runs when it is let go (so a late success moves the program); any
  other held request keeps the reply it had when it came (so a delayed `variables`
  reply carries the old stop's data). What that reply would set off (other held
  replies' release, the exit after `disconnect`) is held with it; its effect on the
  program's state happens when the request comes.
- Variable references name places in the program (a function's locals, the
  globals, a variable's children), so `variables` reads, and `setVariable` changes,
  the program itself, and only the member the reference names.
- Once `terminate` or `disconnect` comes, a launch not yet answered fails and
  nothing (a late `configurationDone` or `runInTerminal` reply) starts the program.
- Without `supportsConfigurationDoneRequest`, configuration ends (and the program
  starts) on `setExceptionBreakpoints`, which the record makes the client always send
  to such an adapter.
- Beyond §2's list, for D2: `stop_on_entry`, spinning lines for Pause, `restart`
  re-sending `initialized`, `runInTerminal` and `startDebugging` reverse requests,
  `output` events, an exit code, and `lingers` (a mute adapter that outlives
  `disconnect` until killed).
- The `fe-dap-fake` binary requires `test-support`. The crate dev-depends on itself
  with that feature, so `cargo test`/`nextest` builds the binary, and the unit tests
  in `tests::launcher` find it beside their own executable (cargo sets
  `CARGO_BIN_EXE_*` only for integration tests).

## The app links fe-dap

`crates/fastedit/tests/l10n_catalogue.rs` requires every crate with an `l10n/`
directory to be linked into the app, so `fastedit` depends on `fe-dap` from this
card on, and the test lists it. Nothing in the app calls it yet; once the workbench
cards use it, it is reached through `fe-workbench` as well.
