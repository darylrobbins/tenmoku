# Debugger: a Debug Adapter Protocol client, adapters, launch configs, UI and cards

Status: **approved with amendments by the user on 2026-11-13** (§13 records the
ruling and the amendments; §13 wins where it and §1-§12 disagree).
Researched 2026-11-06 against fastedit `5262609`; amended against `25ebfe614`.
Source card T486 on helium (lent from Mac T1415). Implementation cards are filed
from §11 as amended by §13.

Tenmoku has no debugger. VS Code users and LazyVim users (with the `dap.core`
extra) both expect breakpoints, stepping, a call stack and variables. This record
works out how a Debug Adapter Protocol (DAP) client fits Tenmoku, the way
`docs/decisions/lsp-client.md` did for LSP, and splits the work into cards. It
writes no code.

**Paths.** Bare `crates/…` paths are this repo at `5262609`. `async-lsp/src/…` and
`dap/src/…` are inside those crates' tarballs at the versions named in §1.

## 1. What was read and spiked

**The protocol.** The DAP specification and its changelog, from
`microsoft/debug-adapter-protocol` (pushed 2026-11-03; current version **1.71**).
Its documentation is CC BY 3.0 US and its code and schema are MIT
(`License.txt`, `License-code.txt`). Reading a specification is fine under
`CLEAN_ROOM.md`.

**DAP crates**, from crates.io (metadata through `cargo info` and the crates.io API
on 2026-11-06, sources from the downloaded tarballs):

| Crate | Version, date | Licence | What it is |
|---|---|---|---|
| `dap` (sztomi/dap-rs) | 0.4.1-alpha1, 2023-09-19 | MIT OR Apache-2.0 | A framework for writing *adapters*. Client-side derives (`Serialize` on requests, `Deserialize` on responses and events) sit behind a `client` feature. Its reverse requests are `runInTerminal` only, with no `startDebugging` (`dap/src/reverse_requests.rs:54`). Alpha, and no release in three years. |
| `emmy_dap_types` | 0.2.0, 2025-11-18 | MIT | A fork of `dap` with "cross-editor compatibility" changes, for one Lua debugger. |
| `dap-types` (jDomantas) | 0.0.1, 2024-05-22 | MIT OR Apache-2.0 | Types generated from the schema, at protocol **1.65**. One release. |
| `debug-adapter-protocol` | 0.1.0, 2023-01-29 | MIT | Typed builders over `typed-builder` 0.12. One release. |
| `debugserver-types` | 0.5.0, 2019-07-07 | MIT | VS Code's old debug-server types, from 2019. |

Every licence is in `deny.toml`'s allowlist. None of them is maintained against 1.71.

**async-lsp 0.2.4** (already in our lock, `docs/decisions/lsp-client.md` §2.1):
`src/lib.rs`, for its framing. `Message::read` and `Message::write`
(`async-lsp/src/lib.rs:418`, `:452`) are private. Every message is a
`RawMessage { jsonrpc, #[serde(flatten)] inner }` (`:310`), and `inner` is a
JSON-RPC `Request | Response | Notification` with an `id` (`:333`).

**This repo:**

- `crates/fe-lsp`: `lib.rs`, `launcher.rs`, `server.rs`, `router.rs`,
  `requests.rs`, `manager.rs`, `registry.rs`, `restart.rs`, `clock.rs`, and the
  layout of `src/tests/` (`fake.rs`).
- `docs/decisions/lsp-client.md`, sections 1 and 3.
- `docs/decisions/vmtnmkay62-extensions-and-plugins.md`, all of it.
- `docs/decisions/c1pt00h80c-grammar-fetch.md` and `pp118n3cn5-grammar-prompt.md`.
- `docs/release/macos-signing.md` and `crates/fastedit/macos/Tenmoku.entitlements`.
- `crates/fe-workbench/src/{task_defs,task_commands,task_run,task_problems,trust,lsp_consent,dock,terminal_panel}.rs`.
- `crates/fe-editor/src/decorations.rs`.
- `crates/fe-gpui/src/{gutter_annotations,lens,inlay_hints}.rs` and `view.rs`
  (`GutterGlyph`, `set_gutter_glyph`).
- `crates/fe-config/src/settings.rs` (`LspSettings` and `narrow_lsp`) and `paths.rs`.
- `crates/fastedit/presets/lazyvim.vimrc`.
- `crates/fe-keymap/keymaps/fastedit.toml` and `kiln/crates/kiln/src/setup/keymap.toml`,
  to see which F-keys are taken.
- `scripts/check-no-gpui.sh`, `deny.toml` and `CLEAN_ROOM.md`.

**LazyVim** at `c10948c5` (2026-06-02, Apache-2.0), the local install:
`lua/lazyvim/plugins/extras/dap/core.lua`, `extras/test/core.lua`,
`extras/lang/python.lua` and `extras/lang/rust.lua`. I read their key tables only,
as the existing preset already does.

**Adapters**, as repository metadata and release listings (GitHub API, 2026-11-06):

| Adapter | Release | Licence |
|---|---|---|
| lldb-dap | LLVM 23.1.5 (2026-11-03); Ubuntu candidate `lldb` 1:21.1.6 | Apache-2.0 WITH LLVM-exception |
| codelldb | v1.12.3 (2026-08-23). Per-platform `.vsix` files for darwin-arm64/x64 and linux-x64/arm64/armhf | MIT, bundling LLDB (Apache-2.0 WITH LLVM-exception) |
| debugpy | 1.8.22 on PyPI. A `py2.py3-none-any` wheel, SHA-256 `a9e9d355…913947` | MIT. It vendors pydevd, which is EPL-1.0: this matters only if Tenmoku redistributes it (§3) |
| vscode-js-debug | v1.140.0 (2026-09-23). `js-debug-dap-v1.140.0.tar.gz` | MIT |
| delve | v1.27.2 (2026-09-09). Per-platform tarballs, with `checksums.txt` and its signature | MIT |

**Not read.** No editor's DAP client code was opened: not Zed's `dap`,
`debugger_ui` or `project` (GPL; `CLEAN_ROOM.md`), and not nvim-dap, Helix or VS
Code's debug UI. Nothing in this design comes from them. LazyVim contributed key
names only.

**Spike.** I downloaded the debugpy 1.8.22 wheel into a scratch directory outside
the repo and checked its SHA-256. A ~130-line Python DAP probe was written to drive
it through `initialize`, `launch`, `setBreakpoints`, `configurationDone`, one stop,
`stackTrace`, `scopes`, `variables`, `evaluate`, `next` and `continue`. **It was not
run.** This session's permission policy refused to run downloaded code, so the
message-order claims below come from the specification, not from observing an
adapter. Card D16's real-adapter tests are the first place they are checked
against debugpy and an LLDB adapter.

**One finding about this machine.** On helium, `/proc/sys/kernel/yama/ptrace_scope`
is `2`, which is admin-only ptrace. Without `CAP_SYS_PTRACE`, no native debugger
(lldb-dap, codelldb, delve) can debug anything here, even a program it launches.
debugpy traces in-process and is unaffected. See §10.

**Decision.** Use no DAP crate (§2). Every adapter is an external process found or
installed apart from Tenmoku (§3). No GPL editor code was read.

## 2. Client architecture

**A new crate, `fe-dap`, with no GPUI.** `scripts/check-no-gpui.sh` treats every
workspace member as core unless it is listed in `UI_CRATES`, so `fe-dap` is checked
as soon as it joins the workspace. There is nothing to add to the script, but card
D1 confirms that `ok: fe-dap` appears in the output.

**Runtime: the same shape as fe-lsp.** Each session gets one thread named
`fe-dap <adapter>`. It runs an actor under `async_io::block_on`, with the adapter
process from `async-process`: no tokio, as in `lsp-client.md` §2.2. Results leave
the crate in two ways:

- request futures (`futures` oneshots) that are `Send` and can be awaited anywhere;
- an `FnMut(DapEvent) + Send` sink, called on the session thread.

Timeouts are measured on an injected clock. Like fe-lsp, no public item names a
dependency's type.

**Transport: our own framing, not async-lsp.** DAP borrows LSP's `Content-Length`
header framing but not its envelope. A DAP message is
`{seq, type: "request"|"response"|"event", …}`:

- a request has a `command`;
- a response has `request_seq`, `success`, `message` and `body`;
- an event has `event`.

There is no `jsonrpc` field and no `id`. async-lsp can't parse this, and its codec
is private (§1). So `fe-dap::transport` is about 150 lines:

- **Reading:** header lines to `\r\n\r\n`, `Content-Length` (case-insensitive),
  other headers ignored, then the body.
- **Writing:** one `write_all` and a flush per message.
- **Limit:** a body over 64 MB fails the session with a protocol error, not an
  allocation.
- **Connections:** it runs over stdio, or over a TCP connection to `127.0.0.1`
  (§3). The listen-and-accept mode used by a "reverse" adapter is later.

**Launching: `AdapterLauncher`, mirroring `ServerLauncher`.**
`fe-lsp`'s `ServerLauncher::launch` takes a `ServerSpec`, which is LSP-specific
(readiness, pull diagnostics; `launcher.rs:24`). DAP also needs a TCP mode, so
`fe-dap` gets its own trait:

```rust
fn launch(&self, spec: &AdapterSpec, cwd: &Path, cancel: &LaunchCancel)
    -> io::Result<AdapterIo>
```

`AdapterIo` carries the streams, stderr, and a process handle with
`exited()`/`kill()`, as `ServerIo` does. `LocalLauncher` spawns the adapter in its
own process group (fe-lsp's approach, written again; it is our code). For a TCP
adapter it picks a free port, substitutes it for `${port}` in the arguments, and
connects with retries for up to 5 s. A host can replace the launcher to sandbox or
refuse adapters, as with servers.

Pulling the process-group code out into a crate shared with fe-lsp would touch
fe-lsp's contested files. It is listed as a possible follow-up, not a prerequisite.

**Types: hand-written serde types for the subset we use**, in `fe_dap::protocol`.
The client needs about 30 messages:

- **Requests:** `initialize`, `launch`, `attach`, `configurationDone`,
  `setBreakpoints`, `setFunctionBreakpoints`, `setExceptionBreakpoints`,
  `threads`, `stackTrace`, `scopes`, `variables`, `continue`, `next`, `stepIn`,
  `stepOut`, `pause`, `evaluate`, `terminate`, `disconnect`, `restart`, `source`,
  `cancel`, `completions`, and later `setVariable`, `goto`/`gotoTargets`.
- **Events:** `initialized`, `stopped`, `continued`, `exited`, `terminated`,
  `thread`, `output`, `breakpoint`, `capabilities`, `invalidated`, `process` and
  `progressStart`/`Update`/`End`. `module`, `loadedSource` and `memory` are
  logged and ignored.
- **Reverse requests:** `runInTerminal` and `startDebugging`.

Why not a crate (§1):

- none tracks 1.71;
- the best maintained (`dap`) is built for adapters and is an alpha;
- `startDebugging` is missing from all but `dap-types`.

The parts that vary most, `launch` and `attach` arguments, are adapter-specific
JSON whatever crate is used. So they are a `serde_json::Map` built from the launch
configuration (§4), and the typed messages keep unknown fields in
`#[serde(flatten)] extra` maps so nothing an adapter sends is lost. Integers are
`i64`, following 1.71's `format` annotations. If maintaining these types costs more
than expected, `dap-types` can replace `protocol` behind the same API, since no
public item names it.

**Positions.** The client sends:

- `linesStartAt1: true`, `columnsStartAt1: true`, `pathFormat: "path"`;
- `supportsRunInTerminalRequest: true`;
- `supportsVariableType`, `supportsVariablePaging`, `supportsProgressReporting`,
  `supportsInvalidatedEvent` and `supportsStartDebuggingRequest` (the last only
  once multi-session lands; false in v1).

DAP doesn't say what unit its columns are in. VS Code counts them as UTF-16, so
fe-dap does too and maps them through `fe_text::PointUtf16`. Only frame columns and
(later) inline breakpoints use columns. Line breakpoints don't.

**The session state machine** (`fe_dap::Session`):

```text
Launching ─▶ Initializing ─▶ Configuring ─▶ Running ⇄ Stopped
   (spawn)    (initialize)   (initialized       │        │
                              event seen)        ▼        ▼
                                              Terminating ─▶ Ended(exit code?, reason)

Stopped ─(resume sent)─▶ Resuming ─(success)─▶ Running
                            │ (failure) ─▶ Stopped
                            │ (stopped event first) ─▶ Stopped
                            └ (timeout) ─▶ Uncertain ─(late success / continued)─▶ Running
                                               └──(late failure / stopped)─▶ Stopped

Stopped or Running ─(restart sent)─▶ Restarting ─(success)─▶ Running (fresh baseline)
                                          └ (failure / timeout) ─▶ stop, then start again
```

Every state can go to Terminating, and Terminating ignores late completions.

1. **Initializing.** Send `initialize` and keep the reply's `Capabilities`. A later
   `capabilities` event merges into them.
2. **Then, right away,** send `launch` or `attach` with the configuration's
   arguments, **without awaiting its response**. The specification lets an adapter
   answer `launch` only after `configurationDone`, and debugpy is reported to do
   so (D16 checks it). A client that waits would deadlock.
3. **Configuring.** On each `initialized` event, which may come before or after
   the `launch` response (and again after a `restart`, item 7):
   - send `setBreakpoints` for each file that has breakpoints;
   - send `setFunctionBreakpoints` (later);
   - send `setExceptionBreakpoints` with the user's filters (by default the
     filters the adapter marks `default`). This goes only if the adapter's
     `exceptionBreakpointFilters` is non-empty, or the adapter lacks
     `supportsConfigurationDoneRequest`, as the specification's
     `initialized` event says;
   - then send `configurationDone` if the adapter supports it.

   **Configuration is complete** at one of two points:

   - with `supportsConfigurationDoneRequest`, when the `configurationDone`
     response arrives (success or failure: a failure is shown, and the session
     carries on);
   - without it, when every configuration request sent on `initialized` has been
     answered, or has failed or timed out.

   A `launch` or `attach` error response ends the session with that message.

   The valid orderings, which the fake scripts separately (D2):

   - `initialized` → configuration → `configurationDone` → the `launch` response;
   - the `launch` response → `initialized` → configuration → `configurationDone`;
   - either of those from an adapter without `supportsConfigurationDoneRequest`,
     where configuration ends with the last configuration response.

   If the `launch` response has succeeded and no `initialized` event has come 5 s
   later, the session logs a warning and counts configuration as complete with
   nothing sent. Breakpoints are still sent if `initialized` comes later, so a
   slow adapter loses nothing.

   **The thread baseline.** Once both the `launch` (or `attach`) response and
   configuration completion have come, in either order, the session sends
   `threads` once, whatever state it is in. DAP makes `thread` events optional,
   so a program that runs without stopping may never send one. `thread` events
   then add and remove threads, and every stop fetches `threads` again.
