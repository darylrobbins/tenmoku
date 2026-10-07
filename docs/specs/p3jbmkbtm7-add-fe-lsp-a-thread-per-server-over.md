---
task_uid: "01m49phbzptd8rjap3jbmkbtm7"
task: "T138"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::lifecycle::initialize_advertises_m4_capabilities: through FakeLauncher, the fake records one initialize carrying processId, rootUri and workspaceFolders equal to the single root, clientInfo.name \"fastedit\", the spec's initializationOptions, general.positionEncodings [\"utf-8\",\"utf-16\"], window.workDoneProgress, workspace.configuration, workspace.workspaceFolders and experimental.serverStatusNotification true, and every dynamicRegistration field set to false; initialized follows the reply; the negotiated encoding is recorded (utf-8 when the fake picks it, utf-16 when the field is absent); the server runs on a thread named \"fe-lsp \u003cserver name\u003e\""
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::lifecycle::shutdown_sequence: stopping a server sends shutdown, then exit after the reply, then waits for ServerProcess::exited and calls kill only if it hasn't resolved within 1 s; a fake that never answers shutdown gets exit after 2 s; all waits use the injected Clock; a non-zero exit code or Eof after exit ends in Stopped(None), not an error"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::lifecycle::initialize_failure_stops_the_server: an initialize error, an initialize left unanswered for 30 s on the injected Clock, and a transport EOF before the reply each end in LspEvent::Status Stopped(Some(error)) naming the cause, and the server thread exits"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::router::unknown_notifications_do_not_stop_the_loop: after the fake sends telemetry/event, custom/unknown and $/typescriptVersion, each is logged and a request sent afterwards still gets its reply"
    required: true
    manual: false
  - id: "C5"
    text: "fe-lsp test tests::router::server_requests_get_the_m4_answers: per docs/decisions/lsp-client.md §3.6, window/workDoneProgress/create gets null, workspace/workspaceFolders gets the single root, workspace/applyEdit gets applied false with failureReason \"not supported\", window/showMessageRequest gets null, window/showDocument gets success false, workspace/diagnostic/refresh, workspace/semanticTokens/refresh, workspace/inlayHint/refresh and workspace/codeLens/refresh get null, and an unknown request gets MethodNotFound"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test tests::router::mirrored_request_ids_match: for 20 requests issued through the actor, some answered by the fake out of order, the id the actor mirrored for each equals the id the fake received"
    required: true
    manual: false
  - id: "C7"
    text: "fe-lsp test tests::launcher::local_launcher_kills_the_process_group: LocalLauncher launching /bin/sh with a script that starts a background sleep 60 and then copies stdin to stdout echoes bytes written to input on output, runs with cwd equal to the root and the spec's env set, and kill() ends both the shell and the sleep (its pid is gone within 1 s); a command that doesn't exist returns io::ErrorKind::NotFound"
    required: true
    manual: false
  - id: "C8"
    text: "fe-lsp test tests::launcher::launcher_errors_stop_the_server: a launcher returning Err marks the server Stopped(Some(message)) with that error's message, starts no main loop and sends nothing; the launcher is called on the server's own thread, not the caller's"
    required: true
    manual: false
  - id: "C9"
    text: "LspManager::new takes an Arc\u003cdyn ServerLauncher\u003e and an FnMut(LspEvent) + Send + 'static sink; ServerSpec, ServerIo, ServerProcess, ServerLauncher and LocalLauncher are pub; LspManager and its request futures are Send; no pub item's signature names an async_lsp type; the crate docs state the two embedding facts of docs/decisions/lsp-client.md §2.2"
    required: true
    manual: false
  - id: "C10"
    text: "crates/fe-lsp/Cargo.toml pins async-lsp =0.2.4 (default-features = false, omni-trait) and lsp-types =0.95.1 through workspace entries; root Cargo.toml and Cargo.lock change only by added lines; Cargo.lock gains exactly the packages fe-lsp, async-lsp 0.2.4, lsp-types 0.95.1, tower-layer 0.3.3 and tower-service 0.3.3; cargo tree -p fe-lsp -e normal -i tokio prints nothing; scripts/check-no-gpui.sh prints ok: fe-lsp; the gate's deny stage passes with deny.toml unchanged"
    required: true
    manual: false
---

# Add fe-lsp: a thread per server over async-lsp, a host-replaceable ServerLauncher, initialize and shutdown, and a fake server

# Add fe-lsp: a thread per server over async-lsp, a host-replaceable ServerLauncher, initialize and shutdown, and a fake server

Milestone M4 "Navigate", second half: the first `fe-lsp` card (card 1 of
`docs/decisions/lsp-client.md` §8). The design is that document. Read §1, §2 (all of it), §3.1, §3.4
("Start" and "Shutdown" only), §3.6 and §6.1 before starting. This card creates the crate and
its plumbing: no document sync, readiness, restart or settings. Cards 2–4 add those on top of the module layout
below, so keep it.

User decisions this card depends on (`docs/decisions/lsp-client.md` §9, "Decided (user, 2026-10-06)"):

- **Q1 (a):** `LspManager` takes an `Arc<dyn ServerLauncher>`. `LocalLauncher` lives in `fe-lsp`,
  and a host may pass its own.
- **Q3 (a):** no tokio. async-lsp runs on `async-io` and `async-process`, on a thread per server
  (§2.2).
- **Q7 (c):** a launcher returning `Err` is how a host refuses a server (§3.1). Standalone fastedit
  starts servers.

## Change

### Dependencies

