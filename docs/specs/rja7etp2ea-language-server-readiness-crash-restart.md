---
task_uid: "01m49phep3svtdw7rja7etp2ea"
task: "T142"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::readiness::progress_mode_never_trusts_quiet: in progress mode, for a first progress beginning 700 ms and separately 15 s after initialize, for gaps of 900 ms and of 15 s between progress phases, and for indexing with no progress at all, an empty definition reply during the fake's scripted indexing is marked not final and a non-empty reply is returned; Ready latches 500 ms after the last open token ends, ignoring tokens that match ready_ignores_progress; after 60 s of quiet an empty reply is still not final; all on the injected clock"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::readiness::server_status_mode: Ready comes only at the first experimental/serverStatus with quiescent true, never from progress, including when that status arrives after the progress rule would have latched; quiescent false after true makes empty replies not final again while Ready stays latched; with no status 10 s after initialize the server routes by the progress rule, logs the fallback, and its empty replies are never final"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::readiness::first_request_after_ready: the first definition request after Ready returns the fake's exact non-empty reply"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::restart::rolling_window_backoff: crashes at 0, 1, 3, 7, 15 and 31 s give Restarting(1..5) with restarts 1, 2, 4, 8 and 16 s later, then Stopped at the 6th; a crash every 2 minutes always restarts after 1 or 2 s and never stops; crashes spaced just over 3 minutes apart never raise n above 1; LspManager::restart clears the list and starts the server from Stopped"
    required: true
    manual: false
  - id: "C5"
    text: "fe-lsp test tests::restart::restart_is_a_new_session: after the fake closes its transport, pending requests fail with ServerDown, a new initialize runs with a new session id, every open document is re-sent with didOpen, its current text and version 0, readiness is Starting again, and the old session's identities are retired; an initialize failure and an initialize timeout each count as a crash"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test tests::cancel::drop_and_timeout_cancel: dropping a definition future sends $/cancelRequest with the mirrored id; a request past its timeout (from lsp.timeouts, e.g. definition 5 s on the injected clock) answers Timeout at once and sends $/cancelRequest; a late reply after either reaches no caller; replies with -32800, -32801 and -32802 become Cancelled, Retry and Retry"
    required: true
    manual: false
  - id: "C7"
    text: "fe-lsp test tests::cancel::orphan_budget: with a fake that answers cancelled requests, the orphan count returns to 0; with a fake that never answers them, the 65th abandoned request recycles the session (shutdown, kill, new initialize), counting as a crash, and over 3 such rounds the orphan count never exceeds 64"
    required: true
    manual: false
  - id: "C8"
    text: "fe-lsp test tests::registration::dynamic_registration_policy: two workspace/didChangeConfiguration registrations A and B are accepted; a settings change sends didChangeConfiguration with both, after unregistering A, after unregistering B too, and with no registration at all; unregistering an unknown id succeeds and changes nothing; a didChangeWatchedFiles registration, alone or with a didChangeConfiguration one in the same request, gets error -32803 naming the unsupported method and records nothing"
    required: true
    manual: false
  - id: "C9"
    text: "fe-lsp test tests::status::status_and_log_events: the sink gets Starting, then Indexing with the progress title while a token is open, then Ready; Restarting(2) after a second crash; Stopped with \"rust-analyzer not found\" when the launcher returns NotFound; LspManager::log returns the fake's stderr lines and window/logMessage lines, keeping only the last 64 KiB; window/showMessage arrives as LspEvent::ShowMessage"
    required: true
    manual: false
  - id: "C10"
    text: "Manual: with the rust-analyzer rustup component on this repository, record whether experimental/serverStatus arrives with the opt-in, whether quiescent waits for cache priming and ignores flycheck, and whether rust-analyzer notices a file changed on disk without didChangeWatchedFiles; if either serverStatus check fails, the built-in rust-analyzer spec uses progress mode; the findings are written into §10 of docs/decisions/lsp-client.md"
    required: true
    manual: false
  - id: "C11"
    text: "scripts/check-no-gpui.sh prints ok: fe-lsp; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
  - id: "C12"
    text: "fe-lsp test tests::restart::exit_with_transport_open: when the fake's process exit resolves while its stdout stays open, both during initialize and after Ready, the session ends with an error naming the exit within 1 s on the injected clock, pending requests fail with ServerDown, and the exit counts as a crash (Restarting(1)), not a timeout"
    required: true
    manual: false
  - id: "C13"
    text: "fe-lsp test tests::cancel::fake_records_client_responses: when the fake sends a server-initiated request (e.g. workspace/configuration or client/registerCapability) and the client answers with a result or an error, records() contains that response with its wire id and launch identity"
    required: true
    manual: false
  - id: "C14"
    text: "fe-lsp test tests::cancel::drains_do_not_starve_queued_commands: with a stepped writer that frees one slot per backstop interval and edits plus clock ticks continuing throughout, a definition request and a didClose queued before the edits are both sent within 3 backstop intervals"
    required: true
    manual: false
---

# Language server readiness, crash restart with a rolling budget, request timeouts and cancellation, and status events

# Language server readiness, crash restart with a rolling budget, request timeouts and cancellation, and status events

Card 4 of `docs/decisions/lsp-client.md` §8. The design is:

- §3.4 "Readiness" and "Empty answers"
- §3.5 (crashes, cancellation, the orphan budget, timeouts, what the user sees)
- §3.6 "Dynamic registration" and "Notifications from servers"
- §2.1 gaps 2 and 3

It builds on cards 1–3. Every timer uses card 1's injected `Clock`, so these tests run in
milliseconds.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q2 (a):** a server that isn't installed shows "stopped: <name> not found". There is no download offer.
- **Q3 (a):** timers are `async_io::Timer` behind the `Clock`; no tokio.

## Change

- **`src/readiness.rs` (new).** The spec's `ReadinessMode` decides readiness before startup.
  - **`server-status`.** `Ready` at the first `experimental/serverStatus` with `quiescent: true`.
    Progress never makes the server ready. With no status 10 s after `initialize`, fall back to the
    progress rule for routing, and log it.
  - **`progress`.** `Ready` once `initialize` has answered and no work-done token has been open for
    500 ms. Tokens matching `ready_ignores_progress` don't count, and begin/end pairs are matched
    per occurrence.
  - **Latching.** `Ready` latches until a restart.
  - **Empty answers.** Every reply carries whether an empty result is **final**. That is true only
    in `server-status` mode with the latest status `quiescent: true` and no fallback. Card 6 maps a
    non-final empty answer to `Unavailable`.
- **`src/restart.rs` (new).** A crash is:
  - an exit or EOF while not shutting down
  - an initialize failure or timeout
  - a main-loop error

  On a crash:
  - pending requests fail with `ServerDown`
  - the crash joins a 3-minute rolling list, and *n* is the number left in it
  - *n* ≤ 5 gives `Restarting(n)` after 1, 2, 4, 8 or 16 s
  - *n* = 6 gives `Stopped(error)` until `LspManager::restart(server, root)` (which clears the list)
    or a spec change

  A restart is a new session (§3.5):
  - `initialize` runs again
  - every open document gets `didOpen` with its current text, at version 0
  - readiness returns to `Starting`
  - card 2b's identity table retires the old session first
- **`src/requests.rs` (new).** Per-method timeouts come from card 3's `lsp.timeouts`.
  - **Dropped or timed out.** Dropping a request future, or reaching its timeout, sends
    `$/cancelRequest` with the mirrored id and answers `Cancelled` or `Timeout` at once.
  - **Orphans.** The async-lsp future moves to an orphan `FuturesUnordered`. More than 64 orphans
    recycle the session: shutdown with a 2 s timeout, kill, restart. That counts as a crash.
  - **Error codes.** -32800 maps to `Cancelled`. -32801 and -32802 map to `Retry`.
- **`src/registration.rs` (new).** `client/registerCapability` accepts only
  `workspace/didChangeConfiguration`, recorded by registration id. Any other method fails the whole
  request with -32803, which names the methods, and records nothing. `unregisterCapability`
  removes ids, and an unknown id is a no-op. `didChangeConfiguration` is sent whatever is
  registered.
- **Status and logs** (`src/event.rs`, `src/server.rs`).
  - `ServerStatus` gains `Indexing { title }`, `Ready` and `Restarting(n)`.
  - The launch error `NotFound` reads "<name> not found".
  - The last 64 KiB of each server's stderr and `window/logMessage` go into a ring, read with
    `LspManager::log(server, root)`.
  - `window/showMessage` is emitted as `LspEvent::ShowMessage`.
  - `$/progress` and `window/workDoneProgress/create` are tracked.
- **Real rust-analyzer check (manual; §10).** Run the rustup component (`rustup component add
  rust-analyzer`) on this repository, with the opt-in, through a scratch harness (not committed). Check:
  1. that `experimental/serverStatus` arrives
  2. whether `quiescent` waits for cache priming and ignores flycheck
  3. whether rust-analyzer watches the disk itself without `didChangeWatchedFiles` (§7)

  If (1) or (2) fails, switch the built-in rust-analyzer spec in `src/registry.rs` to `progress`.
  Record the findings in §10 of `docs/decisions/lsp-client.md`.

## Size

About 850–950 non-test lines estimated. If it runs over the cap, move dynamic registration and the
log ring into a follow-on card.

## Out of scope

- The navigation provider (card 6).
- The status bar item, `lsp.restart` and `lsp.show_log` commands (card 5).
- Diagnostics (card 7).

## Folded review findings (from T163 and T164, abandoned in favour of this task)

These medium findings from the reviews of T138 and T139 touch the same files as this task, so they are fixed here rather than in separate follow-ups.

- **T138 F2 (server.rs): server exit is not monitored.** `ServerProcess::exited` is polled only during shutdown. If the launched process exits while a descendant or proxy keeps stdout open, startup reports a timeout instead of the exit, and an initialized session stays active indefinitely. Monitor `exited` during initialization and normal operation: end the session with an error naming the exit, fail pending requests with ServerDown, and count it as a crash for the restart policy.
- **T138 F3 (tests/fake.rs): the fake does not record responses to its server-initiated requests.** Record incoming successful and error responses with their wire ids and launch identity, so `records()` is the complete message record the fake promises.
- **T139 F3 (server.rs): backstop drains can starve queued commands.** `next_command` always prefers a due drain over the backlog and the command receiver, so with continuing edits and a slow writer freeing one slot per backstop interval, an already queued request, open or close can wait forever. Schedule coalesced drains through the ordered command queue, or otherwise guarantee queued commands progress between drains.