4. **Running and Stopped.** The **stop epoch** is a counter that names the
   current suspension. `variablesReference` and `frameId` values are valid only
   while execution stays suspended (DAP's "lifetime of objects references"). So
   every id the UI holds carries the epoch it came from, and a request with an id
   from another epoch answers `Stale` locally, without a round trip.

   **Stopping.** `stopped` carries the thread, the reason and
   `allThreadsStopped`. It sets the state to Stopped, **increments the epoch**,
   and clears any pending resume, whatever was in flight.

   **Resuming**, with `continue`, `next`, `stepIn`, `stepOut`, later `goto`, and
   `restartFrame`:

   - **Sending** a resume request **increments the epoch at once**, so the
     stopped data (frames, scopes, variables, watch values, hover values) is
     retired before any reply. The state becomes Resuming, which disables the
     step and resume commands. The session records the request as the **pending
     resume**: its `seq` and the epoch it created. There is at most one pending
     resume, and a newer one replaces it.
   - **The completion guard.** A resume's completion (its success, its failure,
     including a "cancelled" failure, or its timeout) applies only if all three
     hold:
     - it is still the pending resume (the `request_seq` matches);
     - the epoch is still the one it created (no `stopped`, `continued` or other
       resume since);
     - the session is in Resuming, or in Uncertain because of this same pending
       resume's timeout. It must not be Terminating, Ended or Restarting.

     Otherwise it is logged and ignored. So a late failure can't undo a newer
     resume, a newer stop or a termination, and a late success can't mark a newer
     stop as running.
   - **A success response**, through the guard, moves Resuming (or Uncertain) to
     Running. Running is reached **without a `continued` event**: DAP sends
     `continued` only for resumes the client didn't ask for.
     - For `continue`, all threads run unless the response's
       `allThreadsContinued` is `false`, in which case only the requested thread
       does. `allThreadsContinued` exists only on `continue`'s response.
     - `next`, `stepIn` and `stepOut` resume all threads, since v1 never sends
       `singleThread`.
   - **A `stopped` before the response.** The specification says a step "first
     sends the response and then a `stopped` event", and that is the case D2
     tests first. Some adapters send the `stopped` first anyway. The session
     tolerates that: the stop clears the pending resume, so the late response
     fails the guard and changes nothing.
   - **A failure response**, through the guard, returns to Stopped, with the
     error shown. The data is fetched again under a new epoch, since the old ids
     were already retired.
   - **No response.** A resume that gets no response within its timeout (10 s)
     passes the guard as a timeout. The session sends `cancel` if
     `supportsCancelRequest` and moves to **Uncertain**, because the program may
     be running or still stopped. It keeps the request as the pending resume. A
     toast says "the adapter didn't answer `next`".

     The specification says a cancelled request still gets a response, so a late
     reply is the expected way out of Uncertain:
     - its late success, through the guard, moves to Running;
     - its late failure, "cancelled" included, means the program never resumed.
       That moves to Stopped and fetches again under a new epoch.

     Otherwise, in Uncertain:
     - Continue, Pause and Stop are enabled, and stepping is disabled;
     - a `stopped` event moves to Stopped, and `continued` moves to Running;
     - Continue sends a new resume, which replaces the pending one, so the old
       one's late reply fails the guard;
     - Pause sends `pause` (below).

     The controls never stay disabled waiting for a reply that may never come.
   - **`continued`** (unsolicited) moves to Running, increments the epoch, and
     clears any pending resume.

   **Pause** targets the selected thread, otherwise the thread that last stopped,
   otherwise the first thread in the baseline. With no thread known it sends
   `threads` first. If that list is empty, Pause stays disabled, with the tooltip
   "the program reported no threads". Pause changes no state itself: the
   `stopped` event that follows does. A failed or timed-out `pause` is shown and
   leaves the state as it was.

   **Late replies.** Every fetch (`threads`, `stackTrace`, `scopes`, `variables`,
   `evaluate`) carries the epoch it was sent in. A reply that arrives under
   another epoch is dropped, so a slow `variables` reply from before a step never
   fills the new stop's tree. Once Terminating, every completion other than
   `terminate` and `disconnect`'s own is ignored.
5. **Fetching on a stop.** Fetch `threads`, then `stackTrace` for the stopped
   thread (first 20 frames; more on demand, using `totalFrames`), then `scopes`
   for the top frame, then `variables` for each scope not marked `expensive`.
   Nothing deeper is fetched until the UI expands it, so children are lazy.
   Watch expressions are evaluated (context `watch`) against the selected frame.
   When some threads stay stopped after a single-thread `continue`, their frames
   are fetched again under the new epoch.
6. **Terminating.** Two different starts:
   - **The user stops.** With `supportsTerminateRequest`, send `terminate`, which
     asks the debuggee to end gracefully. Then send
     `disconnect { terminateDebuggee: true }` as soon as the adapter's
     `terminated` event arrives, or after 2 s without it. Without that support,
     send the `disconnect` at once.
   - **The adapter sends `terminated`** on its own (the program ended). Debugging
     is over, so send `disconnect` at once and never `terminate`.
     `terminated.body.restart` (the specification's request to restart, passing
     the value back as `__restart`) is **deferred**: v1 ends the session, and the
     user can rerun it.
   - Either way, if the adapter hasn't exited 2 s after the `disconnect`
     response (or its timeout), kill the adapter's process group.

   An `exited` event records the debuggee's exit code. An adapter exiting on its
   own ends the session as `Ended(AdapterExited)`. There is **no automatic
   restart**: a crashed adapter is an error to show, not a server to revive
   (contrast `lsp-client.md` §3.5).
7. **Restart.** With `supportsRestartRequest`, send `restart` with the
   configuration's launch arguments.
   - **Sending it** moves to **Restarting**, increments the epoch, clears any
     pending resume, and records the **pending restart** (its `seq` and the
     epoch it created).
   - **The restart guard** mirrors the resume guard: a completion applies only
     if it is the pending restart, the epoch is unchanged, and the state is
     still Restarting.
   - **Its success**, through the guard, moves to Running: threads are cleared
     and a fresh `threads` baseline is fetched. Any `initialized` the adapter
     sends again runs configuration again (item 3), so breakpoints are re-sent.
     A `stopped` (for example on entry) then moves to Stopped as usual.
   - **Its failure or timeout**, through the guard, falls back to emulation: stop
     the session (item 6), then start a new one from the same resolved
     configuration.

   Without `supportsRestartRequest`, restart is that emulation directly.

**Requests** have per-command timeouts on the injected clock: 10 s by default, 30 s
for `launch` and `attach`, and none for `evaluate` in the repl context, which the
user cancels. A dropped request future sends `cancel` when
`supportsCancelRequest`. Unlike async-lsp, nothing holds a request's slot open
after it is abandoned, so fe-lsp's orphan budget has no counterpart.

**Reverse requests:**

- **`runInTerminal`** becomes `DapEvent::RunInTerminal { args, cwd, env, kind,
  title, reply }`. The host runs it in a terminal tab (§5, §8) and replies with the
  `processId`.