- Root `Cargo.toml` `[workspace.dependencies]`, **additions only**: the root `Cargo.toml` and `Cargo.lock` are
  `shared_append_only` in `regatta.toml`, so this card doesn't claim them. The additions:
  - `fe-lsp`
  - `async-lsp = { version = "=0.2.4", default-features = false, features = ["omni-trait"] }` (add
    `"tracing"` only if used)
  - `lsp-types = "=0.95.1"`
  - `async-io = "2"`, `async-process = "2"` and `futures = "0.3"`: already in the lock, as §2.2 lists
  - `rustix` (already in the lock) for `killpg`, if needed
- `crates/fe-lsp/Cargo.toml` uses those, plus `serde`, `serde_json`, `log` and `thiserror`.
- It also declares `fe-text`, `fe-buffer` and `fe-config` now, though cards 2a, 2b and 3 are the first to use
  them. That keeps `fe-lsp`'s `Cargo.lock` entry from changing in those cards, while T107 and T108
  hold `Cargo.lock` in their scopes.
- `Cargo.lock` gains exactly five packages: `fe-lsp`, `async-lsp`, `lsp-types`, `tower-layer` and
  `tower-service` (§2.1).
- No tokio, smol or other executor.
- `deny.toml` is unchanged: every licence is on the allowlist (§2.5).

### Module layout (later cards name these files)

| File | Holds |
|---|---|
| `src/lib.rs` | crate docs (including §2.2's two embedding facts), `mod` lines, re-exports |
| `src/launcher.rs` | `ServerSpec`, `ServerIo`, `ServerProcess`, `ServerLauncher`, `LocalLauncher`, `ExitInfo` (§3.1 as written; `ReadinessMode` and `UnversionedDiagnostics` exist with their defaults, but card 4 and card 7 give them behaviour) |
| `src/manager.rs` | `LspManager`: `new(launcher, sink)`, starting a server for a spec and root, stopping it, and the request entry point the actor serves |
| `src/server.rs` | one thread per server, named `fe-lsp <name>`, running `async_io::block_on(join(main_loop, actor))`; the actor owns pending requests and mirrors async-lsp's request ids (§2.1 gap 2 (a)) |
| `src/router.rs` | the client `Router`: `unhandled_notification` logs and continues (§2.1 gap 1); the §3.6 fixed answers |
| `src/clock.rs` | a `Clock` trait: a real one on `async_io::Timer`, and a manual one for tests. Every timer in the crate goes through it, so card 4's backoff tests can use it |
| `src/event.rs` | `LspEvent`, `ServerStatus` (`Starting`, `Stopped(Option<String>)` here; card 4 adds the rest), `LspError` (`ServerDown`, `Cancelled`, `Timeout`, `Unavailable(String)`, `Retry`, …) |
| `src/tests/mod.rs` + one file per area | unit tests (`tests::<module>::<fn>`) |
| `src/tests/fake.rs` | the fake server of §6.1 |

### Lifecycle (§3.4 "Start" and "Shutdown")

- **`initialize`.** It carries the parameters and capabilities §3.4 lists:
  - `processId`, `rootUri` and `workspaceFolders` (the single root)
  - `clientInfo.name = "fastedit"`
  - the spec's `initializationOptions`
  - `general.positionEncodings = ["utf-8", "utf-16"]`
  - `window.workDoneProgress`
  - `workspace.configuration` and `workspace.workspaceFolders`
  - `experimental.serverStatusNotification`
  - every `dynamicRegistration` set to `false` (§3.6)
- **After the reply.** Record the server capabilities and the negotiated encoding (absent means
  utf-16), then send `initialized`.
- **Startup timeout.** 30 s on the clock.
- **Shutdown.** `shutdown` (2 s timeout), then `exit`, then up to 1 s for `ServerProcess::exited`,
  then `kill`. A non-zero exit code, or async-lsp's `Err(Eof)`, after `exit` is a normal stop: typescript-go exits 1.
- **Failures.** In this card, an initialize error, an initialize timeout or an early EOF ends in
  `Stopped(Some(error))`. Card 4 turns these into crash restarts.

### `LocalLauncher` (§3.1)

- Build a `std::process::Command` with `CommandExt::process_group(0)` and convert it to
  `async_process::Command`.
- Piped stdio, `cwd` = root, the spec's env, `kill_on_drop`.
- `kill` signals the whole group.
- Bare-command `PATH` resolution through the login shell is card 3.

### The fake server (§6.1), at `src/tests/fake.rs`

- It is compiled as `#[cfg(any(test, feature = "test-support"))] #[path = "tests/fake.rs"] pub mod
  fake;`, so fe-workbench can use it from card 5 through a `test-support` feature. Tests reach it as
  `crate::fake`, and `tests/mod.rs` doesn't declare it a second time.
- It is built on async-lsp's server side over an in-memory duplex, with no subprocess.
- `FakeLauncher` returns that duplex as `ServerIo`.
- In this card it needs:
  - a scripted `InitializeResult`, an initialize delay or failure
  - a record of every message with its request id
  - scripted replies and delays for a generic request
  - server-initiated requests and notifications
  - crash on cue (EOF, or a non-zero exit after `exit`)
- Cards 2–9 extend it: the document replica, progress, diagnostics and so on.

## Out of scope

- Document sync and encodings (cards 2a and 2b).
- Settings, built-in specs and lazy start (card 3).
- Readiness, restart, timeouts other than initialize and shutdown, `$/cancelRequest`, dynamic
  registration, status text and the log ring (card 4).
- Any UI.

## Decided (lead defaults; the user may revisit)

- The fake lives under `src/tests/`, behind a `test-support` feature, so it stays out of the
  1,000-line count and can be shared with fe-workbench.
- `fe-text`, `fe-buffer` and `fe-config` are declared in this card, to avoid a `Cargo.lock` overlap
  later (see above).