- **`startDebugging`** (js-debug's child sessions) answers an error, "child
  sessions are not supported", in v1. That is why js-debug is not in the MVP (§9).

**Output and logging.** `output` events reach the sink by `category`: `console`,
`stdout`, `stderr`, `important` and `telemetry` (telemetry is dropped).

A per-session protocol log ring, like `LspManager::log`, backs a `debug.show_log`
command.

- **By default it records metadata only:**
  - each message's `seq`, `type`, `command` or `event`, `request_seq`,
    `success` and timing;
  - for a failed response, only that it failed and its numeric `body.error.id`,
    if any;
  - the *names* of a body's top-level keys for protocol-defined bodies. For the
    free-form `launch` and `attach` arguments, only the number of keys.

  It records **no free text and no values**: no error `message` or `format`
  string, no launch arguments, no `runInTerminal` argv or env, no variable
  values, evaluation expressions or results, and no output text. Any of those can
  hold a secret: a token in `args`, a password in a local variable, a key
  printed to stdout, an adapter's error echoing the command line it failed to
  run. The error message still reaches the user in its toast, which is on screen
  and not kept.
- **Payloads are opt-in.** A user setting, `debug.log_payloads` (never a project
  setting; §3's narrowing), records whole messages, error messages included.
- **Redaction applies in every mode** to every string the log records. In the
  default mode there are none, so it matters for payloads:
  - the values of `env` maps are replaced by `<redacted>`;
  - every string a `${env:NAME}` expanded to is replaced by `${env:NAME}`,
    wherever it appears: `args`, `program`, passthrough, an adapter's error
    message, an `output` event, an adapter echo.
- **The payload log's first line** says it may still contain secrets of the
  program's own (variables, output, evaluation results) and of the
  configuration's literal strings.

The log is for diagnosing adapters. Nothing claims that it is safe to share.

**A fake adapter for tests.** `fe_dap::fake` (behind a `test-support` feature, like
`fe_lsp::fake`) is an in-process adapter over an in-memory pipe. It is driven by a
script:

- a tiny "program": a list of source lines, the functions they belong to, and
  each function's locals;
- breakpoints, steps and `continue` move a program counter over it
  deterministically;
- knobs for the hard orderings:
  - the startup orderings of item 3, with and without
    `supportsConfigurationDoneRequest`;
  - resumes answered with no `continued` event;
  - a `stopped` event sent before a step's response;
  - a resume that fails;
  - a resume that is never answered;
  - a resume's failure delayed until after another resume, or after a
    terminate;
  - a `variables` reply delayed past a step;
  - a program that runs with no `thread` events;
  - unverified, then verified, breakpoints;
  - the adapter exiting mid-request.

Gate tests use only the fake. Real adapters are a slow `#[ignore]` group (D16).

**Decision.**

- `fe-dap` is GPUI-free, has its own framing and `AdapterLauncher`, and uses
  hand-written serde protocol types, with no DAP crate and not async-lsp.
- Its runtime is fe-lsp's: a thread per session, async-io and async-process.
- It keeps the session state machine above:
  - lazily fetched stop data guarded by a stop epoch;
  - the epoch retired as soon as a resume is sent;
  - Running reached on a resume's response, without needing `continued`;
  - late replies dropped;
  - a thread baseline after startup, for Pause.
- The protocol log records metadata only, unless the user opts into payloads.
- `runInTerminal` is handled by the host. `startDebugging` is refused in v1.
- A scripted fake adapter serves the gate.

## 3. Adapter discovery and installation

An **`AdapterSpec`** holds:

- a `name`;
- `command`, `args` and `env`;
- a `transport` (`Stdio`, or `Tcp` with `${port}` in `args`);
- the `languages` it serves, for picking a default;
- the `types` it answers to, the VS Code `type` strings `launch.json` uses (§4);
- `launch_defaults`, a JSON map merged under each configuration;
- `init_commands`, for LLDB adapters.

**Built-in specs, all found on `PATH`.** The launcher's environment `PATH`
includes mise's shims when the user's shell sets them. A spec whose program isn't
found is listed with "not found" and a hint.

| Name | Command | Transport | Languages | `type` aliases |
|---|---|---|---|---|
| `lldb-dap` | `lldb-dap`; on macOS also `xcrun -f lldb-dap` | stdio | rust, c, cpp | `lldb-dap` |
| `codelldb` | `codelldb` (the binary in the extension's `adapter/` dir) | stdio (`--port` also works) | rust, c, cpp | `lldb` |
| `debugpy` | `<python> -m debugpy.adapter` | stdio | python | `debugpy`, `python` |
| `delve` | `dlv dap --listen 127.0.0.1:${port}` | tcp | go | `go` |
| `js-debug` | `node <dir>/src/dapDebugServer.js ${port} 127.0.0.1` | tcp | javascript, typescript | `pwa-node`, `node` |

- **`<python>`** is the project's interpreter, the first of:
  - `<root>/.venv/bin/python`;
  - `$VIRTUAL_ENV/bin/python`;
  - `python3` on `PATH`.

  debugpy must be importable by it. If it isn't, the adapter's own error message
  ("No module named debugpy") is shown, with the install hint (D14).
- **Rust under an LLDB adapter.** Tenmoku adds `init_commands` that import the
  Rust toolchain's LLDB formatters:

  ```text
  command script import <sysroot>/lib/rustlib/etc/lldb_lookup.py
  command source <sysroot>/lib/rustlib/etc/lldb_commands
  ```

  `<sysroot>` comes from `rustc --print sysroot`, run in the root. Without them,
  `Vec` and `String` show as raw structs. codelldb has its own Rust support and
  doesn't need them.
- **The default for Rust, C and C++** is `lldb-dap` when found, otherwise `codelldb`.
- **Which specs ship when.** D3 adds lldb-dap, codelldb, debugpy and delve (delve
  as data, tested end to end in D19). js-debug's spec is listed here for the
  design but ships only with multi-session support, since it needs
  `startDebugging`.

**Resolution order**, the same layering as fe-lsp's `registry.rs`:

1. specs the host registers;
2. the user's `[debug.adapters.<name>]` settings (`command`, `args`, `env`, `path`);
3. extension-contributed specs, after consent;
4. the built-in specs.

**A project's settings can't name an adapter's command, arguments or environment.**
fe-config narrows a project's `debug` keys as `narrow_lsp` does for `lsp`
(`crates/fe-config/src/settings.rs:1219`), so opening a folder can't choose what
runs. The project chooses only which *configuration* to launch (§4), and that is
behind trust (§8).

**Installing.** Tenmoku doesn't download adapters in the MVP: it finds them, as
`lsp-client.md` Q2 decided for language servers. A later install flow (D14) follows
the grammar precedent (`c1pt00h80c-grammar-fetch.md`, `pp118n3cn5-grammar-prompt.md`):

- **Offer.** A toast offers to install when a configuration needs an adapter that
  isn't found. Nothing downloads without that click.
- **Pinned in the app.** The URL, size and SHA-256 of each artifact are compiled
  into the app, in a table like `extensions/grammars/pack.toml`. The hash is
  checked as the body arrives and again before anything is written, and the size
  limit is enforced at the read (`fe_net`'s `ReadLimit`).
- **Installed as an ordinary user extension.** For example
  `<extensions>/tenmoku.debug_codelldb/`, holding `extension.toml` with a
  `contributes.debug_adapters` entry. It is staged under the extensions directory
  and renamed into place, exactly like `tenmoku.grammar_<language>`. So it adds no
  new registration path, and the extensions picker lists and removes it.
- **Fetched from upstream, never rehosted:**
  - codelldb: its GitHub release `.vsix` for the platform, a zip. Only
    `extension/adapter/` and `extension/lldb/` are kept.
  - debugpy: its PyPI `none-any` wheel, unpacked. The spec runs the project's
    Python with `PYTHONPATH=<dir>`.

  Downloading at the user's request onto their machine is not Tenmoku
  distributing debugpy's EPL-1.0 pydevd or codelldb's LLDB. Rehosting them would
  be, which is why it isn't done.
- **Not installed by Tenmoku:** lldb-dap (it comes with LLVM or Xcode), delve
  (`go install`) and js-debug.

**Extension-contributed adapters.** The extensions record lists "debug adapters" as
out of scope for stage 1 (`vmtnmkay62-extensions-and-plugins.md` §8). This design
proposes adding a stage-1 kind, `[contributes.debug_adapters.<name>]`, with
`command`, `args`, `env`, `transport`, `languages` and `types`. It is gated exactly
as `[contributes.lsp.<name>]` is (`crates/fe-workbench/src/lsp_consent.rs`):

- consent binds to the effective invocation (resolved executable, arguments,
  environment, and the content hash of any file in the extension directory they
  name);
- any change asks again;
- interpreters are flagged;
- workspace extensions contribute one only after folder trust.

`crates/fe-lsp/src/extension_servers_tests.rs` is the test pattern to copy. The
install flow above depends on this kind.

**Decision.** Use the built-in specs above, found on `PATH`, with the user's
settings over them. Project settings are narrowed so they can't name what runs.
There are no downloads in the MVP. A later install flow (D14) installs codelldb and
debugpy as consented, hash-pinned user extensions, fetched from upstream.

**Open question Q1 (for the user):** may extensions contribute debug adapters
(`contributes.debug_adapters`, with `lsp`'s consent rules)? This amends the
extensions record's §8. D14 and D15 wait on the answer. If the answer is no, D14
writes into a managed `<state dir>/debug-adapters/` directory instead and gets its
own list-and-remove surface.

## 4. Launch configuration

**Where configurations live: a sibling file, `.tenmoku/launch.toml`**, not a
`[[debug]]` table in `.tenmoku/tasks.toml`. Three reasons:

1. **It mirrors VS Code's `launch.json` beside `tasks.json`**, so the import maps
   one file onto one file.
2. **`task_defs.rs` checks every entry's keys against a fixed list**
   (`ENTRY_KEYS`, `task_defs.rs:71`) so a typo is reported. A debug entry carries a
   free-form adapter passthrough table, which would weaken that check or
   complicate it.
3. **Breakage stays contained.** A `launch.toml` that doesn't parse loses only the
   debug configurations, while the tasks file and the detected tasks survive as
   they do today.

A new `crates/fe-workbench/src/launch_defs.rs` mirrors `task_defs.rs`:

- the same `ProjectFiles` seam (so tests need no disk);
- the same diagnostic shape (by name, else by place in the file, `#2`);
- the same rules: a later duplicate name is dropped, an unknown key is reported
  but keeps its entry;
- `cwd` is checked by its words.

It only models data and runs nothing.

```toml
[[launch]]
name = "server"                    # required, unique in the file
adapter = "lldb-dap"               # an adapter name or a VS Code `type` alias (§3)
request = "launch"                 # the default; "attach" is parsed but refused until D20
program = "${workspaceFolder}/target/debug/server"
args = ["--port", "8080"]          # an array only, never a shell string
cwd = "crates/server"              # relative to the root, the default
env = { RUST_LOG = "debug" }       # strings only
stop_on_entry = false
pre_launch_task = "build server"   # a task label, file or detected (§7)
console = "integrated"             # "integrated" (a terminal tab) or "internal"

[launch.adapter_args]              # passthrough: merged last into the launch arguments
sourceMap = { "/rustc/abc" = "/home/me/.rustup/toolchains/stable/lib/rustlib/src/rust" }

[[launch]]
name = "this file"
adapter = "debugpy"
program = "${file}"

[launch.adapter_args]
justMyCode = false
```

`adapter` is always a string naming the adapter. The passthrough table is
`adapter_args`, a different key: a TOML key can't be both a string and a table.
The example parses with Python's `tomllib`. In an array of tables,
`[launch.adapter_args]` belongs to the `[[launch]]` above it.

**Attach is not in the MVP.** `request = "attach"` and its keys (`pid`, later
`pid = "pick"` for a process picker, and `host`/`port` for remote) are recognised,
so the file isn't rejected, but such a configuration is listed with "attach isn't
supported yet" and can't be started until card D20.

**Building the launch arguments.** The adapter's `launch_defaults` come first, then
the typed fields mapped to the adapter's own key names, then `adapter_args`
verbatim. `program`, `args`, `cwd`, `env` and `stopOnEntry` are common to all five
adapters. debugpy and js-debug also accept a `module` or `runtimeExecutable`, which
go in `adapter_args`. `console = "integrated"` sets the key each adapter uses to
ask for `runInTerminal`:

- debugpy: `console: "integratedTerminal"`;
- lldb-dap: `runInTerminal: true`;
- codelldb: `terminal: "integrated"`;
- delve: no equivalent, so its output goes to the console.

**Variables**, expanded in `program`, `args`, `cwd`, `env` values and passthrough
strings:

- `${workspaceFolder}`, `${workspaceFolderBasename}`;
- `${file}`, `${fileDirname}`, `${fileBasename}`, `${fileBasenameNoExtension}`,
  `${relativeFile}`, `${lineNumber}`, which use the active editor and are an error
  when there is none;
- `${env:NAME}` and `${userHome}`.

`${command:…}` and `${input:…}` are **unsupported**. The configuration is listed
with a diagnostic naming the variable and can't be started. The one exception will
be `${command:pickProcess}`, which maps to `pid = "pick"` when attach lands (D20).

**Importing `.vscode/launch.json`, read-only.**

- It is read as JSONC (comments and trailing commas). Its configurations join the
  picker under the source `.vscode/launch.json`, after `launch.toml`'s. A name
  repeated in `launch.toml` wins over the imported one.
- `type` maps through §3's aliases. An unknown `type` lists the configuration with
  "no adapter for type X".
- `request`, `program`, `args`, `cwd`, `env` and `stopOnEntry` map to the typed
  fields. Every other key goes to the passthrough map as written.
- A matching `linux` or `osx` sub-object is laid over the configuration.
- `preLaunchTask` looks up a `tasks.toml` or detected task of that label. Tenmoku
  doesn't read `.vscode/tasks.json`, so a label only defined there gets "pre-launch
  task not found".
- `compounds`, `serverReadyAction`, `postDebugTask` and `presentation` are ignored
  with a diagnostic.
- Tenmoku never writes `launch.json`. A "copy to launch.toml" command is later.
- **JSONC:** if T480 (user snippets in the VS Code JSON format) lands a JSONC reader
  first, D4 uses it. Otherwise D4 writes a small comment-and-trailing-comma
  stripper over `serde_json` and T480 can adopt it.

**Configurations generated from what the project is**, listed after the file's own
with the source "detected". They come from the root only, as `task_defs`'s
detection does:

- **`Cargo.toml`:** one "Debug <bin>" per binary target. Its pre-launch step is
  `cargo build --bin <bin> --message-format=json-render-diagnostics`, and
  `program` is the `executable` path from cargo's `compiler-artifact` message, not
  a guess at `target/debug/<bin>`, which `CARGO_TARGET_DIR`, profiles and
  workspaces break. The build runs as a task (its output tab and problem matcher,
  §7).
- **A Python file open:** "Debug current file", `program = "${file}"`, with debugpy.
- **`go.mod`, later (D19, with the delve spec's end-to-end tests):** "Debug
  package", with delve's `mode = "debug"` and `program = "${fileDirname}"`. It is
  not part of D4.

With nothing configured, `debug.start` offers the detected ones first.

**Trust.** Listing configurations needs no trust. Starting one does, including a
detected one (a cargo build runs build scripts), as `task_run.rs`'s `UNTRUSTED`
does for tasks: "Trust this folder to debug its programs" (§8).

**Decision.**

- Configurations live in `.tenmoku/launch.toml`, as `[[launch]]` entries with the
  fields above and an `adapter_args` passthrough table. They are loaded by a new
  `launch_defs.rs` that copies `task_defs.rs`'s seams and diagnostics.
- Only `request = "launch"` starts in the MVP. Attach waits for D20.
- `.vscode/launch.json` is imported read-only, with `${command:}` and `${input:}`
  refused.
- Cargo and Python configurations are detected in the MVP, and cargo's artifact
  JSON names the program. Go detection comes with delve (D19).

## 5. UI

**The breakpoint model** has two layers:

- **Data:** `fe_dap::Breakpoint { path, line, enabled, condition, hit_condition,
  log_message }`. The last three are carried and persisted from D5 on, but nothing
  edits or sends them until D17.
- **The store:** `BreakpointStore` in `fe-workbench`, one per root. While a file is
  open, each of its breakpoints holds an `fe_buffer::Anchor` at its row's start
  with **`Bias::Right`** ("move after text inserted at this offset", `fe-text`'s
  `Bias`), and its row is read back from the anchor. This is how anchored
  decorations follow edits (`fe-editor/src/decorations.rs`), and how
  `gutter_annotations.rs` keeps rows.
  - **Why right bias.** A newline typed at the very start of the breakpoint's row
    (vim `O` above it, or Enter at column 0) pushes the code down. A left-biased
    anchor would stay before the inserted newline, leaving the breakpoint on the
    new empty row above. A right-biased anchor follows the code. Text typed at the
    row's start stays on the same row either way.
  - **Joining.** Deleting the newline before the row joins it to the row above,
    and the anchor collapses to the join, which is on the row above, now holding
    the code. Deleting the breakpoint's whole row collapses it onto the row that
    survives. Two on one row merge, and the first's settings win.
  - **Known limit: an empty row.** On an empty row, `O` on it and `o` on the row
    above are the same edit (an insertion at one offset), so no anchor can tell
    them apart. A breakpoint on an empty row moves down with either. Breakpoints
    belong on code rows, and adapters move empty-row breakpoints to the next code
    row anyway.
  - On close and on save, rows are written back as numbers.
- **Persistence:** per project, in `<state dir>/debug/<layout_key(root)>.json`
  (a `breakpoints` list, and the `watches` list Watch keeps)
  (`kiln::core::layout::layout_key`, which chats already use for their per-root
  directory). It is written atomically on change and debounced, and survives
  restarts. Breakpoints are the user's state, not the project's, so they never go
  in `.tenmoku/`.
- **Sending:** while a session runs, a file's breakpoints are sent again
  (`setBreakpoints` replaces a file's whole set) when one is added, removed,
  enabled, disabled or edited, and after a save that moved rows. They are not sent
  on every keystroke, because the running program doesn't match unsaved text.
- **Verification:** the adapter's `verified`, `message` and `line` come back per
  breakpoint, and later through `breakpoint` events. A breakpoint is drawn at the
  line the adapter reports for the session's lifetime, but that move is not
  persisted.

**The gutter: a breakpoint lane** at the gutter's left edge, left of line numbers,
where VS Code draws it. Two reasons it is a lane of its own:

- the existing mark slot holds one glyph per row (the diagnostic dot or the
  lightbulb: `GutterGlyph`, `view.rs:1972`), and a row can have both a breakpoint
  and a diagnostic;
- annotations (blame) are a text column.

The view gets `set_breakpoints(version, Vec<BreakpointMark>)` plus click and
context-menu callbacks, like `set_gutter_annotations`. States:

| State | Glyph | Card |
|---|---|---|
| enabled, verified (or no session) | filled dot, error tint | D6 |
| enabled, unverified | hollow dot; tooltip is the adapter's message | D6 |
| disabled | dot in the muted tint | D6 |
| current frame | arrow over the dot, accent tint | D7 |
| conditional or hit count | dot with a short bar; tooltip shows the condition | D17 |
| logpoint | diamond | D17 |

Interactions:

- A click on the lane toggles a breakpoint. A click on an existing one removes it.
- The context menu offers Disable or Enable, and Remove (D6). D17 adds Edit
  Condition, Edit Hit Count and Edit Log Message, each a one-line prompt (the
  existing modal input).
- With no gutter (`set_gutter(false)`), breakpoints are still set with commands,
  and the current-line band still shows.

**Debug toolbar.** It shows while a session exists, as a small **floating bar at
the top centre of the pane area**, over the editors, where VS Code's floating
debug toolbar sits. The workbench draws it in its own render
(`crates/fe-workbench/src/workbench.rs`), so it looks the same on both platforms.
The title row is not an option: it is kiln-shell's `TitleBar`, drawn only with
macOS's unified title bar (`crates/fastedit/src/main.rs:388`), and Linux has none.
Its buttons:

- Continue, or Pause while running (both while Uncertain, §2);
- Step Over, Step Into, Step Out;
- Restart and Stop;
- a session label naming the configuration, with a menu once multi-session exists.

Each button dispatches a `debug.*` command (§6), so the toolbar has no logic of its
own. Its disabled states come from the session state: no stepping while Running,
Resuming or Uncertain, and nothing but Stop while Resuming.

**Panels.**

- **A "Run and Debug" `ViewContainer` in the right dock**, with four views:
  Variables, Watch, Call Stack and Breakpoints. Source Control's Changes-over-Graph
  container (`dock.rs`, `source_control`) is the pattern. Its rail button joins the
  rail the first time a session starts, as References and Problems do once shown.
  - **Variables:** a lazily expanded tree, one root per scope, built on
    `uniform_list` rows like the project tree. It pages children beyond 100
    (`indexedVariables`/`namedVariables` with `start`/`count`). Set Value comes
    later.
  - **Watch:** user expressions, kept per project in the `watches` list of D5's
    state file, and
    evaluated on each stop.
  - **Call Stack:** threads, each holding its frames. Selecting a frame opens its
    source at its line and re-scopes Variables and Watch. Frames with no `path`
    (only a `sourceReference`), or with `presentationHint: "subtle"`, are dimmed.
    Opening a `sourceReference` frame as a read-only buffer from `source` comes
    later.
  - **Breakpoints:** every breakpoint grouped by file, plus the adapter's
    `exceptionBreakpointFilters` as on/off checkboxes. Rows reuse the Problems
    panel's grouping and reveal (`crates/fe-workbench/src/problems_panel.rs`). Filter conditions
    (`supportsExceptionOptions`, `filterOptions`) come later.
- **Debug Console: a tab in the bottom terminal panel.** It is not a terminal
  session, because there's no pty: an output list of `output` events plus a
  one-line input.
  - Each output row is coloured by category, and multi-line output keeps its rows.
    ANSI styling, when the adapter says `supportsANSIStyling`, is later.
  - The input has history and evaluates with context `repl` against the selected
    frame. Its result is an expandable value row. `completions` come later.
  - The terminal panel's tab strip already holds task tabs beside shells
    (`terminal_panel.rs`'s `add_task`). The console is a third kind of tab, so its
    tab strip and dock behaviour come for free.
- **The debuggee's own terminal: a debugger-owned tab**, opened by a new
  `TerminalPanel::add_debuggee(title, argv, cwd, env, session)`. It shares the task
  tab's *presentation*: named for the configuration, always listed, kept after
  exit with its output, and a check mark or failure toast. Its pid answers the
  request. It does **not** share task bookkeeping:
  - `add_task` records `last_task` (`terminal_panel.rs:326`), which
    `tasks.rerun_last` replays, and running a label again replaces the tab's
    session with the same command. The debuggee's argv is the adapter's one-shot
    launcher command: debugpy's includes a temporary listener port. Replaying it
    after the session fails, so the tab sets no `last_task` and never reruns its
    argv.
  - Its rerun action, if shown, dispatches `debug.rerun_last` for the tab's
    configuration. That resolves the configuration again and starts a fresh
    session, which asks the adapter for a fresh `runInTerminal`. A tab whose
    session is still running is stopped first.
  - A new session of the same configuration reuses the finished tab, as tasks do,
    through this path.

**The current line.** While stopped, the selected frame's row gets a
`RowBackground` decoration with a new tint, `Tint::DebugCurrent`, for the top
frame, or `Tint::DebugFrame` for a frame further up. Its gutter shows the arrow.
The editor scrolls to it on each stop and on each frame selection. `Tint` is the
seam that keeps fe-editor free of colours (`decorations.rs:29`). The theme files
gain two keys, falling back to the accent and muted tints.

**Run to cursor.** Add a hidden one-shot breakpoint at the cursor's row to that
file's `setBreakpoints`, then `continue`. Remove it at the next stop, whatever the
stop's reason. This needs no adapter support. `goto` (`<leader>dg`, "jump without
executing") comes later and only where `supportsGotoTargetsRequest`.

**Hover to evaluate.** While stopped, resting on an identifier, or on a selection,
evaluates it with context `hover`. It does so only when the adapter says
`supportsEvaluateForHovers`, since evaluating arbitrary expressions in the `repl`
context can have side effects. The value shows in the existing hover popover above
the language server's hover, as markdown: the value's type and value, and up to
20 first-level children fetched once. fe-gpui's `HoverContent` is text only
(`crates/fe-gpui/src/hover.rs:72`), so there is no expanding inside the popover:
the Variables view is where values expand. The identifier's
extent comes from the syntax tree, with no guessing at word boundaries.

**Inline values (later: D18, not in the MVP).** After a stop, each visible row of
the stopped function, up to the stopped line, gets an `AfterLine` decoration
(`decorations.rs:72`). It lists `name = value` for the top frame's local-scope
variables whose names occur as identifier nodes on that row. Values are truncated
to 40 chars. It uses only the variables already fetched and the fe-syntax tree, so
it needs no adapter support. D18 adds a setting, `debug.inline_values`, which
defaults to on and turns the feature off when false.

**Layout while debugging.** Starting a session shows the right dock's Run and Debug
container and the console tab once per session. Ending it leaves both where they
are. There is no separate debug layout in v1.

**What is checked headless, and what needs a macOS screenshot.**

- **Headless (GPUI test platform, on Linux):**
  - the breakpoint store's anchoring, merging and persistence;
  - gutter clicks hit-tested to rows;
  - every command's effect on the session (through the fake adapter);
  - the panels' row contents and expansion;
  - the console's rows and evaluation;
  - the current-line decoration set, and later the inline-value set (D18);
  - the toolbar's enabled states;
  - run to cursor's one-shot breakpoint;
  - hover evaluation text (the live-popover text hook, `build_with_text_system`).
- **macOS screenshot check** (look only; Linux Xvfb screenshots via the
  `run-fastedit-linux` skill are a first pass):
  - the lane's glyphs, their sizes and tints beside the diagnostic dot and
    lightbulb;
  - the floating toolbar over the editors;
  - the Run and Debug container's density;
  - the console tab;
  - the current-line band in both themes;
  - later, inline values' styling (D18) and the conditional and logpoint glyphs
    (D17).

**Decision.**

- An anchored (right-biased), per-project `BreakpointStore`, persisted in the
  state dir and sent per file on change and save.
- A dedicated gutter lane with the D6 states in the MVP. Conditional, hit-count
  and logpoint editing and glyphs come in D17.
- A floating toolbar at the top of the pane area, on both platforms, driven by
  `debug.*` commands.
- A right-dock Run and Debug container (Variables, Watch, Call Stack, Breakpoints)
  and a Debug Console tab in the bottom terminal panel.
- The debuggee's terminal is a debugger-owned tab with a task tab's look and no
  task rerun bookkeeping.
- The current line is a tinted band plus a gutter arrow.
- Run to cursor is a one-shot breakpoint.
- Hover evaluation only where the adapter supports it.
- Inline values are D18, after the MVP.

## 6. Vim mappings

**Commands**, all typed `#[kiln::command]`s on the workbench
(`workbench_commands.rs`) under a new `debug` namespace, which is free: nothing
registers `debug.*` today.

| Command | What it does | Card |
|---|---|---|
| `debug.start` | pick a configuration and start it; continue while stopped | D7 |
| `debug.stop` | terminate (§2) | D7 |
| `debug.rerun_last` | run the last configuration again | D7 |
| `debug.show_log` | the adapter protocol log | D7 |
| `debug.start_with_args` | start, prompting for extra args | D8 |
| `debug.continue` | continue | D8 |
| `debug.pause` | pause | D8 |
| `debug.step_over` / `debug.step_into` / `debug.step_out` | step | D8 |
| `debug.restart` | restart | D8 |
| `debug.run_to_cursor` | §5 | D8 |
| `debug.show_session` | session picker (one entry in v1) | D8 |
| `debug.toggle_breakpoint` | toggle at the cursor's row | D8 |
| `debug.toggle_breakpoint_enabled` | enable or disable at the cursor | D8 |
| `debug.remove_all_breakpoints` | clear the project's breakpoints | D8 |
| `debug.frame_up` / `debug.frame_down` | select the caller or callee frame | D9 |
| `debug.show_panels` | toggle the Run and Debug container | D9 |
| `debug.show_console` | show and focus the console tab; again: hide the bottom panel | D10 |
| `debug.evaluate` | evaluate the word or selection into a popover | D11 |
| `debug.hover` | the value under the cursor in a popover (`<leader>dw`) | D11 |
| `debug.test_at_cursor` | §7 | D13 |
| `debug.conditional_breakpoint` / `debug.log_point` | prompt, then set | D17 (after the MVP) |
| `debug.goto_line` | jump without executing | later, not carded |

**The LazyVim preset** (`crates/fastedit/presets/lazyvim.vimrc`) gets a new
`" debug` block after `" diagnostics`. The keys are LazyVim's own, checked in
`extras/dap/core.lua` at `c10948c5`. **This corrects the card's draft:** in
LazyVim, `<leader>do` is Step **Out**, `<leader>dO` is Step **Over**, Pause is
`<leader>dP`, and there is no `<leader>dp`.

| Key | LazyVim | Tenmoku command |
|---|---|---|
| `<leader>db` | Toggle Breakpoint | `debug.toggle_breakpoint` |
| `<leader>dB` | Breakpoint Condition | `debug.conditional_breakpoint`: D17 adds this line; until then it is in the unmapped list |
| `<leader>dc` | Run/Continue | `debug.start` (continues while stopped) |
| `<leader>da` | Run with Args | `debug.start_with_args` |
| `<leader>dC` | Run to Cursor | `debug.run_to_cursor` |
| `<leader>dg` | Go to Line (No Execute) | unmapped until `debug.goto_line` exists |
| `<leader>di` | Step Into | `debug.step_into` |
| `<leader>do` | Step Out | `debug.step_out` |
| `<leader>dO` | Step Over | `debug.step_over` |
| `<leader>dj` | Down (stack) | `debug.frame_down` |
| `<leader>dk` | Up (stack) | `debug.frame_up` |
| `<leader>dl` | Run Last | `debug.rerun_last` |
| `<leader>dP` | Pause | `debug.pause` |
| `<leader>dr` | Toggle REPL | `debug.show_console` |
| `<leader>ds` | Session | `debug.show_session` |
| `<leader>dt` | Terminate | `debug.stop` |
| `<leader>dw` | Widgets (hover) | `debug.hover` |
| `<leader>du` | Dap UI toggle (dap-ui) | `debug.show_panels` |
| `<leader>de` | Eval, in n and x (dap-ui) | `debug.evaluate` (`nnoremap` and `xnoremap`) |
| `<leader>td` | Debug Nearest (`extras/test/core.lua`) | `debug.test_at_cursor` |

LazyVim's Python extra has `<leader>dPt` and `<leader>dPc` (Debug Method and Debug
Class). They overlap `<leader>dP` (Pause) as a prefix, so they are left out, and
`<leader>td` covers the method case. The Rust extra's buffer-local `<leader>dr`
(rustaceanvim debuggables) loses to the core `<leader>dr`, as it would in LazyVim
for a non-Rust file. `<leader>dg` joins the preset's "Unmapped LazyVim keys" list
until `goto` lands, and so does `<leader>dB` until D17 adds conditional
breakpoints and its preset line.

**F-keys, for everyone** (`crates/fe-keymap/keymaps/fastedit.toml`), following VS
Code's layout:

| Key | Command | Context |
|---|---|---|
| `f5` | `debug.start` (continue when stopped) | workbench |
| `shift-f5` | `debug.stop` | while a session exists |
| `secondary-shift-f5` | `debug.restart` | while a session exists |
| `f6` | `debug.pause` | while running |
| `f9` | `debug.toggle_breakpoint` | editor |
| `f10` | `debug.step_over` | **while stopped only** |
| `f11` | `debug.step_into` | **while stopped only** |
| `shift-f11` | `debug.step_out` | while stopped |
| `secondary-f10` | `debug.run_to_cursor` | while stopped |

**Linux clashes:**

- `f10` opens the menu bar on Linux (`fastedit.toml:739`, platform `!macos`;
  `linux-menu-bar.md`).
- `f11` toggles fullscreen there (`fastedit.toml:611`).

Both stay as they are except while a session is **stopped**. The workbench sets
a `debug_stopped` context key then, and the step bindings carry it, so they apply
only in that state and come after the menu-bar and fullscreen bindings in the
table. On macOS the F-keys need `fn` unless the user has set them as standard keys,
as VS Code users know.

**Card scope traps (from memory of earlier cards):**

- A new binding breaks the fe-gpui binding-count tests and the `DEDICATED` table.
- A new command needs its rows in `typed_command_ids_tests.rs` and
  `command_catalog_tests.rs`.
- There can be only one `#[kiln::command] impl Workbench`.

Each card that adds a command or a binding puts those test files in its scope up
front (§11). The preset lines go in with D12, except `<leader>td` (D13) and
`<leader>dB` (D17).

**Decision.** Use the `debug.*` commands above. The LazyVim preset maps LazyVim's
real `<leader>d*` keys and `<leader>td`, as tabled. The F-keys follow VS Code, with
F10 and F11 claimed only while stopped so Linux keeps its menu-bar and fullscreen
keys.

## 7. Tasks runner and test at cursor

**Pre-launch tasks.** `pre_launch_task` names a task from `task_defs::load` (file or
detected). `debug.start`:

1. runs the task through the existing path (`Workbench::run_task`, so it holds
   trust, opens a terminal-panel tab, and runs its problem matcher into Problems
   at exit; `task_run.rs`);
2. waits for its session to exit;
3. starts the debug session only if the task exited 0.

On failure it toasts "pre-launch task <label> failed" and doesn't start, and
Problems already lists what the matcher found.

The detected Cargo configurations (§4) run their `cargo build
--message-format=json-render-diagnostics` the same way. The task runner gets one
small seam for this: a task whose output a caller reads at exit (a callback with
the captured output), so the debugger can read cargo's `compiler-artifact` lines.
The terminal tab still shows the rendered diagnostics. That seam belongs in D7's
scope (`task_run.rs`, `terminal_panel.rs`).

**The output pane.** The debuggee's terminal tab (§5) looks like a task tab: it
is kept after exit, with the exit mark and the failure toast. But it is the
debugger's own (`TerminalPanel::add_debuggee`), not a task:

- it never becomes `tasks.rerun_last`'s task;
- it never replays the adapter's one-shot argv;
- its rerun goes through `debug.rerun_last` and a fresh session.

A debuggee's output isn't run through a problem matcher. That would turn a
program's own error output into Problems.

**Debug Test at Cursor.** The "run test at cursor" cards are being filed on the Mac
board: **Mac T1409** (Run Test at Cursor and Tests in File, through the tasks
runner) and **Mac T1410** (the Run Test lens). Neither has a helium handle yet:
`rgt status` on helium lists no such card on 2026-11-06. When T1409 is cut back to
helium, D13 names its helium handle. This record asks T1409 to expose one seam that
both Run and Debug consume:

```rust
/// A test the cursor is in, resolved: Mac T1409 produces it, `tests.run_at_cursor`
/// and `debug.test_at_cursor` both consume it.
pub struct ResolvedTest {
    pub label: String,            // "tests::parses_empty", for tab names and rerun
    pub program: String,          // what Run runs: "cargo", "python3", "go"
    pub args: Vec<String>,        // ["test", "-p", "fe-text", "--lib", "--", "tests::parses_empty", "--exact"]
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub framework: TestFramework, // what Debug needs that a command line hides
}

pub enum TestFramework {
    /// Run: `cargo test …`. Debug: build with `--no-run --message-format=json`,
    /// take the test executable from cargo's artifact message, launch it with
    /// `[name, "--exact", "--nocapture"]` in the directory of the artifact's
    /// `manifest_path`, with `CARGO_MANIFEST_DIR` set, as `cargo test` runs it.
    Cargo { package: String, target: CargoTarget, name: String },
    /// Run and Debug: `python -m pytest <node_id>`; Debug launches debugpy with
    /// `module = "pytest"`, `args = [node_id]`.
    Pytest { node_id: String },
    /// Run: `go test -run '^Name$' ./pkg`. Debug: delve `mode = "test"`,
    /// `program = <package dir>`, `args = ["-test.run", "^Name$"]`.
    GoTest { package_dir: PathBuf, name: String },
}
```

Run consumes `program`, `args`, `cwd` and `env`. Debug consumes `framework` and
`env`, and `cwd` for pytest. For Cargo, Debug's working directory is the test
binary's package directory, from cargo's artifact message: `cargo test` runs a
test binary there, not where it was invoked, and tests that read fixtures by
relative path depend on it. That way the two never drift on which test the cursor is in, and Debug
never parses a command line back. T1409's test detection (syntax tree or LSP
runnables) is reused unchanged. Mac T1410's lens gets a "Debug" item beside "Run"
that dispatches `debug.test_at_cursor` with the lens's test. If T1409 lands without
`framework`, D13 adds it there, in T1409's module, rather than inferring it from
`args`.

**Decision.**

- `pre_launch_task` reuses `run_task` and starts the session only after it exits 0.
- Detected Cargo configurations read the program from cargo's artifact JSON,
  through a small read-output-at-exit seam on task runs.
- The debuggee runs in a debugger-owned terminal tab that looks like a task tab,
  with no problem matcher, no `last_task` entry and no argv replay.
- Debug Test at Cursor consumes Mac T1409's `ResolvedTest` (with `framework`), and
  Mac T1410's lens gets a Debug item.

## 8. Plugin and sandbox considerations

**Adapters run as native processes with the user's rights, like language servers.**
A debug session runs the project's program by definition, so it needs **folder
trust**:

- `debug.start` (and every path to it: F5, the lens, test at cursor, a restart)
  checks the root's live trust just before launching, as tasks do. An untrusted
  root's configurations are listed but never started. The toast reads "Trust this
  folder to debug its programs", with the Trust action.
- The trust prompt (`trust.rs`, T177) gains "and run its debug
  configurations" in its explanation.
- This mirrors `lsp_trust_tests.rs` and `lsp_consent_tests.rs`: untrusting a root
  while a session runs stops the session.

**`runInTerminal`** runs what the adapter asks, in a terminal tab. Only a trusted
root's session can be asking, since the session itself needed trust, so it is
gated by the same rule and needs no second prompt. Its argv is run directly, never
through a shell. The adapter's `argsCanBeInterpretedByShell` is ignored in v1, and
args are passed as an array.

**What a project file can and can't decide:**

- `launch.toml` and `launch.json` can choose the adapter *by name*, the program,
  arguments, cwd, env and the passthrough map.
- They can't choose the adapter's *command* (§3), so an untrusted project can't make
  Tenmoku run an arbitrary executable as "the adapter".
- Once the folder is trusted, the program is the project's code anyway, which is
  why trust is the gate.

**Env and secrets.** `env` values in `launch.toml` are plain text in the project, as
in `tasks.toml`. The doc comment says so and recommends `${env:NAME}`, so a secret
stays in the user's environment. The protocol log (§2) records no values and no
free text (error messages included) by default. With `debug.log_payloads` on, it redacts `env` values and every
`${env:NAME}` expansion wherever it appears. It still makes no claim that a log
is safe to share, since variables, evaluation results and output can hold
secrets of the program's own.

**Plugins (stage-2 wasm).** There is **no plugin access to debugging in v1**:

- no WIT interface for breakpoints, sessions or evaluation;
- no plugin-callable `debug.*` command (they stay off the plugin allowlist, which
  the extensions record's stage-2 decisions keep to read-only, navigation and two
  edit commands).

Evaluation runs code in the debuggee, so a later `fastedit:debug` interface would
need its own capability (`debug.read`, `debug.control`) under the extensions
record's grant and escalation rules. That is a separate design.

The extensions record's VS Code risk table already names "debug launch" config as
something a workspace must not get to run: trust is that gate here. The only
extension surface this design adds is Q1's declarative `debug_adapters`
contribution, and its consent is `lsp`'s. That keeps to the direction of the plugin
cards (helium T431, T432, T433, T453, and the Mac plugin cards T1347, T1348 and
T1357, which aren't readable from helium): no new plugin import and no change to
grants.

**Decision.**

- Starting a session (including via `runInTerminal`) requires live folder trust.
- Project files choose configurations, never adapter commands.
- `runInTerminal` runs argv directly.
- Logs record metadata only by default. Opt-in payload logs redact `env` values
  and `${env:}` expansions, and logs are never called safe to share.
- Plugins get no debugger access in v1. Extension-contributed adapters (Q1) use
  `lsp`'s consent.

## 9. MVP scope and non-goals

**MVP:** cards D1 to D13 and D16, with the adapters lldb-dap or codelldb (Rust, C,
C++) and debugpy (Python). Every MVP card's sketch and criteria (§11) stay inside
this list. What is outside it has a card of its own (D14, D15, D17 to D20) or is
left for a later design.

- Launch request only. `attach` configurations are listed but refused.
- Line breakpoints: toggled, enabled or disabled, persisted, and verified state
  shown. No conditions, hit counts or logpoints.
- Continue, pause, the three steps, run to cursor, stop and restart.
- Threads and the call stack, with frame selection.
- Variables with lazy expansion, and Watch.
- The Debug Console with repl evaluation and output.
- Hover evaluation.
- The debuggee in an integrated terminal tab.
- Pre-launch tasks, and detected Cargo and Python configurations (no Go).
- Exception filters as on/off checkboxes.
- `launch.json` import.
- The LazyVim preset and F-keys.
- Debug test at cursor for Cargo and pytest.

**Later, with cards sketched in §11** and filed after the MVP:

- conditional breakpoints, hit counts and logpoints, with their prompts, glyphs and
  `<leader>dB` (D17; the model carries the fields from D5);
- inline values and `debug.inline_values` (D18);
- delve end to end, and Go detection (D19);
- attach, by pid and then with a process picker (D20);
- the install flow (D14) and extension-contributed adapters (D15), pending Q1.

**Later, not yet carded**, each needing its own sketch first:

- function and data breakpoints, and exception filter conditions;
- set value;
- `goto`;
- js-debug, which needs multi-session through `startDebugging`;
- multiple concurrent sessions;
- remote debugging (`attach` to host and port, path mapping);
- disassembly and memory views;
- `sourceReference` sources;
- the console's ANSI styling and completions.

**Non-goals:**

- a debugger of our own, or an in-process LLDB;
- reading or writing `.vscode/tasks.json`;
- compound configurations;
- debugging on Windows (not a target: the extensions record's stage-2 decision);
- a plugin debugging API in v1.

**Decision.**

- The MVP is the list above, with lldb-dap or codelldb plus debugpy, built by
  D1 to D13 and D16.
- D14, D15 and D17 to D20 are sketched and filed after it.
- The rest of "Later" gets its own design before any card.

## 10. Linux and macOS

| Adapter | Linux | macOS |
|---|---|---|
| lldb-dap | from LLVM: distro `lldb` packages (Ubuntu's 21.1.6) or an LLVM release. It was renamed from `lldb-vscode` in LLVM 18 | with Xcode or its Command Line Tools (`xcrun -f lldb-dap`), Apple-signed, debugging through Apple's `debugserver` |
| codelldb | self-contained linux-x64/arm64 builds | darwin-arm64/x64 builds. The downloaded files carry quarantine. As a user-installed extension (D14), Tenmoku clears the quarantine attribute only on files whose hash it checked |
| debugpy | any Python ≥ 3.10 (1.8.22's `requires_python`) | the same |
| delve | Linux builds | needs Xcode CLT's `debugserver` |
| js-debug | needs Node | the same |

**ptrace on Linux.** Native debugging is subject to Yama's
`kernel.yama.ptrace_scope`:

- **`1`, Ubuntu's default,** allows launching a program under the debugger but not
  attaching to an unrelated pid. Attach then needs `ptrace_scope=0` or
  `CAP_SYS_PTRACE`.
- **`2`, helium's setting,** forbids both without `CAP_SYS_PTRACE`.
- **`3`** forbids ptrace entirely.

When a native adapter's launch fails with a ptrace or permission error, Tenmoku's
toast names the setting and its value (read from
`/proc/sys/kernel/yama/ptrace_scope`) rather than leaving the adapter's terse
message. The same check makes D16's native real-adapter tests skip, with a reason,
where the scope forbids them. So on helium only the debugpy half of D16 runs.

**macOS signing and entitlements.**

- **Tenmoku itself** needs no debugger entitlement (`com.apple.security.cs.debugger`):
  it never ptraces anything. The adapter does, and Apple's `lldb-dap` and
  `debugserver` carry their own entitlements. `Tenmoku.entitlements` stays as it
  is (JIT and unsigned executable memory, for wasm).
- **The debuggee** must be debuggable. Programs a user builds locally (cargo,
  clang) are ad-hoc signed without the hardened runtime, and `debugserver` can
  attach to them. A hardened-runtime binary needs `com.apple.security.get-task-allow`
  in its own entitlements, which Xcode's Debug configurations add. Tenmoku can't
  fix that, so the failure toast says so.
- **The first debug session on a Mac** may raise the system's "Developer Tools
  Access" prompt (the user must be in the `_developer` group, or run
  `DevToolsSecurity -enable`). Tenmoku documents it and doesn't script it.

**`fe-dap` has nothing macOS-only.** Its paths, process groups and TCP are Unix. The
`xcrun` lookup for lldb-dap is a built-in spec detail in fe-dap's registry behind
`cfg(target_os = "macos")`, and is testable as data on Linux. Every card except the
look checks in §5 is Linux-OK.

**Decision.**

- Every MVP card is Linux-OK.
- lldb-dap and codelldb cover both platforms, and debugpy everywhere.
- Ptrace refusals are explained by naming `ptrace_scope`.
- `Tenmoku.entitlements` doesn't change.
- Debuggee signing is the user's build's concern, and its failure is explained.
- Visual checks of the gutter, toolbar, panels and current line need a macOS
  screenshot (§5).

## 11. Proposed card breakdown

Nothing below is filed by this card. Sizes are xs to xl.

- **Linux-OK** means a helium lane can do the whole card. No card is Mac-only.
- **"+ Mac look"** means a macOS screenshot check is part of acceptance. On helium
  that criterion is moved to a Mac respec before the first finish, since review
  fails a look check made on Linux.
- **Scope** lists every file a card is expected to touch, including the `lib.rs`
  that declares each new module and the command-catalog tests
  (`crates/fe-workbench/src/typed_command_ids_tests.rs`,
  `crates/fe-workbench/src/command_catalog_tests.rs`) for each new command.
- **Commands** are each owned by the card that builds what they do. Every
  `#[kiln::command]` goes in the workbench's one command impl,
  `crates/fe-workbench/src/workbench_commands.rs`.
- Criterion ids are per card.

**D1. fe-dap: transport, protocol types and fake adapter.**
Size m. Depends on nothing. Linux-OK.
Scope: `crates/fe-dap/**`, `Cargo.toml`, `Cargo.lock`.

A new GPUI-free crate:

- Content-Length framing over stdio and TCP, with the 64 MB limit;
- the hand-written `protocol` types for §2's messages, with `extra` passthrough
  maps;
- `AdapterLauncher`, `LocalLauncher` (process group, `${port}`, connect retries),
  `LaunchCancel` and the clock;
- `fake` behind `test-support`, with every knob §2 lists, so D2 needs no change
  to it. It serves any byte stream, including an accepted TCP connection.
- `fe-dap-fake`, a test-support binary target that runs the fake over TCP on the
  port it is given, so `LocalLauncher`'s TCP mode is tested end to end.

Criteria:

- C1: round-trip tests of framing: split headers, two messages in one read, an
  oversize body, an unknown header.
- C2: serde tests of every type against spec examples, with unknown fields kept.
- C3: the fake answers an `initialize` and `launch` script over an in-memory pipe,
  and each §2 knob has a test showing it changes what the fake sends.
- C4: `LocalLauncher` starts `fe-dap-fake` with `${port}` substituted and connects
  to it.
- C5: `scripts/check-no-gpui.sh` prints `ok: fe-dap`, and `cargo deny check`
  passes.

**D2. Session model.**
Size l. Depends on D1. Linux-OK.
Scope: `crates/fe-dap/src/**` (session, requests, events, log, and `lib.rs`),
`crates/fe-dap/Cargo.toml`.

`Session` and its actor, built on §2's state machine:

- `launch` not awaited before configuration;
- configuration completion with and without `configurationDone`;
- conditional `setExceptionBreakpoints`;
- per-file `setBreakpoints`;
- stop epochs and lazy `threads`/`stackTrace`/`scopes`/`variables`;
- the thread baseline after startup;
- `evaluate`, steps and pause, with the resume guard, Uncertain, and late replies
  of §2 item 4;
- restart with its own guard (item 7);
- terminate and `terminated` handling (item 6);
- timeouts and `cancel`;
- `runInTerminal` as an event with a reply;
- `startDebugging` refused;
- the metadata-only protocol log, and payload redaction. The log mode is a
  `Session` option, which D7 sets from D3's setting.

Criteria:

- C1: against the fake, the session reaches Stopped under each startup ordering
  of §2 item 3, scripted separately:
  - `initialized` → configuration → `configurationDone` → the `launch` response;
  - the `launch` response → `initialized` → configuration → `configurationDone`;
  - an adapter without `supportsConfigurationDoneRequest`, whose configuration
    ends with the last configuration response.

  `setExceptionBreakpoints` is sent only under §2's condition.
- C2: with a fake that sends no `continued` event, a successful `next` moves the
  session to Running on its response. Step commands are disabled from the send
  on. A `variables` request with the old reference answers `Stale` without a
  request.
- C3: the conformant order first: a step's response, then `stopped`, ends Stopped
  at a new epoch. Then the tolerated non-conformant order: a `stopped` before the
  step's response also ends Stopped, and the late response changes nothing. A
  `variables` reply delayed past a step is dropped. A failed `next` returns to
  Stopped with fresh data.
- C4: the resume guard and Uncertain:
  - a `next` failure delayed until after a later `continue` changes nothing;
  - a `next` failure delayed until after Stop changes nothing, and the session
    still ends;
  - a `next` that is never answered moves to Uncertain after its timeout, with
    Continue, Pause and Stop enabled, and then each of these resolves it:
    - its late success moves to Running;
    - its late "cancelled" failure moves to Stopped, with fresh data;
    - a `stopped` event moves to Stopped.
- C5: Pause on a launch that runs at once, with no `thread` events, sends `pause`
  with a thread id from the startup `threads` baseline. This holds both with and
  without `supportsConfigurationDoneRequest`.
- C6: restart:
  - with `supportsRestartRequest`, a successful `restart` reaches Running with a
    fresh baseline, and re-sends breakpoints on the adapter's new `initialized`;
  - a failed or unanswered `restart` falls back to stop and start;
  - a `restart` completion arriving after Stop changes nothing.
- C7: termination:
  - user Stop sends `terminate`, then `disconnect` on `terminated`, and kills a
    mute adapter 2 s after the `disconnect`;
  - an adapter's own `terminated` gets `disconnect` at once and no `terminate`.
- C8: the default log holds no argument, variable, evaluation or output values
  and no error text. A failed `launch` whose `message` echoes the expanded
  `${env:API_TOKEN}` leaves only the failure and its error id in the default log.
  With payloads on, that token shows as `${env:API_TOKEN}` in the request, in the
  error message and in any adapter echo.

**D3. Adapter registry and discovery, and `[debug]` settings.**
Size m. Depends on D1. Linux-OK.
Scope: `crates/fe-dap/src/registry*.rs`, `crates/fe-dap/src/lib.rs`,
`crates/fe-config/src/settings.rs`, `crates/fe-config/src/tests/**`.

`AdapterSpec`, §3's built-in specs for lldb-dap, codelldb, debugpy and delve (as
data only: delve is tested end to end in D19; js-debug's spec waits for
multi-session), and resolution in the order host → user settings → extension →
built-in. Also:

- `PATH` lookup, and the `xcrun` lookup behind `cfg(macos)`;
- the Python interpreter choice;
- Rust `init_commands` from `rustc --print sysroot`;
- `DebugSettings` (`[debug.adapters.<name>]`, timeouts and `debug.log_payloads`),
  with `narrow_debug` stripping a project layer's adapter commands and
  `log_payloads`. `debug.inline_values` is D18's.

Criteria:

- C1: resolution order, as table tests.
- C2: a project settings file can't set an adapter's `command`, `args` or `env`,
  or `debug.log_payloads`.
- C3: `type` aliases resolve.
- C4: a missing program gives a "not found" status.

**D16. Real-adapter slow test group.**
Size s. Depends on D2 and D3. Linux-OK.
Scope: `crates/fe-dap/tests/real_adapter.rs`, `crates/fe-dap/Cargo.toml`,
`scripts/dap-real.sh`, `.github/workflows/dap.yml`.

`#[ignore]` tests in the style of `lsp-real.sh`:

- debugpy (a pinned wheel hash in the script) runs a Python fixture through one
  breakpoint, a step, variables and an evaluate;
- lldb-dap or codelldb does the same for a tiny Rust binary;
- native tests skip, naming `ptrace_scope`, where it forbids them.

These are the first observations of §2's ordering claims (§1, Spike). Criteria:

- C1: debugpy passes locally.
- C2: native tests skip with a reason on helium and pass on a GitHub runner.

**D4. Launch configurations: `launch.toml`, detection and `launch.json` import.**
Size l. Depends on D3. Linux-OK.
Scope: `crates/fe-workbench/src/launch_defs*.rs`, `crates/fe-workbench/src/lib.rs`,
`crates/fe-workbench/Cargo.toml`, `Cargo.lock`, and
`crates/fe-workbench/src/jsonc*.rs` if T480 has no JSONC reader.

§4's schema (with the `adapter_args` passthrough), variables, diagnostics, Cargo
and Python detection (Go is D19's), and the read-only, JSONC-tolerant
`launch.json` import with `type` mapping and platform sub-objects. `attach`
entries parse and are marked not startable. It is a data model only, and tested
through `ProjectFiles`. fe-workbench gains its `fe-dap` dependency here, for
D3's alias table. Criteria:

- C1: every field and diagnostic, by table tests mirroring `task_defs_tests.rs`,
  including §4's example file parsed whole, with each `adapter_args` table on its
  own entry.
- C2: an `attach` entry is listed with "attach isn't supported yet" and can't be
  started.
- C3: `${command:…}` and `${input:…}` give a diagnostic and can't be started.
- C4: a `launch.json` with comments, trailing commas and an `osx`/`linux` override
  imports.
- C5: a name in `launch.toml` wins over an imported one.

**D5. Breakpoint model and persistence.**
Size m. Depends on D1 and D4. Linux-OK.
Scope: `crates/fe-workbench/src/debug_breakpoints*.rs`,
`crates/fe-workbench/src/lib.rs`, `crates/fe-dap/src/breakpoint.rs`,
`crates/fe-dap/src/lib.rs`.

`BreakpointStore`, built on §5's model:

- per root;
- anchored with `Bias::Right` while a file is open, rows written back on save and
  close;
- collapsing and merging on deleted rows;
- persisted to `<state dir>/debug/<layout_key>.json`, atomically and debounced.
  The file has a `breakpoints` list and a `watches` list, which D5 keeps and
  writes back unchanged, and D9 fills;
- the condition, hit and log fields carried as data only (edited and sent from
  D17).

Criteria:

- C1: a breakpoint follows inserted and deleted rows above it.
- C2: a newline inserted exactly at the start of the breakpoint's (non-empty) row
  moves the breakpoint down with its text, both by `O` on that row and by Enter
  at column 0.
- C3: deleting its row moves it to the surviving row. Joining it to the row above
  moves it there.
- C4: a restart restores breakpoints, and an unknown `watches` list survives a
  write.
- C5: a file reloaded from disk keeps its breakpoints on the right rows.

**D6. Breakpoint gutter lane, and the debug tints.**
Size m. Depends on D5. Linux-OK, + Mac look.
Scope:

- `crates/fe-gpui/src/breakpoint_gutter*.rs`, `crates/fe-gpui/src/view.rs`,
  `crates/fe-gpui/src/lib.rs`, `crates/fe-gpui/src/theme.rs`,
  `crates/fe-gpui/src/element/**`;
- `crates/fe-editor/src/decorations.rs`;
- `crates/fe-config/src/theme.rs`, `crates/fe-config/themes/*.toml`;
- `crates/fe-workbench/src/debug_breakpoints*.rs`.

The lane:

- `set_breakpoints` with its click and context-menu callbacks (Disable or Enable,
  Remove);
- the D6 rows of §5's state table, including the current-frame arrow, which D7
  sets;
- tooltips.

The tints: `Tint::DebugCurrent` and `Tint::DebugFrame` in fe-editor, their arms
in `tint_color` (`crates/fe-gpui/src/element/decorations.rs:64`), and the two
`EditorTheme` keys with fallbacks in every bundled theme, as
`vim_yank_highlight` was added. Condition, hit-count and logpoint editing and
glyphs are D17's. Criteria:

- C1: a click on row 7's lane toggles a breakpoint at row 7, and not at the
  carets.
- C2: the states follow verification events.
- C3: the lane coexists with the diagnostic dot and the lightbulb.
- C4: a theme without the new keys gets the fallbacks.
- C5 (Mac look): a screenshot of every D6 state and both bands, in both themes.

**D7. Debug sessions in the workbench.**
Size l. Depends on D2, D3, D4, D5 and D6. Linux-OK.
Scope:

- `crates/fe-workbench/src/debug_session*.rs`, `crates/fe-workbench/src/lib.rs`;
- `crates/fe-workbench/src/workbench_commands.rs`,
  `crates/fe-workbench/src/workbench_root.rs` (`reload_settings`, for
  `DebugSettings`);
- `crates/fe-workbench/src/task_run*.rs`, `crates/fe-workbench/src/terminal_panel*.rs`,
  `crates/fe-workbench/src/trust*.rs`;
- the command-catalog tests.

The workbench side of a session:

- commands `debug.start` (its configuration picker), `debug.stop`,
  `debug.rerun_last` and `debug.show_log`;
- the live trust check and its toast;
- `pre_launch_task` via `run_task`, plus the read-output-at-exit seam for cargo's
  artifact JSON;
- `runInTerminal` into a debugger-owned tab (`TerminalPanel::add_debuggee`, §5)
  that sets no `last_task` and whose rerun goes through `debug.rerun_last`;
- the current-line band and D6's arrow;
- breakpoints sent on change and save;
- `DebugSettings` applied through `reload_settings`;
- the ptrace and signing failure toasts.

Criteria:

- C1: an untrusted root lists configurations and refuses to start one.
- C2: a failing pre-launch task starts no session.
- C3: against the fake, a stop opens the file and bands the stopped row.
- C4: untrusting stops a running session.
- C5: after a debuggee tab's session ends, `tasks.rerun_last` still reruns the last
  real task. The tab's rerun starts a new session, which gets a new
  `runInTerminal`, rather than replaying the old argv.
- C6: changing `debug.log_payloads` in user settings applies to the next session.

**D8. Debug toolbar, stepping and breakpoint commands, and F-keys.**
Size m. Depends on D7. Linux-OK, + Mac look.
Scope:

- `crates/fe-workbench/src/debug_toolbar*.rs`, `crates/fe-workbench/src/lib.rs`;
- `crates/fe-workbench/src/workbench.rs` (hosting the toolbar, and the
  `debug_stopped` key context at its root's `key_context`);
- `crates/fe-workbench/src/workbench_commands.rs` and the command-catalog tests;
- `crates/fe-keymap/keymaps/fastedit.toml`;
- the fe-gpui binding-count tests (`crates/fe-gpui/src/command_tests.rs` and
  `crates/fe-gpui/src/tests.rs`'s `DEDICATED` table);
- `crates/fe-workbench/src/shortcuts_recorder_tests.rs`.

Commands:

- `debug.continue`, `debug.pause`, `debug.step_over`, `debug.step_into`,
  `debug.step_out`, `debug.restart` and `debug.run_to_cursor`;
- `debug.start_with_args` and `debug.show_session`;
- `debug.toggle_breakpoint`, `debug.toggle_breakpoint_enabled` and
  `debug.remove_all_breakpoints`.

Also the floating toolbar (§5), the `debug_stopped` context key, and the F-key
table. Criteria:

- C1: each of D8's commands against the fake.
- C2: `f10` opens the menu bar on Linux with no session and steps over while
  stopped.
- C3: the toolbar's disabled states in Stopped, Resuming, Running and Uncertain.
- C4: the toolbar shows on Linux, which has no title row, and on macOS.
- C5 (Mac look): the toolbar screenshot.

**D9. Run and Debug container: Variables, Watch, Call Stack and Breakpoints.**
Size l. Depends on D7. Linux-OK, + Mac look.
Scope:

- `crates/fe-workbench/src/debug_panel*.rs`, `crates/fe-workbench/src/lib.rs`;
- `crates/fe-workbench/src/dock.rs`, `crates/fe-workbench/src/problems_panel.rs`
  (sharing its grouping and reveal), `crates/fe-workbench/src/sidebar_panels_tests.rs`;
- `crates/fe-workbench/src/debug_breakpoints*.rs` (the `watches` list);
- `crates/fe-workbench/src/debug_session*.rs` (exception filters and frame
  selection);
- `crates/fe-workbench/src/workbench_commands.rs` and the command-catalog tests.

The right-dock `ViewContainer` and its four views (§5):

- lazy expansion and paging;
- frame selection re-scoping Variables and Watch;
- Watch persisted in D5's file;
- the Breakpoints view in Problems' grouping, with exception filters re-sent
  through `setExceptionBreakpoints`;
- the rail button on the first session;
- commands `debug.frame_up`, `debug.frame_down` and `debug.show_panels`.

Criteria:

- C1: expanding a variable fetches its children once per stop.
- C2: selecting frame 2 opens its file and bands the row with the frame tint.
- C3: watch expressions re-evaluate on each stop and survive a restart.
- C4: toggling an exception filter re-sends `setExceptionBreakpoints`.
- C5 (Mac look): a screenshot.

**D10. Debug Console.**
Size m. Depends on D7. Linux-OK, + Mac look.
Scope: `crates/fe-workbench/src/debug_console*.rs`, `crates/fe-workbench/src/lib.rs`,
`crates/fe-workbench/src/terminal_panel*.rs`,
`crates/fe-workbench/src/workbench_commands.rs` and the command-catalog tests.

The console tab in the bottom panel: output rows by category, a one-line input
with history, `repl` evaluation with expandable result rows (the console's own
list, not a popover), and `debug.show_console`. Criteria:

- C1: output events append in order, with stderr tinted.
- C2: an evaluation's result row expands.
- C3: history recalls with up and down.
- C4 (Mac look): the console tab with mixed output.

**D11. Hover evaluation.**
Size s. Depends on D9. Linux-OK, + Mac look.
Scope: `crates/fe-workbench/src/debug_hover*.rs`, `crates/fe-workbench/src/lib.rs`,
the hover popover's host in `crates/fe-workbench/src/lsp*.rs` where it merges
hovers, `crates/fe-workbench/src/workbench_commands.rs` and the command-catalog
tests.

Hover with context `hover`, only where `supportsEvaluateForHovers`, above the LSP
hover, with the identifier's extent from the syntax tree. It renders as
`HoverContent` markdown: the type and value, and up to 20 first-level children
fetched once. No expansion inside the popover: fe-gpui's `HoverContent` is text
only (`crates/fe-gpui/src/hover.rs:72`), and the Variables view expands. Commands
`debug.hover` and `debug.evaluate`. Inline values are D18's. Criteria:

- C1: no `evaluate` is sent without the capability.
- C2: a hover's value from before a step is never shown after it (epoch-tagged,
  §2).
- C3 (Mac look): the popover with a value and its children.

**D12. LazyVim preset.**
Size s. Depends on D8, D9, D10 and D11. Linux-OK.
Scope: `crates/fastedit/presets/lazyvim.vimrc`, `crates/fastedit/src/vim_remaps*.rs`
and its preset tests.

§6's `<leader>d*` block for the commands D7 to D11 add. `<leader>dg` and
`<leader>dB` go in the unmapped list (D17 maps `<leader>dB`). `<leader>td` is D13's,
so the preset doesn't wait on Mac T1409. Criteria:

- C1: each key dispatches its command in a workbench test with `vim.preset =
  "lazyvim"`.
- C2: `<leader>do` steps out and `<leader>dO` steps over (LazyVim's meaning).

**D13. Debug Test at Cursor.**
Size m. Depends on D7 and D12, on **Mac T1409** ("Run Test at Cursor / Tests in
File"; its helium handle once cut back), and on **Mac T1410** for the lens item.
Linux-OK.
Scope:

- `crates/fe-workbench/src/debug_test*.rs`, `crates/fe-workbench/src/lib.rs`;
- `crates/fe-workbench/src/workbench_commands.rs` and the command-catalog tests;
- `crates/fastedit/presets/lazyvim.vimrc` and its preset tests (`<leader>td`);
- T1409's test-target module for `TestFramework` if it lacks it, and T1410's lens
  module.

`debug.test_at_cursor` consuming `ResolvedTest` (§7). A `GoTest` is refused with
"Go tests need delve" until D19.

- **Cargo.** Build through `--no-run --message-format=json` and take the test
  executable. Launch it with cwd set to the directory of the artifact message's
  `manifest_path` (where cargo itself runs a test binary) and
  `CARGO_MANIFEST_DIR` set to it, not with `ResolvedTest.cwd`, which is where
  `cargo test` is invoked.
- **pytest.** Run through debugpy's `module = "pytest"`.

Also a Debug item on the Run Test lens, and `<leader>td` in the preset.
Criteria:

- C1: the cursor in a Rust test of a workspace member launches that test binary
  with `[name, "--exact", "--nocapture"]`, cwd at the member's directory, and
  `CARGO_MANIFEST_DIR` set.
- C2: the same for pytest's node id.
- C3: Run and Debug resolve the same test from one `ResolvedTest`.
- C4: `<leader>td` dispatches `debug.test_at_cursor` under the LazyVim preset.

**D14. Adapter install flow.**
Size l. Depends on D3 and D15 (pending Q1). Linux-OK, + Mac look for the prompt.
Scope: `crates/fe-workbench/src/debug_install*.rs`, `crates/fe-workbench/src/lib.rs`,
`crates/fe-net/src/**` (if a fetch helper is needed),
`extensions/debug-adapters/pack.toml`.

The toast offer, hash-pinned upstream fetches of the codelldb `.vsix` and the
debugpy wheel, staging and rename as `tenmoku.debug_<name>` user extensions,
quarantine handling on macOS, and the failure messages, all following
`c1pt00h80c-grammar-fetch.md`. Criteria:

- C1: a wrong hash writes nothing.
- C2: the installed extension is listed and its adapter resolves.
- C3: nothing downloads without the click.

**D15. `contributes.debug_adapters` extension kind.**
Size m. Depends on D3 and the user's answer to Q1. Linux-OK.
Scope: `crates/fastedit/src/config.rs`, `crates/fe-workbench/src/lsp_consent*.rs`
(generalised to adapters) or a new `crates/fe-workbench/src/debug_consent*.rs`,
`crates/fe-workbench/src/lib.rs`, `crates/fe-dap/src/registry*.rs`.

The schema, consent bound to the invocation exactly as `lsp`'s, workspace
extensions only after trust, and tests copied from `extension_servers_tests.rs`.
Criteria:

- C1: an ungranted adapter never launches.
- C2: a changed invocation asks again.

**D17 (after the MVP). Conditional breakpoints, hit counts and logpoints.**
Size m. Depends on D6, D7, D10 and D12. Linux-OK, + Mac look.
Scope:

- `crates/fe-workbench/src/debug_breakpoints*.rs`,
  `crates/fe-workbench/src/debug_session*.rs` (building each `SourceBreakpoint`);
- `crates/fe-gpui/src/breakpoint_gutter*.rs`;
- `crates/fe-workbench/src/workbench_commands.rs` and the command-catalog tests;
- `crates/fastedit/presets/lazyvim.vimrc`, `crates/fastedit/src/vim_remaps*.rs` and
  its preset tests.

`debug.conditional_breakpoint` and `debug.log_point`, the context menu's Edit
Condition, Edit Hit Count and Edit Log Message prompts, the D17 glyph rows of §5's
table, and `<leader>dB` in the preset. Each field is sent only with its
capability:

- `condition` with `supportsConditionalBreakpoints`;
- `hitCondition` with `supportsHitConditionalBreakpoints`;
- `logMessage` with `supportsLogPoints`.

A field the adapter can't take shows the breakpoint unverified, with a message.
Logpoint output goes to the console. Criteria:

- C1: each field is sent only with its capability.
- C2: logpoint messages appear in the console.
- C3: `<leader>dB` prompts and sets a conditional breakpoint.
- C4 (Mac look): the conditional and logpoint glyphs.

**D18 (after the MVP). Inline values.**
Size m. Depends on D9 and D11. Linux-OK, + Mac look.
Scope: `crates/fe-workbench/src/debug_inline_values*.rs`,
`crates/fe-workbench/src/lib.rs`, `crates/fe-workbench/src/debug_session*.rs` (the
resume hook that retires them), `crates/fe-workbench/src/workbench_root.rs`
(`reload_settings`), `crates/fe-config/src/settings.rs` and
`crates/fe-config/src/tests/**`.

§5's `AfterLine` values from the fetched locals and the syntax tree, retired with
the epoch on resume, and the `debug.inline_values` setting. Criteria:

- C1: values show only up to the stopped row and only for identifiers on that row.
- C2: they disappear when execution resumes.
- C3: setting `debug.inline_values = false` in user settings removes them without
  a restart.
- C4 (Mac look): their styling.

**D19 (after the MVP). delve and Go.**
Size m. Depends on D3, D4, D13 and D16. Linux-OK (its real test needs
`ptrace_scope` ≤ 1).
Scope: `crates/fe-dap/src/registry*.rs`, `crates/fe-dap/tests/real_adapter.rs`,
`scripts/dap-real.sh`, `.github/workflows/dap.yml` (installing delve),
`crates/fe-workbench/src/launch_defs*.rs`, and D13's
`crates/fe-workbench/src/debug_test*.rs` (its dispatch and tests).

The delve spec over TCP tested end to end, `go.mod` detection ("Debug package"),
and `TestFramework::GoTest` in D13's `debug.test_at_cursor` dispatch, replacing
D13's "Go tests need delve" refusal. Criteria:

- C1: the delve spec's TCP arguments launch D1's `fe-dap-fake` through
  `LocalLauncher` with `${port}` substituted, standing in for `dlv`.
- C2: a `go.mod` root lists "Debug package".
- C3: a resolved `GoTest { package_dir, name }` launches delve with
  `mode = "test"`, `program = <package_dir>` and
  `args = ["-test.run", "^<name>$"]`, against the fake.
- C4: a real delve test in the slow group, skipped by `ptrace_scope` where
  forbidden.

**D20 (after the MVP). Attach.**
Size m. Depends on D4 and D7. Linux-OK.
Scope: `crates/fe-workbench/src/launch_defs*.rs`,
`crates/fe-workbench/src/debug_session*.rs`,
`crates/fe-workbench/src/process_picker*.rs`, `crates/fe-workbench/src/lib.rs`.

`request = "attach"` by `pid`, then `pid = "pick"` with a process picker and
`${command:pickProcess}` from `launch.json`. Also the ptrace toast for attach under
`ptrace_scope = 1`. Criteria:

- C1: an attach configuration starts against the fake.
- C2: the picker lists the user's processes.
- C3: an attach refused by ptrace names `ptrace_scope`.

**Order:**

```text
MVP:   D1 → D2, D3 → D16 → D4 → D5 → D6 → D7 → D8 → D9, D10 → D11 → D12 → D13
Later: D17, D18, D19, D20; D15 (after Q1) → D14
```

D13 is last because it also waits on Mac T1409. Nothing else does.

**Which handle Debug Test at Cursor waits on:** "run test at cursor" is **Mac
T1409** (with **Mac T1410** for the Run Test lens). D13 names T1409's helium
handle once it is cut back.

**Decision.**

- After the user approves this record, file D1 to D13 and D16, the MVP series and
  exactly §9's MVP list, in the order above.
- D17 to D20 are filed after the MVP lands.
- D14 and D15 wait on Q1.

## 12. Open questions for the user

1. **Q1. Extension-contributed adapters.** Allow a stage-1
   `contributes.debug_adapters` kind with `lsp`'s consent rules, amending the
   extensions record's §8? Recommended: yes. Without it, D14 needs its own managed
   install directory and management UI.
2. **Q2. Launch file.** `.tenmoku/launch.toml` (recommended, §4), or a
   `[[debug]]` table in `.tenmoku/tasks.toml`?
3. **Q3. Default Rust adapter.** lldb-dap when found, else codelldb (recommended),
   or codelldb first for its built-in Rust formatting?
4. **Q4. F10 and F11 on Linux.** Claim them only while stopped (recommended, §6),
   or always, which would move the menu bar to `alt` alone and fullscreen to
   another key?
5. **Q5. Real native adapters on helium.** `ptrace_scope` is 2 there, so native
   real-adapter tests can't run on helium. Accept that they run only on a GitHub
   runner and on the Mac (recommended), or lower helium's `ptrace_scope` to 1?

## 13. The user's ruling (2026-11-13)

Approved by the user on 2026-11-13, with these amendments.

### Open questions

Q1-Q5 take the recommended answers: extensions may contribute debug adapters
under `lsp`'s consent rules (Q1); `.tenmoku/launch.toml` (Q2); lldb-dap when found,
else codelldb (Q3); on Linux F10 and F11 step only while stopped (Q4); real native
adapter tests run on a GitHub runner and on the Mac only (Q5).

### Scope

- **Java joins the MVP** (new card D21): java-debug through jdtls (built in since
  T1658). jdtls starts with the java-debug bundle, Tenmoku asks jdtls over LSP
  (`vscode.java.startDebugSession`) for the adapter's port and connects to it;
  main-class and classpath resolution also go through jdtls. launch.json import
  gains the `java` type. Gradle and Maven tasks (T1660) serve as pre-launch tasks.
  This is a new adapter launch kind (a server-provided port) that crosses into
  `fe-lsp`; D3's registry gets a `Port` launch alongside `Stdio`.
- **C# later** (new card D22, not MVP): netcoredbg (MIT) as a built-in adapter for
  launch.json's `coreclr` type, program = the built DLL. vsdbg is ruled out by name
  (its licence allows only Microsoft tools). Debugging a C# test attaches to the
  test host (`VSTEST_HOST_DEBUG`), so it waits on D20 and .NET test detection.
- **TypeScript later** (new card D23, first after the MVP): multi-session support
  plus js-debug.

### B3 design first

A design card (D0) fits the debugger to the B3 design system
(`docs/design/design-system.md`, where the debugger is still to design) before the
UI cards: D6, D8, D9, D10 and D11 depend on it; the engine cards (D1-D5, D7, D16,
D21) do not. D0 settles, with artboards and the design-system entries:

- B3 has no separate right dock (the rail defaults to the left and panels open on
  its side): the debugger's Variables, Watch, Call Stack and Breakpoints become a
  Debug tool window in the bottom panel, its icon in the rail's bottom group next
  to Runs, instead of §5's right-dock container.
- The B3 gutter is a fixed 81 px pinned by tests: breakpoints draw over the
  line-number column instead of in a new lane.
- The title bar's run widget (T1539) gets its Debug button and lists launch
  configs beside tasks; §5's floating toolbar stays where there is no title bar.
- A debug session shows in the Runs panel (T1570, T1571) and the status bar's
  operations list (T1585); the test lens reads `▷ Run test · Debug` (`test_lens.rs`)
  and test failure detail's "Debug this test" (T1590) opens D13.
- The new tints get theme roles (`roles.rs`) and every theme's values.

### Review findings folded into D2

D2 (session model) carries T486's three open findings as criteria:

- F2: after a failed or cancelled step, continue or similar request, the session
  stays Uncertain until a stop is proven (a `stopped` event or a successful
  `threads`/`stackTrace` that the adapter answers while stopped), handles the
  adapter's `notStopped` error, and never fetches stack or variables while the
  debuggee may run.
- F10: `configurationDone` has a timeout; on expiry the session ends with a named
  error instead of waiting forever.
- F11: after Stop, late `stopped`/`continued` events never re-enable the controls.

### Amendments from landed work

- Adapters on a bare command resolve on the login shell's PATH as language servers
  do (`fe-lsp/src/launcher.rs` LoginPath), not "app PATH plus mise shims".
- T1409 landed as `test_runnables.rs` and `test_commands.rs` without a resolved
  test type, for Rust, Go and Python only: D13 adds that seam there (a resolved
  test target with program, args and cwd that Run and Debug both consume).
  T1573 is the fe-runlog cargo-test parser, not run configs.
- Every card adds its strings to a `debug.ftl` catalogue (l10n lint), gives its
  controls accessible names and announces session changes ("Stopped at line N").
- New tests follow today's placement (`src/tests/<name>.rs`, or a new module
  beside the code for new-API tests).
