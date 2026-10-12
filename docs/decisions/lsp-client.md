# fe-lsp client: crate, server lifecycle, document sync and test servers

Status: **proposed.** The user rules on it before any `fe-lsp` card is
written. Researched 2026-10-06 against fastedit `f577369`. Source card T100
(M4 "Navigate", second half).

**What was read.**

- **Crates**, from crates.io tarballs:
  - `async-lsp` 0.2.4
  - `lsp-types` 0.95.1, 0.96.0 and 0.97.0
  - `tower-lsp` 0.20.0 and `tower-lsp-server` 0.23.0
  - `lsp-server` 0.10.0
  - `helix-lsp` 0.0.0 (the crates.io placeholder)
- **Repositories:**
  - Lapce at `f66ffaa95387794205c35864517de70958b9ea8d` (2026-09-30), `lapce-proxy`
    only.
  - The gpui-kit copy at `/tmp/gpui-kit` (crate `gpui-base` 0.7.0). It isn't a git
    checkout, so it has no commit hash. Our `THIRD_PARTY.md` rows cite
    `2c5162f8`.
  - helix at `ba40e547426b0f9896c8bdc699a4ab11f2b37dbc`, `helix-lsp/Cargo.toml`
    only. No helix source was opened, and nothing from it shapes this design.
  - Server sources at the versions spiked, for the lines cited in §4:
    rust-analyzer tag `2026-10-05` (commit `65ac641199d5`), pyright 1.1.414,
    typescript-language-server 6.0.1 and typescript-go 7.0.2.
- **Licences** come from each crate's `Cargo.toml` `license` field and its
  `LICENSE*` files.
- **Release cadence and issue counts** come from the crates.io API and
  `gh api` on 2026-10-06.
- **Zed's GPL crates** (`lsp`, `project`, `language`, `editor` …) were not opened.

**Spikes.** All of them ran outside this repository, in a scratch directory,
and none were merged:

1. **Lock probe.** A copy of this workspace with a throwaway GPUI-free member
   that depends on `async-lsp`. Diffing its `Cargo.lock` against ours gives the
   lock and licence numbers in §2.
2. **Server probe.** A 249-line binary using `async-lsp` 0.2.4, `lsp-types`
   0.95.1, `async-process` 2.5.0 and `async-io` 2.6.0, with no tokio. For each
   of the four servers in §4 it ran with three `positionEncodings` offers:
   - started the server on a tiny project
   - printed the negotiated `positionEncoding`
   - logged every server→client message with timestamps
   - asked `textDocument/definition` across emoji, ZWJ and combining-mark lines
   - shut the server down

   The four servers were rust-analyzer, pyright, typescript-language-server
   and typescript-go. Raw logs were kept with the spike, not in the repo.

**Paths.** `fastedit/…` or bare `crates/…` paths are this repo at `f577369`.
Paths of the form `async-lsp/src/…` or `lsp-types/src/…` are inside that crate's
tarball at the version named.

## 1. Summary and recommendation

**Client crate and runtime (§2).** Keep the plan's **`async-lsp` 0.2.4**,
pinned with `=`, with **`lsp-types` 0.95.1**, the version async-lsp requires.
Both are MIT-licensed or dual MIT/Apache. With `default-features = false,
features = ["omni-trait"]` they add 4 crates to `Cargo.lock` and **no tokio**.

`fe-lsp` runs each server on its own thread, with `async-io`'s `block_on`.
Process stdio comes from `async-process`. Both crates are already in our lock
through GPUI's Linux backend, so on macOS they add compile time but no lock
entries. Results leave `fe-lsp` in two ways, neither of them GPUI-specific:

- request futures backed by `futures` oneshots
- an `FnMut(LspEvent) + Send` sink for notifications, the pattern
  `fe_project::ProjectWatcher` already uses

async-lsp has gaps we fill ourselves:

- client-side `$/cancelRequest`
- timeouts
- a router that by default stops on an unknown notification

If the cancellation workaround proves brittle, a ~500-line hand-written client
can replace async-lsp behind the same `fe-lsp` API. No dependency licence falls
outside `deny.toml`'s allowlist.

**Server lifecycle (§3).**

- **Launch seam.** Rename the plan's `fe-lsp` "`Spawner`" to **`ServerLauncher`**.
  It is unrelated to `fe-syntax`'s job `Spawner`. A host implements it to hand
  fastedit a server's byte streams, and **`LocalLauncher`** spawns a local
  process.
- **Instances.** Run one server instance per (server, workspace root). Start it
  lazily, when the first document of its language opens.
- **Readiness.** Each server spec fixes its readiness mode before startup.
  - rust-analyzer uses its authoritative `serverStatus` (`quiescent: true`),
    which we opt into. The probe didn't opt in, so this is unverified (§10).
  - Other servers are "ready" once `initialize` has answered and no work-done
    progress has been open for 500 ms.
  - An **empty** answer is final only while a server in `server-status` mode
    reports `quiescent: true`. That is a server-specific completion signal,
    and card 4 verifies it.
  - Otherwise an empty answer falls through to search: always in `progress`
    mode, however long the server has been quiet. No quiet-period timer ever
    makes an empty answer final.
- **Crashes.** Restart with a backoff of 1, 2, 4, 8 and 16 s, by the number of
  crashes in a rolling 3-minute window. Give up when a 6th crash lands inside
  the window. On restart, re-open every open document.
- **Requests.** Each request has a per-method timeout. When one times out or
  is dropped, send `$/cancelRequest`. More than 64 abandoned requests still
  unanswered recycles the session, since async-lsp only frees a request when
  its reply arrives.
- **Trust.** Launch commands, arguments, environment, initialization options
  and server settings come only from built-in defaults, the user's settings or
  the host. A repository's `.fastedit/settings.toml` can't make fastedit run
  anything (Q7).

**Document sync and encoding (§4).**

- **Sync.** Use incremental sync, built from `BufferEvent::Edited`'s `edits`
  against the previously sent text. Fall back to full text when the server only
  supports full sync, when the drained batches hold more than 64 edits, or
  when their new text exceeds half the document.
  - The server's actor drains each document's subscription itself.
  - It keeps the last 32 sent versions' texts, to decode diagnostics and
    replies against the exact text they refer to.
  - Diagnostic events carry session and incarnation ids. Opens set the
    document's active `(session, incarnation)`, guarded by a per-buffer open
    generation so a stale open can't reactivate a closed buffer; closes and
    session ends retire it.
    - The UI applies a `Diagnostics` only if its identity is the active one
      and hasn't been cleared, so stale ones are rejected even before
      anything newer arrives.
    - A `DiagnosticsCleared` is emitted after retirement, so it is never
      checked against the active identity. It empties the set only if the
      set carries that identity, and records the identity as cleared.
  - Unversioned diagnostics are dropped unless a named, verified
    server-specific guarantee fixes the text they were computed on,
    including across a reopen. No server has one yet.
  - Saves are barriers that become `didSave`.
- **Large files.** Never sync a file over 50 MB, or one opened as the
  read-only view for very large files.
- **Encoding offer.** Offer `["utf-8", "utf-16"]` and not utf-32.
  - rust-analyzer and typescript-go pick utf-8.
  - pyright and typescript-language-server ignore the offer and use utf-16.
  - utf-8 maps onto `fe_text::Point`, and utf-16 onto `PointUtf16`, both
    existing.
  - Nothing new is needed in `fe-text` unless utf-32 is ever offered.
- **Emoji tests.** "UTF-16 emoji tests pass" becomes the 13 cases in §4.4,
  checked in all three encodings.

**Hand-off from search (§5).** The fe-nav cards are filed: T105, T106 and T107.
`fe-lsp` plugs a `LspNavigation` provider into T105's `NavigationProvider` at
precedence 100, above search's 0, with `Precision::Exact`. Search answers while
the server isn't `Ready`, and after it reports `Unavailable`. Results replace
each other rather than merge, per T105. An empty answer is final only once
the server reports quiescence (rust-analyzer's `serverStatus`); otherwise
search answers. Three small seam additions
are needed before the LSP provider lands:

- an optional `kind` on `Definition`
- a buffer identity and version on `NavRequest`
- the same on `Location`

**Testing (§6).**

- **Gate.** Most tests run against a fake in-process server. It is built on
  async-lsp's own server side, over an in-memory pipe, with no subprocess, Node
  or network.
- **Slow group.** Real-server tests are `#[ignore]`d, like the soak tests. They
  run through `scripts/lsp-real.sh` and an `lsp.yml` workflow. They pin
  rust-analyzer through the rustup component of the pinned toolchain, and the
  Node servers through a checked-in `tools/lsp/package-lock.json`.
- **TypeScript.** typescript-go 7.0.2 (`tsc --lsp --stdio`) is the default
  server (Q5, changed 2026-10-11). typescript-language-server 6.0.1 with
  `typescript` 6.0.3 is the alternative, behind `lsp.typescript_server`. The
  current `typescript` 7 is the Go port and has no `tsserver.js`.

**Feature scope for M4 (§7).** M4 covers push diagnostics, definition,
references, workspace symbols, hover and completion. Diagnostics go into a
`DiagnosticSet` in `fe-editor`, copied from gpui-kit but **storing anchored
byte ranges**. gpui-kit's version counts LSP columns in chars, which is wrong
for every encoding except utf-32. Code actions, rename, formatting, semantic
tokens, inlay hints, signature help and snippets are out. Pull diagnostics
for open documents came in later (T39, §7).

**Questions for the user (§9):**

1. **Who owns server processes when embedded?** Recommended: fastedit spawns
   them by default, and the host may replace the launcher.
2. **Does fastedit ever install or bundle servers?** Recommended: not in M4.
   Servers come from `PATH`, settings or the host.
3. **Tokio in a core crate?** Recommended: no. Use the async-io and
   async-process arrangement above.
4. **When do real-server tests run in CI?** Recommended: nightly and on demand,
   not on every PR.
5. **Which TypeScript server?** Decided: typescript-go as the default, and
   typescript-language-server as a configurable alternative (changed
   2026-10-11; first recommended the other way round).
6. **When do servers start?** Recommended: lazily, on the first open file of
   their language.
7. **Do servers start in a workspace nobody has trusted?** Recommended: yes in
   M4, with project-layer LSP settings ignored (§3.3), and the host able to
   refuse through its launcher.

## 2. Client crate and runtime

### 2.1 `async-lsp`: confirmed

| | |
|---|---|
| Version read | 0.2.4, released 2026-04-24. Repo `oxalica/async-lsp`, last commit 2026-08-09 |
| Licence | `license = "MIT OR Apache-2.0"` in crate metadata. The tarball ships `LICENSE-MIT` and `LICENSE-APACHE`. (GitHub's licence detector shows only Apache-2.0.) |
| Releases, last 12 months | 0.2.3 (2026-03-04), 0.2.4 (2026-04-24). Earlier: 0.2.2 (2025-03-07), 0.2.1 (2024-11-11), 0.2.0 (2024-01-07) |
| Issues / PRs | 2 open and 16 closed issues; 2 open and 10 closed PRs. PR #27, "switch to gen-lsp-types", was closed unmerged on 2026-04-23: "the most popular choice is still the older lsp-types 0.95.1" |
| MSRV | `rust-version = "1.66"`. Our toolchain is 1.98.1 (`rust-toolchain.toml`). The lock probe built and its tests passed on 1.98.1, inside our edition-2024 workspace |
| `lsp-types` | `lsp-types = "0.95.0"`, which resolves to 0.95.1 |
| Features for a client | `default-features = false, features = ["omni-trait"]`, adding `"tracing"` if wanted. The defaults are `client-monitor`, `omni-trait`, `stdio` and `tracing`. `client-monitor` and `stdio` are server-side: `client_monitor.rs:3` says "Only applies to Language Servers" |
| Added to our lock | 4 crates: `async-lsp` 0.2.4, `lsp-types` 0.95.1, `tower-layer` 0.3.3 and `tower-service` 0.3.3. The same 4 with `tracing` added. Default features also bring `socket2` 0.6.5 and `waitpid-any` 0.3.0. Everything else it uses is already in the lock: futures 0.3.34, serde, serde_json 1.0.151, thiserror 2.0.21, pin-project-lite, url 2.5.8 and others |

**Runtime fit.**

- **No tokio needed.** async-lsp's tokio dependency is optional. Its only tokio
  code is `src/stdio.rs:228-397`, which is server-side `PipeStdin` support
  behind `feature = "tokio"`.
- **No timers.** The concurrency layer uses `futures::stream::Abortable` and
  `AtomicWaker` (`concurrency.rs:18-19`). There is no timeout layer at all
  (`lib.rs:22`: users "implement their own middlewares for like timeout").
- **Any executor works.**
  - `MainLoop::run_buffered(input: impl AsyncRead, output: impl AsyncWrite)`
    (`lib.rs:521`) takes futures-io traits.
  - The loop is `futures::select_biased!` (`lib.rs:548`).
  - The framing is a strict hand-written `Content-Length` codec
    (`lib.rs:418-465`).
- **Probe results.**
  - The lock probe's test ran async-lsp's server and client over a Unix socket
    pair under `smol::block_on`, with no tokio in the tree:
    `client_on_smol_no_tokio ... ok`.
  - The server probe drove all four real servers with
    `async_process::ChildStdout`/`ChildStdin` passed straight to
    `run_buffered`, under `async_io::block_on`, with no glue code.
  - `cargo tree` showed no tokio for `["omni-trait"]`, and none with
    `"tracing"` added.

**How a client is put together.**

- `MainLoop::new_client(|server| router)` (`lib.rs:497`) returns the loop and a
  `ServerSocket` (`lib.rs:721`).
- The router is a `Router` with typed `.request::<R>()`,
  `.notification::<N>()` and `.event::<E>()` handlers.
- `omni-trait` adds typed methods such as `server.initialize(params).await`
  and `server.definition(..).await`.

**Gaps we fill in `fe-lsp`.**

1. **Unknown notifications stop the loop.** `Router::new`'s default
   `unhandled_notification` returns `ControlFlow::Break(Err(Routing(..)))` for
   any method not starting with `$/` (`router.rs:64-72`), and that ends
   `MainLoop`. Any notification we don't register a handler for would stop the
   loop: `telemetry/event`, or rust-analyzer's `experimental/serverStatus` (sent
   once the client opts in). typescript-language-server's custom
   `$/typescriptVersion` starts with `$/`, so it is tolerated. So `fe-lsp` must
   override `unhandled_notification` to log and continue, as the probe did.
2. **No client-side cancellation.**
   - A request's future is a plain oneshot receiver
     (`PeerSocketRequestFuture`, `lib.rs:768`).
   - Dropping it sends nothing. The lock probe confirmed this: the server saw 0
     `$/cancelRequest`.
   - The pending entry stays in `MainLoop.outgoing` until a reply arrives
     (`lib.rs:604`).
   - Request ids are assigned inside the loop, as an `i32` counter from 0
     (`lib.rs:619-621`), and never returned to the caller.

   Two ways to fill this:

   - **(a)** `fe-lsp` issues every request from one actor task per server, so
     the loop's FIFO channel assigns ids in issue order. The actor mirrors the
     counter, and `fe-lsp` sends `$/cancelRequest` with the mirrored id. A
     fake-server test asserts that the mirrored ids equal the ids received, so
     an upstream change shows up in the gate. The version is pinned with `=`.
   - **(b)** Upstream a small change: return the id, or add cancel-on-drop. PR
     #30 shows the maintainer reviews slowly.

   Recommendation: do (a) now and offer (b).
3. **Timeouts.** Race each request against `async_io::Timer`. On expiry, cancel
   it as in (2).

**When to drop async-lsp.** If (a) proves brittle, replace it with a thin
hand-written client. Estimated size:

- framing: ~60–80 lines
- message types: ~100
- a pending map with cancel-on-drop and timeouts: ~120–150
- dispatch of server requests and notifications: ~80–120
- process lifecycle: ~100–150

That is ~450–600 lines plus tests, with `lsp-types` kept for payloads. Nothing
outside `fe-lsp` names an async-lsp type, so the swap stays inside the crate.

### 2.2 Runtime: own thread per server, async-io and async-process, no tokio

**Runtime crates already in `Cargo.lock`**, from `cargo tree --target all -i <crate>`:

| Crate | Version | Comes from | Built on macOS today? |
|---|---|---|---|
| `tokio` | 1.53.1 | `wasm-bindgen-futures` ← `gpui-pre-web` and `wgpu` (feature `rt` only) | No |
| `smol` | 2.0.2 | `gpui-pre-linux` | No |
| `async-process` | 2.5.0 | `smol`, and `zbus` (accesskit and notify on Linux) | No |
| `async-io` | 2.6.0 | `smol`, `async-process`, `zbus`, `oo7` | No |
| `futures` / `futures-executor` | 0.3.34 | `gpui-pre`, and `postage` (in `gpui-pre`) | Yes |
| `async-channel` | 2.5.0 | `gpui-pre`; fe-workbench and fastedit use it directly | Yes |

All the GPUI-side entries come from GPUI's Linux and web backends. On macOS,
`cargo tree -i tokio` (and `smol`, `async-process`, `async-io`) prints nothing.
GPUI's macOS executor is GCD, not an async runtime crate.

**The arrangement.**

- Each running server gets one thread, named e.g. `fe-lsp rust-analyzer`.
  It runs `async_io::block_on(join(main_loop, actor))`.
  - The **main loop** is async-lsp's `MainLoop` over the server's byte streams.
  - The **actor** owns the server's state: documents, pending requests,
    readiness and restart.
- The default launcher spawns with `async_process::Command`. Its
  `ChildStdin`/`ChildStdout` are the futures-io streams `run_buffered` takes.
  Timers are `async_io::Timer`.
- A thread per server isolates a busy or wedged server. There are only a
  handful of servers, so the threads cost little. One shared executor thread
  would save threads but couple the servers.

**What this adds to a macOS build** (lock probe, `cargo tree -p probe -e
normal` minus the workspace's macOS normal graph):

- **4 new lock entries** (above).
- **7 crates compiled on macOS that weren't before:** 6 runtime crates
  (`async-io`, `async-lock`, `async-process`, `async-signal`, `polling` and
  `signal-hook-registry`) plus `serde_repr`. All are MIT/Apache, and all are already in the lock.
- **No tokio, smol or other executor** in the core crate.

**Two embedding facts** to put in the `fe-lsp` crate docs:

- async-io starts its reactor thread lazily, and async-process starts an
  `async-process` thread lazily (`async-process/src/lib.rs:7, 150-152` at
  2.5.0).
- On macOS (anything but Linux and Windows), async-process reaps children
  through a `SIGCHLD` handler (`src/reaper/mod.rs:21-30`;
  `src/reaper/signal.rs:4` uses `async_signal::Signals`). async-signal
  registers it with `signal-hook-registry`. tokio uses the same registry, so a
  tokio host's handler and ours chain rather than clobber each other.

**Alternatives if Q3 goes the other way:**

- tokio's `current_thread` runtime on the same per-server thread, with
  async-lsp's `tokio` feature. That makes tokio a real dependency of a core
  crate. It adds no lock lines (tokio is already there, with only
  pin-project-lite as its dependency), but it adds tokio's process, io and time
  features to every build.
- `std::process` plus reader, writer and stderr threads, as Lapce does. This
  needs no async-io and no signal handler, at the cost of ~3 threads per server
  and our own bridging into futures.

**How results reach the UI thread without GPUI.** `fe-lsp` exposes:

- **Request futures** (`impl Future<Output = Result<T, LspError>> + Send`),
  completed by a `futures::channel::oneshot` from the server's thread. GPUI
  awaits them in `cx.spawn`, as it awaits any future. Blocking callers on a
  background thread (T105's providers, §5) use `futures::executor::block_on`.
  `futures-executor` is already compiled on macOS.
- **An event sink**, `LspManager::new(…, sink: impl FnMut(LspEvent) + Send +
  'static)`, called on server threads, for:
  - published diagnostics
  - server status: starting, ready, restarting, stopped
  - progress
  - `window/showMessage`
  - log lines

  fe-workbench forwards these with `async_channel::Sender::send_blocking` into
  a `cx.spawn` loop. This is exactly how `ProjectPanel` consumes
  `ProjectWatcher` (`crates/fe-workbench/src/project_panel.rs:77-97`).

`fe-lsp` depends on neither GPUI nor `fe-gpui`, so `scripts/check-no-gpui.sh`
keeps passing.

### 2.3 `lsp-types`: 0.95.1

| Version | Released | URI type |
|---|---|---|
| 0.95.1 | 2024-03-18 | `pub use url::Url;` (`lsp-types/src/lib.rs:26`), with `url` 2.5.8 already in our lock |
| 0.96.0 | 2024-05-17 | `pub struct Uri(fluent_uri::Uri<String>)` (`src/uri.rs:7`), on `fluent-uri` 0.1.4 (current fluent-uri is 0.4.1) |
| 0.97.0 | 2024-06-04 | as 0.96. The only other changes are `sync_kind: TextDocumentSyncKind` and `InlineValueRequest::Result` |

- **Licence.** MIT in metadata (`Cargo.toml:30`). The `LICENSE` file is "The
  MIT License (MIT) Copyright (c) 2016 Markus Westerlind".
- **Maintenance.** Dormant. The last commit was 2024-06-04. There are 31 open
  and 59 closed issues, and 16 open and 207 closed PRs.
- **Why 0.95.1.** async-lsp requires `^0.95`, so 0.97 would put two
  `lsp-types` in the lock and break the decided "one version". Lapce, and
  helix's fork, are on 0.95.1 as well. gpui-kit uses 0.97, but we copy from it
  rather than link it.
- **Position encoding is supported** in 0.95.1, not behind `proposed`:
  - `PositionEncodingKind` with `UTF8`, `UTF16` and `UTF32` (`lib.rs:314-331`)
  - `GeneralClientCapabilities.position_encodings` (`lib.rs:1674`)
  - `ServerCapabilities.position_encoding` (`lib.rs:1923`)
- **LSP 3.17 support.** It has the 3.17 pull-diagnostic types:
  `textDocument/diagnostic` (`request.rs:901`), `workspace/diagnostic` (`:915`)
  and `DocumentDiagnosticParams` (`document_diagnostic.rs:80`). It also has
  inlay hints and inline values. M4 needs none beyond the 3.16 core plus
  `positionEncoding`.

### 2.4 Alternatives considered

| Candidate | Version read | Licence (source) | Why not |
|---|---|---|---|
| `tower-lsp` | 0.20.0 (2023-08-11) | MIT OR Apache-2.0 (metadata; `LICENSE-MIT`, `LICENSE-APACHE`) | Server-only: you implement `LanguageServer`, and its `Client` (`src/service/client.rs:44`) is the server's handle to the editor. tokio by default, on `lsp-types` 0.94.1 |
| `tower-lsp-server` | 0.23.0 (0.24.0-rc.1 on 2026-09-11) | MIT OR Apache-2.0 (metadata) | Server-only, same shape (`src/service/client.rs:50`). Uses its own `ls-types` fork rather than `lsp-types` |
| `lsp-server` (rust-analyzer's) | 0.10.0 (2026-07-16) | MIT OR Apache-2.0 (metadata) | Sync, built on crossbeam channels and its own reader, writer and dropper threads (`stdio.rs:16-33`). `Connection` has no constructor over a child's pipes, and its API is the server's (`initialize`, `handle_shutdown`). Usable only as a framing library; request matching would still be ours |
| Lapce's `lapce-proxy` approach | `f66ffaa9` | Apache-2.0 (`LICENSE`; workspace `license = "Apache-2.0"`) | A design, not a crate. `std::process::Command` plus three threads (stdin writer, stdout reader, stderr reader; `lapce-proxy/src/plugin/lsp.rs:210-280`) over crossbeam channels, with hand-formatted `Content-Length` and `jsonrpc-lite` ids. Id matching is `AtomicU64` plus a `Mutex<HashMap>` (`plugin/psp.rs:180-181, 438-439`), and it does send `$/cancelRequest` (`dispatch.rs:389-399`). There is no `positionEncoding` negotiation. Good reference for the `std::process` alternative in §2.2 |
| `helix-lsp` | crates.io 0.0.0 placeholder (2021); upstream `ba40e547` | MPL-2.0 (crates.io metadata; repo licence) | Not published: crates.io has only a placeholder (`//! Placeholder`). Upstream it path-depends on helix-core and helix-loader, hard-depends on tokio (`rt-multi-thread`, `process` …), and would need an MPL exception. Assessed from metadata only |
| Hand-written thin client | n/a | ours | The fallback (§2.1). About 450–600 lines plus tests. It gains cancel-on-drop, timeouts and visible request ids, at the cost of owning framing and dispatch. Not first choice, because async-lsp covers the same ground in 4 lock lines and has been exercised against all four servers |

gpui-kit is not a candidate: it has no LSP client. Its trait shapes
(`crates/base/src/input/editor/lsp/`: `CompletionProvider`, `HoverProvider`,
`DefinitionProvider` and others) all take `&mut Window`/`&mut App` and return
`gpui::Task`, so they are a reference for the `fe-gpui` side only.

### 2.5 Licences outside the allowlist

**None.** The new lock entries are:

- `async-lsp`: MIT OR Apache-2.0
- `lsp-types` 0.95.1: MIT
- `tower-layer` and `tower-service`: MIT

The newly compiled ones are all MIT and/or Apache-2.0: `async-io`,
`async-lock`, `async-process`, `async-signal`, `polling`,
`signal-hook-registry` and `serde_repr`. In the workspace copy,
`cargo deny --offline check licenses` printed `licenses ok`, with async-lsp at
default features plus `tokio` and `forward`, and `lsp-types` 0.95.1.

The language servers themselves are separate programs, not dependencies (§6.2).

## 3. Server lifecycle and process management

### 3.1 The launch seam: `ServerLauncher`

An embedding host may want to sandbox, containerise or proxy servers. So
`fe-lsp` never spawns a process directly. It asks a launcher for a server's byte
streams:

```rust
/// What to start: from settings (§3.3) or supplied by the host.
pub struct ServerSpec {
    pub name: String,                 // "rust-analyzer"
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub languages: Vec<String>,       // fe-syntax language names
    pub initialization_options: Option<serde_json::Value>,
    pub settings: serde_json::Value,  // answers workspace/configuration
    pub readiness: ReadinessMode,     // ServerStatus | Progress; §3.4
    pub ready_ignores_progress: Vec<String>, // token prefixes; §3.4
    pub unversioned_diagnostics: UnversionedDiagnostics, // Drop; §4.1
    pub pull_diagnostics: PullDiagnostics, // Advertised | TsserverRequest; §4.1
}

/// A running server's transport. Client → server bytes go to `input`.
pub struct ServerIo {
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
    pub output: Pin<Box<dyn AsyncRead + Send>>,
    pub stderr: Option<Pin<Box<dyn AsyncRead + Send>>>,
    pub process: Box<dyn ServerProcess>,
}

/// The launcher's handle on what it started, for shutdown and crash detection.
pub trait ServerProcess: Send {
    /// Resolves when the server has gone: exited, or its transport closed.
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>>;
    /// Forcibly stops it (kill the process group, close the proxy …).
    fn kill(&mut self);
}

pub trait ServerLauncher: Send + Sync {
    /// Called on the server's own thread; may block (e.g. start a container).
    /// Should return promptly once `cancel` fires (below).
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo>;
}

/// Fired when the server is stopped, or its manager's kill switch thrown,
/// while it launches. Clone + Send + Sync; once fired, stays fired.
pub struct LaunchCancel { /* … */ }
impl LaunchCancel {
    pub fn new() -> Self;   // fe-lsp makes one per instance; public for tests
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
    /// Resolves once cancelled (at once if already).
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + 'static;
}

/// From `LspManager::kill_switch(&self)`. Clone + Send + Sync, and it
/// outlives the manager.
pub struct KillSwitch { /* … */ }
impl KillSwitch {
    /// Force-stops every server the manager started whose thread hasn't
    /// ended; returns at once. Once thrown, stays thrown.
    pub fn kill(&self);
}

/// The default: `async_process::Command` with piped stdio, cwd = root and
/// `kill_on_drop`. For a process group (so `kill` takes the server's children
/// too), build a `std::process::Command` with `CommandExt::process_group(0)`,
/// convert it (`From<std::process::Command>`, async-process `lib.rs:1151`),
/// and `killpg` through rustix (already in the lock).
pub struct LocalLauncher;
```

The seam is at **byte streams**, not messages. LSP framing is the same
everywhere, so a host that proxies over a socket or a container's stdio only
moves bytes. A message-level transport would duplicate async-lsp's codec.

`LspManager::new(launcher: Arc<dyn ServerLauncher>, …)` takes the launcher, and
fastedit's own binary passes `LocalLauncher`. The host can still decline a
launch: an `Err` marks the server `Stopped` with that error, shown as in §3.5.
Whether the host *must* supply the launcher is Q1.

**Cancelling a launch (T209).** Each server instance has one `LaunchCancel`,
passed to every launch it makes, restarts included. A stop (`LspManager::stop`,
a restart's stop of the old instance, a settings change, the manager's drop)
fires it as it asks the thread to stop, and so does the manager's kill switch.

- **The host contract.** Once `cancel` fires, a launch should return promptly:
  with `ErrorKind::Interrupted`, or with what it has started. A blocking
  launcher waits on `cancel.cancelled()` beside its own work, with
  `async_io::block_on(futures::future::select(..))`. fe-lsp can't preempt a
  launch that ignores `cancel`, so a launcher that doesn't watch it must bound
  its own launch time.
- A launch that returns after its cancel fired is a stop, not a failure: an
  `Ok`'s process is killed before anything is sent to it, an `Err` is dropped,
  and either way the server reports `Stopped(None)`.
- `LocalLauncher` checks `is_cancelled()` after resolving the command and before
  spawning, and returns `Interrupted` without spawning. The login-shell `PATH`
  capture (§3.3) is bounded by its own 2 s and isn't interrupted.

**The kill switch.** `LspManager::kill_switch()` returns a `KillSwitch` shared
with every server thread the manager starts, for a host that can't wait out the
graceful shutdown (§3.4), such as a quit with a deadline. It outlives the
manager, since dropping the manager only asks its servers to shut down.
`KillSwitch::kill` fires the `LaunchCancel` of every instance whose thread
hasn't ended (running, launching, waiting to restart, or still shutting down)
and returns at once. Each thread then kills its process at once, without
`shutdown` or `exit`, launches and restarts nothing more, hands its documents
on as a stop does, and reports `Stopped(None)`. A server the manager starts
after the switch is thrown launches nothing and reports `Stopped(None)`. The
thread keeps its last session's process until the thread ends or launches its
replacement (whose process-group id the old one's may share once reused), so a switch
thrown after the server exited gracefully (a stop, or a recycle) while the
thread still reads its stderr's last words, waits to restart or reports its
end kills what's left of its process group; reading stderr after the
session also stops once the switch is thrown.

**Relationship to `fe-syntax`'s `Spawner`.** These are **two traits**, and the
plan's `fe-lsp` "`Spawner`" gets the new name `ServerLauncher`:

- `fe_syntax::Spawner` (`crates/fe-syntax/src/driver.rs:12-19`) runs a
  `ParseJob` in the background and returns a cancellable `Task`. Its result
  must come back to the driver's thread. `fe-gpui` implements it on GPUI's
  executor (`ParseSpawner`, `crates/fe-gpui/src/view.rs:445-473`). It is about
  *where CPU work runs*.
- `ServerLauncher` is about *where a long-lived external process comes from*.
  It is called on `fe-lsp`'s own server thread, and it has nothing to hand back
  to the UI.

`fe-lsp` doesn't need a job spawner at all, because it owns its threads (§2.2).
Reusing the name would invite one trait to grow both meanings.

### 3.2 Instances

- **One instance per (server name, workspace root).** The workspace root is the
  folder the workbench opened (`Workbench`'s project root, the same root
  fe-nav's `NavRequest.root` carries). Its `workspaceFolders` lists that single
  root.
- **Files outside the root.** These, and documents with no root ("No folder
  open"), get no server in M4. Navigation falls back to search.
- **Multi-root.** When the workbench opens several roots (not in M4), each root
  gets its own instance. The exception is a server that advertises
  `workspace.workspaceFolders.changeNotifications`, which can take
  `workspace/didChangeWorkspaceFolders` instead.
- **Choosing a server for a file.** Use the buffer's `fe-syntax` language name
  (the same `language` that `NavRequest` carries) and pick the first enabled
  server whose `languages` contains it. M4 has one server per language. The
  LSP `languageId` comes from a fixed table: `rust`, `python`, `javascript`,
  `javascriptreact`, `typescript` and `typescriptreact` (fe-syntax's `tsx`).

### 3.3 Configuration

The built-in defaults cover the three M4 servers:

- `rust-analyzer`
- `pyright-langserver --stdio`
- `tsc --lsp --stdio` (typescript-go), the default TypeScript and JavaScript
  server. `lsp.typescript_server = "typescript-language-server"` swaps it for
  `typescript-language-server --stdio`. Only the chosen one runs.

There is no fallback between the two. When `tsc` isn't on the launcher's
`PATH`, the server stops with "typescript-go not found" and navigation stays
search-based, exactly as a missing typescript-language-server does;
typescript-language-server doesn't start in its place. A `tsc` older than
TypeScript 7 is the JavaScript compiler and has no `--lsp`, so `initialize`
fails and §3.5's crash budget takes it to `Stopped`. The fixes are to install
`typescript@7` (or set `lsp.servers.typescript-go.command`), or to set
`lsp.typescript_server = "typescript-language-server"`.

A new `[lsp]` table in `fe-config`'s layered TOML overrides them, through the
same schema, diagnostics and hot reload as `[editor]`. The trust boundary below
limits which layers it may come from:

```toml
[lsp]
enabled = true
max_file_bytes = 52428800          # 50 MB; §4.1

[lsp.servers.rust-analyzer]
command = "rust-analyzer"
languages = ["rust"]
readiness = "server-status"                            # built-in default
ready_ignores_progress = ["rust-analyzer/flycheck/"]   # built-in default
[lsp.servers.rust-analyzer.settings]      # becomes the `rust-analyzer` section
cargo.features = "all"

[lsp.servers.pyright]
command = "pyright-langserver"
args = ["--stdio"]
languages = ["python"]
```

The host can also register `ServerSpec`s through `LspManager`. Host specs win
over settings with the same name.

Setting `command` to a bare name resolves it on the launcher's `PATH`. On
macOS, an app started from Finder gets a minimal `PATH`, so the card should
resolve through the login shell's `PATH` once at startup, as terminals do, or
document absolute paths.

A settings change to a running server's spec restarts that server. A change
under `settings` only sends `workspace/didChangeConfiguration`.

**Trust boundary.** `fe-config` loads the project layer,
`<root>/.fastedit/settings.toml` (`crates/fe-config/src/paths.rs:45-48`),
automatically from whatever repository is opened. A spec is executable
configuration, and so is a server's `settings`: rust-analyzer's settings
include override commands for `cargo check` and build scripts. So:

- **Accepted from:** the built-in defaults, the user layer
  (`user_settings_path()`), and host-registered specs. That covers `command`,
  `args`, `env`, `initialization_options`, `settings`, `languages`,
  `readiness` and `ready_ignores_progress`.
- **Project layer, untrusted.** It may only *narrow*: `lsp.enabled = false`,
  and `lsp.servers.<name>.enabled = false`. Any other `lsp.*` key there is
  ignored, with a `fe-config` diagnostic: "LSP settings in project files apply
  only in trusted folders".
- **Hot reload** applies the same filter, so editing the project file while
  open can't introduce a command either.
- **Workspace trust.** Q7 was revised to (b) (§9, T177): language servers
  start only in folders the user trusts. In a trusted root, the project
  layer's `lsp` keys apply as written (T65): the workbench lays the root's
  project file's `lsp` table over the user file's, over the defaults (each
  an `fe_config::settings::LspLayer`, which keeps its last good table while
  the file has errors), and hands that root's `LspManager` the result. The
  user file's layer is the app's, shared by every window and read by the
  workbench itself, not taken from the filtered settings the window was
  given: those have the project's keys narrowed, so a disabling flag the
  project file has since dropped would stay in them, and a window the host
  settles with a fresh store has no last good table for a broken user file
  (T67). The user layer is the app's, made with the first workbench
  whether or not it has a root or a manager, and it follows the user file
  itself, through a watcher of its own, not the effective settings reloads,
  which skip an edit a project's setting masks (T70). It holds the file's last good
  table by the time a root is first trusted (T69). The watcher is made, held and dropped on the background executor,
  and the main thread never locks the layers a read holds, so untrusting
  and re-rooting don't wait on the file system. It reads the file off the main thread when the
  root becomes trusted, when a manager starts for a trusted root, at each
  settings reload, and when the file changes: it watches the file while the
  root is trusted, as the app's store narrows it and so reloads for no
  change to the keys it drops. Servers start only once the first read is
  applied, and documents opened before then are opened again, under the
  trusted size limit. A read that lands after the root or its trust changed
  is dropped; untrusting goes back to the filtered settings. The app-wide
  `SettingsStore` keeps the filter, since trust is per root, and so do
  workbenches opened later until their own root is trusted.
  `fe_config::settings::trusted_lsp(user, project)` is the same over a fresh
  store's settings.

Starting a configured server on a repository can still run that repository's
code by the server's own design. For example, rust-analyzer runs build scripts
and proc macros, and `cargo check` on flycheck. Whether servers start at all in
an untrusted workspace is Q7. The host can also refuse through its launcher
(§3.1).

Tests (card 3):

- A project layer that sets `command`, `args`, `env` or `settings` for a
  server, both at startup and through a hot reload, never reaches the launcher
  or `workspace/configuration`, and yields the diagnostic.
- A project layer `enabled = false` does stop the server.

### 3.4 Initialize, readiness and shutdown

**Start.**

- **When.** A server starts lazily, when the first document of one of its
  languages is opened in an editor in the workbench (Q6). After that it stays up
  while the workbench is open, even when its last document closes: a restart
  re-indexes, which costs seconds for rust-analyzer.
- **`initialize`.** The call carries:
  - `processId`
  - `rootUri` and `workspaceFolders`
  - `clientInfo { name: "fastedit" }`
  - the spec's `initializationOptions`
  - the capabilities M4 uses (§7)

  The capabilities include:
  - `general.positionEncodings = ["utf-8", "utf-16"]` (§4.2)
  - `window.workDoneProgress = true`
  - `workspace.configuration = true`
  - `workspace.workspaceFolders = true`
  - `experimental.serverStatusNotification = true`, which rust-analyzer reads
    (`crates/rust-analyzer/src/lsp/capabilities.rs:468` at tag `2026-10-05`)
- **After `initialize` answers**, `fe-lsp` records the server capabilities and
  the negotiated encoding (absent means utf-16). It then sends `initialized`,
  and sends `didOpen` for every open document of that server's languages.
- **Startup timeout.** If `initialize` doesn't answer within 30 s, the attempt
  counts as a crash (§3.5).

**Readiness.** The state the fe-nav hand-off reads (§5):

- **The mode is chosen before startup.** It comes from the spec's `readiness`,
  never from what arrives first. There are two modes:
  - **`server-status`** (the built-in rust-analyzer spec). `initialize` sends
    `experimental.serverStatusNotification = true`, and the server is `Ready`
    at its first `experimental/serverStatus` with `quiescent: true`. Progress
    messages never make it ready.
    - If no status at all has arrived 10 s after `initialize` answered (for
      example an older rust-analyzer), the server falls back to the
      `progress` rule below for routing, and its empty answers are never
      final (see **Empty answers**). The fallback is logged.
    - rust-analyzer sends `serverStatus` only on that opt-in. The probe didn't
      opt in, so neither the notification nor what it counts as
      non-quiescent was observed (§10).
  - **`progress`** (every other server). `initialize` has answered, and then
    no work-done progress token has been open for **500 ms**. Tokens matching
    the spec's `ready_ignores_progress` prefixes don't count. The built-in
    rust-analyzer spec lists `rust-analyzer/flycheck/` for its fallback: on a
    large project, the startup `cargo check` can run for minutes and doesn't
    affect navigation.
- **States.** `Starting` lasts from launch until the mode's rule holds, and
  navigation is `NotYet` during it. Then `Ready`, which latches until the
  server restarts: routing doesn't flap back to search on a later
  `cargo check` or re-index.
- **Empty answers.** `Ready` only decides routing. Whether an *empty*
  `definitions`, `references` or `workspace_symbols` answer is final is a
  separate rule:
  - **`server-status` mode, latest status `quiescent: true`** (and no status
    timeout fallback): the empty answer is final, as T105 wants.
  - **Anything else**, including all of `progress` mode and status mode after
    the fallback or while `quiescent: false`: the provider (§5) returns
    `Err(Unavailable("empty answer from a server without a completion
    signal"))`, and search answers.

  A non-empty answer is always final. Readiness heuristics can be wrong
  through a delayed first progress, a gap between phases of any length, or
  indexing that reports no progress at all. When they are, a request reaches
  the server early, and an empty reply gives way to search. An LSP-sourced
  "no definition" therefore only ever comes from a server that has said it is
  done. For pyright and typescript-language-server, which have no such signal
  (§3.4 probe), an empty LSP answer always shows search's answer instead,
  labelled "by search".
- **No upper bound.** On a huge workspace, search keeps answering until the
  server is ready.

**Fake-server tests (card 4):**

- the first progress begins 700 ms, and separately 15 s, after `initialize`
- gaps of 900 ms and of 15 s between progress phases
- indexing with no progress at all
- in each case, an empty answer during the "indexing" falls through to search
  (the fake returns empty until its scripted indexing ends), and a non-empty
  answer is returned
- `progress` mode after 60 s of quiet: an empty answer still falls through
- a `serverStatus` that arrives after the progress rule would have latched
  (status mode must not latch early)
- `quiescent: false` after `quiescent: true` (a re-index): empty falls
  through again
- no status in status mode: the fallback after 10 s, on an injected clock,
  and its empty answers are never final
- the first navigation request after `Ready` returns the fake's exact
  non-empty answer, not search's

Why not plain "after `initialize`": every server answered `initialize` in
4–274 ms, long before it could resolve anything. rust-analyzer's own startup
in the probe (seconds after `initialize` was sent, on a one-crate project):

- `initialize` reply: 0.037
- Fetching: 0.065 → 0.388
- Building CrateGraph and Roots Scanned: ending at 1.010
- a second CrateGraph and Roots Scanned, ending at 1.251
- Loading proc-macros: ending at 1.039, then again at 1.674
- cache priming ("Indexing"): 1.684 → 2.737
- flycheck "cargo check": 2.954 → 3.149

In the other two runs the last `end` came at 2.348 and 3.288 s. Progress tokens
are reused (`rustAnalyzer/Fetching` appears several times), so begins and ends
are matched per occurrence.

**Gaps with no progress open**, between the first `begin` and the last `end`:

| Run | Mid-startup gaps | Gap just before flycheck |
|---|---|---|
| utf-8 | at most 50 ms | 217 ms |
| utf-32 | at most 92 ms | 224 ms |
| none | at most 107 ms | 198 ms |

So a 300 ms settle would have had only ~76 ms of margin. 500 ms has ~275 ms,
and it delays `Ready` by at most 0.5 s. Ignoring flycheck makes the
pre-flycheck gap the end of startup: on this project, `Ready` came about
3.2 s after `initialize`.

The other servers:

- typescript-language-server sent one progress, "Initializing JS/TS language
  features", from 0.19 to 0.36 s.
- pyright and typescript-go sent no progress, so they are `Ready` 500 ms after
  `initialize`. pyright's `initialize` reply came at 0.274 s.

Why not "after the first progress `end`": rust-analyzer's first `end`
(Fetching, 0.388 s) comes before its crate graph exists.

**Shutdown.**

- **When.** On workbench close, on app quit, and before restarting a server
  whose spec changed.
- **Sequence.**
  1. Send the `shutdown` request, with a 2 s timeout.
  2. Send the `exit` notification.
  3. Wait up to 1 s for `ServerProcess::exited`.
  4. Then `kill`.
- **Before `initialize` answers**, the protocol allows nothing to be sent,
  `shutdown` included, so a stop kills the server at once rather than waiting
  for the answer (up to the 30 s startup timeout). Since T209.
- **The kill switch** (§3.1) cuts any of these waits short with `kill`.
- **Measured** in the probe, as the `shutdown` reply and then the exit after
  `exit`:

  | Server | `shutdown` reply | Exit after `exit` |
  |---|---|---|
  | rust-analyzer | 1–2 ms | 31–43 ms |
  | pyright | 1 ms | 8–9 ms |
  | typescript-language-server | 0–1 ms | 3–18 ms |
  | typescript-go | 15–16 ms (once 489 ms) | 3–8 ms |

- **Exit codes.** typescript-go exits with code 1 even after a clean shutdown:
  its `handleExit` returns `io.EOF` (`internal/lsp/server.go:1311-1313` at
  `typescript/v7.0.2`). So after `exit`, a non-zero code and async-lsp's
  `Err(Eof)` from the main loop both count as a normal stop, not a crash.

### 3.5 Crashes, cancellation, timeouts and what the user sees

**Crash** means one of these:

- the process exits, or the transport reaches EOF, while not shutting down
- `initialize` fails or times out
- the main loop errors

On a crash:

1. Every pending request fails with `LspError::ServerDown`. fe-nav's provider
   maps that to `Unavailable`, so search answers.
2. That server's diagnostics are cleared: they belong to a dead session.
3. The crash's timestamp joins the server's crash list. Timestamps older
   than **3 minutes** expire from the list. *n* is the number left: a
   **rolling-window budget**. There is no separate uptime reset.
4. If *n* ≤ 5, the state becomes `Restarting(n)`, and the server restarts after
   a backoff of **1, 2, 4, 8 or 16 s** for *n* = 1 to 5. The backoff follows
   *n*, so it falls back to 1 s as old crashes expire.
5. If *n* = 6, meaning 6 crashes within 3 minutes, it gives up. The state
   becomes `Stopped(error)` until `lsp.restart` (which clears the list) or a
   spec change.

Examples:

- Rapid failures: crashes at 0, 1, 3, 7, 15 and 31 s give restarts after 1, 2,
  4, 8 and 16 s, then `Stopped` at the 6th crash.
- Crashes every 2 minutes: the window holds at most 2, so restarts come after
  1 or 2 s, and it never gives up.

Both cases, and crashes spread just outside the window, are fake-server tests
on an injected clock (card 4).

A restarted server is a new session:

- `initialize` runs again.
- Every open document is re-sent with `didOpen` and its current text.
- LSP document versions restart at 0 for that session (§4.1).
- Readiness starts again at `Starting`.

**Cancellation.**

- Every request returns a future to its caller. Dropping that future, or
  reaching its timeout, sends `$/cancelRequest` with the request's id (§2.1,
  gap 2). The caller is answered at once with `Cancelled` or `Timeout`.
- **Orphaned requests stay bounded.** async-lsp 0.2.4 frees a request's entry
  in its private `MainLoop.outgoing` map only when a reply arrives
  (`lib.rs:604`). Sending `$/cancelRequest` doesn't free it. So the actor
  doesn't drop async-lsp's request future when the caller gives up: it moves
  it to an **orphan set**, a `FuturesUnordered`, which mirrors the entries
  async-lsp still holds.
  - When the late reply (or the server's `RequestCancelled` error) arrives,
    the orphan resolves, its result is discarded, and async-lsp's entry is
    gone too.
  - A compliant server answers every cancelled request, so the set drains.
  - If the set exceeds **64 orphans**, the server has stopped answering. The
    actor recycles the session: shutdown with a 2 s timeout, then kill and
    restart. That counts as a crash for the budget above. The restart drops
    the whole `MainLoop`, and its map with it.

  Fake-server tests (card 4):
  - A server that answers cancels returns the orphan count to 0.
  - A server that never answers recycles the session at the 65th abandoned
    request, and memory stays bounded across repeated rounds.
  - A late reply after a timeout reaches no caller.
- Blocking callers pass T105's `&AtomicBool`. While it waits, the provider
  polls the flag every 10 ms and cancels when it's set.
- A reply of `RequestCancelled` (-32800), `ContentModified` (-32801) or
  `ServerCancelled` (-32802) becomes `LspError::Cancelled` or `Retry`. It is
  never shown to the user. For navigation, `Retry` falls back to search.

**Timeouts.** Per method, settable in `[lsp]`:

| Method | Timeout | Notes |
|---|---|---|
| `definition` | 5 s | on expiry, `Unavailable`: search answers |
| `workspace/symbol` | 5 s | |
| `references` | 30 s | streams nothing until the server replies; T112's panel shows "Finding references…" |
| `hover` | 3 s | silently nothing |
| `completion` | 3 s | menu closes |
| `completionItem/resolve` | 2 s | |
| `shutdown` | 2 s | |
| `initialize` | 30 s | |

A timeout also sends `$/cancelRequest`.

**What the user sees.** `LspEvent::Status` drives a status-bar item, per server
and root, in the UI card:

- "rust-analyzer: starting…"
- "indexing" while progress is open, with the progress title
- nothing once ready
- "restarting (2/5)", where 2 is *n* within the window
- "stopped: <error>", with an `lsp.restart` command

The last 64 KiB of each server's stderr and `window/logMessage` go into a
ring buffer, shown by `lsp.show_log`. `window/showMessage` becomes a toast.
While a server is down, navigation keeps working by search, labelled "by
search" as T107 and T112 already do.

### 3.6 Server-to-client requests

| Request | Answer | Seen in the probe |
|---|---|---|
| `workspace/configuration` | For each item, the value at `section` within the spec's `settings`, or `null` | pyright: sections `python` and `pyright`. typescript-language-server: `formattingOptions`. typescript-go: `js/ts`, `typescript`, `javascript`, `editor`. rust-analyzer sent none in this run |
| `window/workDoneProgress/create` | `null` (accept). Track the token | rust-analyzer 14×, typescript-language-server 1× |
| `client/registerCapability` / `unregisterCapability` | Per registration, see below | typescript-go: `workspace/didChangeConfiguration` (sent even though we don't advertise dynamic registration for it) |
| `workspace/workspaceFolders` | The single root | |
| `workspace/applyEdit` | M4: `{ applied: false, failureReason: "not supported" }`, since M4 sends nothing that triggers it (§7). Later: apply as **`Origin::Lsp`** (`crates/fe-buffer/src/buffer.rs:21-27`), through `Buffer::edit_from` on the snapshot of the version the edit names, so a conflict is refused rather than misapplied. Unopened files are edited on disk or opened first; that choice is for the rename/code-action card | |
| `window/showMessageRequest` | `null` (no action chosen). Show the message | |
| `window/showDocument` | `{ success: false }` in M4 | |
| `workspace/diagnostic/refresh` | `null`, then every open document of the session is pulled again (§7, pull diagnostics) | rust-analyzer: once, after flycheck |
| `semanticTokens/refresh`, `inlayHint/refresh`, `codeLens/refresh` | `null` | |
| anything else | `MethodNotFound` (async-lsp's default for requests, `router.rs:56-62`) | |

**Dynamic registration.** A successful reply means the registration is
installed. So:

- **Initialize:** every `dynamicRegistration` in our client capabilities is
  explicitly `false`.
- **`didChangeConfiguration` is unconditional.** When a server's `settings`
  change (§3.3), we always send `workspace/didChangeConfiguration`, whatever
  is registered. The protocol defines it as a plain client notification, and
  pull-model servers answer it by re-requesting `workspace/configuration`.
- **`client/registerCapability`:** accept only methods whose effect we
  implement. In M4 that is exactly `workspace/didChangeConfiguration`, whose
  effect is already guaranteed by the unconditional policy. Accepted
  registrations are recorded in a set keyed by registration id, so the
  bookkeeping stays honest. Delivery never depends on the set:
  - registering ids A and B, then unregistering A, keeps notifications
    flowing
  - so does unregistering B as well, since the policy is unconditional
- **Anything else** (`workspace/didChangeWatchedFiles`,
  `textDocument/didSave`, `textDocument/*` features …): the whole request is
  answered with a `RequestFailed` (-32803) error naming the unsupported
  methods, and nothing is recorded. The protocol makes registration
  all-or-nothing per request, so a request mixing a supported and an
  unsupported method is refused too.
- **`client/unregisterCapability`:** removes the ids it names from the set.
  An unknown id is a success and a no-op.

Fake-server tests (card 4):

- Two `didChangeConfiguration` registrations (A and B) are accepted. Then:
  - a settings change sends the notification
  - after unregistering A, a settings change still sends it
  - after unregistering B too, it is still sent (unconditional)
  - unregistering an unknown id succeeds and changes nothing
- With no registration at all, a settings change sends it.
- A `didChangeWatchedFiles` registration, alone or mixed with a supported one,
  gets the error and records nothing.

**Notifications from servers.**

- **Handled:** `textDocument/publishDiagnostics`, `$/progress`,
  `window/logMessage`, `window/showMessage`, and rust-analyzer's
  `experimental/serverStatus`.
- **Logged and ignored:** everything else, including `$/typescriptVersion` and
  `telemetry/event`.

Completion accepted by the user, including its `additionalTextEdits`, is one
`Origin::User` transaction: the user chose it, and it should undo as one step
with their typing. `Origin::Lsp` is for edits the server initiates. `Format` is
for formatting, later.

## 4. Document sync and position encoding

### 4.1 Incremental sync from `BufferEvent`

**What the buffer gives us.** `BufferEvent::Edited` carries:

- `from`/`to` versions
- `edits: Arc<[Edit]>`, sorted and non-overlapping, each with `old` in
  pre-batch byte offsets and `new` in post-batch offsets
- the post-batch `text`

(`crates/fe-buffer/src/buffer.rs:46-61`, `crates/fe-buffer/src/edit.rs:5-12`.)
A `Subscription` queues events until `drain()`.

**The conversion.** Each open document's state in `fe-lsp` keeps `sent: Text`,
the text the server has, as a cheap rope clone. For each batch:

1. Walk `edits` **in reverse order**.
2. For each edit, emit one `TextDocumentContentChangeEvent`:
   - `range`: `old.start..old.end` converted to positions **in `sent`**, in the
     negotiated encoding
   - `text`: `event.text.slice_to_string(new)`
3. Set `sent = event.text`.

Changes in one `didChange` apply in order. Walking in reverse means no earlier
change moves a later one's range, so every range can be computed against one
pre-batch text without shifting offsets. Undo and redo batches
(`EditKind::Undo`/`Redo`) are ordinary batches here.

**Who drains.** Opening a document hands `fe-lsp` four things:

- its `BufferId`
- the current `Text` and `fe_buffer::Version`
- its own `Subscription` (`Buffer::subscribe`, `buffer.rs:315`)
- the path

`Subscription` is an `Arc<Mutex<Vec<BufferEvent>>>`, so it is `Send`. The
server's **actor drains it**, on the server thread. The actor can bring a
document up to date itself whenever it needs to, which request admission below
relies on. Draining happens:

- **on a nudge.** The UI calls `LspManager::changed(buffer_id)` once per frame
  for any buffer that emitted an event: edits *and* saves. fe-workbench's save
  path nudges right after `Buffer::did_save` (`buffer.rs:244-249`).
- **before admitting a request** on that document.
- **on a 250 ms backstop tick** over all open documents. It is one lock and a
  length check each. So even a missed nudge can't strand an event: a save after
  edits were already flushed still reaches the server within 250 ms.

**Identity: sessions, incarnations and versions.**

- A **session** is one run of a server process. Its id is a `u64` from the
  `LspManager`, never reused.
- An **incarnation** is one `didOpen` … `didClose` of a document within a
  session. Its id is also a `u64` from the `LspManager`, never reused.
- The LSP `version` is an `i32` per URI per session. It is **monotonic across
  reopens**: a reopen's `didOpen` uses the last version sent for that URI
  plus 1, not 0. So a delayed publication for an earlier incarnation can't
  collide with a version of the new one. A new session starts at 0. The old
  session's transport is closed by then, so none of its messages can arrive.
- Each incarnation keeps a **bounded ring of the last 32 versions sent**. Each
  entry holds:
  - the LSP version
  - the `fe_buffer::Version`
  - the `Text` as sent (a rope clone that shares structure)
  - that batch's `edits`

  `didClose` and the session's end drop the ring.

**Diagnostics** (`textDocument/publishDiagnostics`), for the current
incarnation of the URI:

1. **Versioned, *v* in the ring.** Decode the positions against the ring's
   `Text` for *v*, in the session's encoding. That gives byte ranges in
   buffer version *B(v)*.
2. **Versioned, *v* older than the newest version already published for that
   incarnation** (out of order). Drop it.
3. **Versioned, *v* not in the ring.** Either it is older than the ring, or it
   belongs to an earlier incarnation (lower than the incarnation's first
   version), or it is newer than anything sent (impossible; logged). Drop it.
   The server republishes after the next change.
4. **Unversioned** (LSP 3.17 makes `version` optional, and the notification
   carries no incarnation id either). **Dropped**, whatever the incarnation
   has sent so far.
   - Even "before the first `didChange`" doesn't fix the text. After a
     close and reopen, a publication the server computed on the earlier
     incarnation's text can arrive after the new `didOpen` and before its
     first `didChange`. Nothing in it says which incarnation it belongs to,
     and the order of the two directions doesn't either.
   - The only exception is a **named, verified server-specific guarantee**
     that fixes which text an unversioned publication was computed on,
     including one computed before a close and delivered after the reopen.
     It is recorded in the server's spec as
     `unversioned_diagnostics: UnversionedDiagnostics`, which is `Drop` by
     default; any other value names the guarantee and cites where it was
     verified (the server's source and a card 7 or card 10 test). "The
     server publishes in order" or "it publishes soon after an edit" is not
     such a guarantee. No server has one today: whether any of the three
     omits the version is unchecked (§10).

   The diagnostics already shown stay, anchored and moving with the edits,
   until a versioned publication replaces them. A server that never versions
   its publications therefore shows no diagnostics, a visible but safe
   failure, and card 7 then decides on a server-specific remedy.

   **typescript-language-server's remedy: pull through tsserver** (T73,
   2026-10-12; evidence in `docs/decisions/rq81bj4nvm-tsserver-diagnostics.md`).
   It never versions its pushes, and no guarantee can be verified for them:
   its `FileDiagnostics` keeps one list per tsserver event kind (syntax,
   semantic, suggestion), replaces a kind as its event comes, never clears
   them on a change, and publishes all three merged, debounced 50 ms, so one
   publication can join kinds computed on different texts. Its pushes stay
   dropped. Its spec has `pull_diagnostics: PullDiagnostics::TsserverRequest`
   instead (the default, `Advertised`, is §7's `textDocument/diagnostic`):
   - After the same triggers as §7's pulls, fe-lsp sends three
     `workspace/executeCommand` requests back to back, nothing between them:
     `{ command: "typescript.tsserverRequest", arguments: [<cmd>, { file: <uri> }] }`
     for `syntacticDiagnosticsSync`, `semanticDiagnosticsSync` and
     `suggestionDiagnosticsSync`. The server registers the command
     (`executeCommandProvider`); a session whose server doesn't is never
     pulled.
   - The guarantee is tsserver's: `change`, `open`, `close` and `updateOpen`
     are **fence** commands (`SingleTsServer.fenceCommands`), and no request
     queued after one runs before it. typescript-language-server queues a
     `change` as it reads each `didChange`, and `typescript.tsserverRequest`
     queues its command as it reads the request, so all three are computed
     on the text of the version sent before them.
   - Each reply is tsserver's response object, its `body` the diagnostic
     array. `start` and `end` are 1-based `{ line, offset }`, `offset` in
     UTF-16 code units whatever encoding the session negotiated; they're
     decoded in the ring's text for that version as utf-16. `category`
     `error`, `warning`, `suggestion` and `message` become Error, Warning,
     Hint and Information; `reportsUnnecessary` and `reportsDeprecated`
     become the tags; `code` is kept and `source` is `typescript`. A
     malformed entry is skipped and logged.
   - The three together are one pulled list, merged with pushes as §7 says
     (the pushes being dropped). If any fails, is cancelled or times out,
     the pull is dropped, and the others are cancelled.
5. **No current incarnation** (never opened, already closed, or a restart in
   progress). Ignore it. Workspace-wide diagnostics are out of M4 (§7).

**Delivery to the UI.** Two events:

- `LspEvent::Diagnostics { buffer: BufferId, session, incarnation, version:
  fe_buffer::Version, server, items }` carries **byte ranges in that buffer
  version**, never bare positions.
- `LspEvent::DiagnosticsCleared { buffer, session, incarnation, server }` is
  emitted at `didClose` and when a session ends.

**The active identity.** Comparing an event only with identities the UI has
already accepted isn't enough: after a restart or a reopen, an old event still
queued in the channel arrives before any event of the new identity, and
nothing newer has been accepted to compare it with. So the UI checks each
`Diagnostics` against the **current active `(session, incarnation)`** for
its (buffer, server), which lifecycle changes set, not diagnostic delivery.
(`DiagnosticsCleared` is handled separately, below.)

`LspManager` keeps that table behind a mutex shared with the server actors,
and the UI reads it with `LspManager::active_identity(buffer, server) ->
Option<(SessionId, IncarnationId)>`. Under the same lock it keeps, per
buffer, an **open generation**: a `u64` that `LspManager::open(buffer)` and
`LspManager::close(buffer)` each bump on the calling (UI) thread, and that
an open buffer carries. Every open the actor performs, the first one and
the re-opens after a restart, carries the generation it was issued under.
Only lifecycle changes write the table:

- **Open.** Under the lock, the actor first checks that the buffer is still
  open **and** that its generation is still the one the open carries. If
  so, it inserts the new `(session, incarnation)`, and only then sends that
  incarnation's `didOpen`. If not, a `close` (or a close and reopen) got
  there first: the open is stale, so it inserts nothing, sends no
  `didOpen`, and forgets it. So no diagnostic of an identity can be decoded
  before the identity is active, and an open still pending when the buffer
  closes can't undo the close's retirement. Ids never being reused doesn't
  cover that case; the generation check does.
- **Close retires the identity.** `LspManager::close(buffer)` bumps the
  generation and removes the buffer's entries in one critical section on
  the calling (UI) thread, before the actor even sends `didClose`.
- **An actor's own close retires only its own identity.** When the actor
  closes a document itself (a file that outgrew the size limit), it doesn't
  bump the generation: only the manager's `open` and `close` do. Under the
  lock, it removes the (buffer, server) entry only if the buffer's
  generation is still the one that incarnation was opened under **and** the
  entry is still that incarnation's `(session, incarnation)`. Otherwise the
  UI has closed, or closed and reopened, the buffer since the actor drained
  the oversized edit. That close already retired the identity, so the stale
  close changes no shared state and can't invalidate the newer open, active
  or still pending. Either way it finishes its own incarnation, and the
  document isn't re-opened, on a restart either, until it is back under the
  limit.
- **Session end retires every identity of that session.** On a crash, a
  shutdown, a give-up or a recycle, the actor removes the entries whose
  session is its own, and no others, as soon as it sees the transport close
  or decides to stop, before it emits anything else.

Ids are never reused, so a retired identity never becomes active again.

The UI applies the two events like this:

- A **`Diagnostics`** is applied only if its `(session, incarnation)` **equals
  the active identity** at delivery, and isn't one the UI has seen cleared.
  Anything else is rejected: an old session's or incarnation's event, one
  queued before a close, and one arriving after its `DiagnosticsCleared`.
  On applying, it rebases each range with
  `BufferSnapshot::map_offset(offset, bias, version)` (`snapshot.rs:166`),
  then stores them as anchors in the `DiagnosticSet` (§7), recording the
  identity they came from. A version below the buffer's edit-log floor is
  dropped.
- A **`DiagnosticsCleared`** is emitted after its identity was retired, so it
  is never active. It empties that server's set for the buffer if the set
  came from the same identity. The UI also records, per (buffer, server), the
  last identity it has seen cleared, and rejects any later `Diagnostics` for
  it. The table already rules that event out; this check doesn't depend on
  the order in which the actor updates the table and emits events.

So an event queued in the channel while the buffer was closed and reopened, or
while the server restarted, can't paint stale diagnostics, whether or not
anything newer has been delivered yet. Edits made between publication and
delivery are rebased.

Fake-server tests (card 2/7):

- a publication for a version two edits old lands on the right text
- an edit between publication and delivery
- out-of-order versioned publications
- an unversioned publication from a server whose spec is `Drop` is dropped,
  before and after the first edit
- a delayed unversioned publication across reopen: the fake server computes
  it on the old incarnation's text and sends it after the new `didOpen` and
  before any `didChange`: dropped
- close/reopen with a delayed publication carrying the old incarnation's
  version number: dropped, and versions don't collide
- a restart with an old session's event still queued for the UI: rejected
- an old queued event before any new diagnostic: a `Diagnostics` for the old
  identity is still queued when the buffer is closed and reopened (and, in a
  second case, when the session restarts), and the UI drains it before any
  event of the new identity has arrived: rejected
- a `Diagnostics` delivered after `DiagnosticsCleared` for the same identity:
  rejected, and the set stays empty
- a stale activation: the actor's open (and, in a second case, a restart's
  re-open) is paused before activation, the buffer is closed, and the open
  resumes: no identity is inserted, no `didOpen` is sent, and a diagnostic
  the fake server publishes for that URI is rejected
- a stale activation across close and reopen: as above, but the buffer is
  reopened before the paused open resumes. The stale open activates
  nothing; only the new open's identity becomes active, and only its
  diagnostics are applied
- a stale size-limit close: the actor drains an edit that takes the
  document over the limit and is paused before retiring it; the buffer is
  closed and reopened, and the retirement resumes. The new open's
  generation and entry are untouched, it still activates (or stays active),
  and its diagnostics are applied

**Ordering and batching.**

- Each server has one outgoing queue, served by its actor, and it stays in
  order.
- A drain processes the document's events **in order**. Consecutive `Edited`
  batches coalesce into one `didChange`, with their changes concatenated in
  batch order.
- **A `Saved` is a barrier.** At `Saved { version }`, the actor does three
  things in order:
  1. Flushes the `didChange` for the batches before it, which brings the
     server up to that version.
  2. Sends `didSave`. With `save.includeText`, the text is the ring's text for
     that version, which is exactly what was saved.
  3. Starts a new `didChange` for the batches after it.

  A drain of `Edited(V1), Saved(V1), Edited(V2), Saved(V2), Edited(V3)`
  sends: change to V1, save V1, change to V2, save V2, change to V3.
- There is no timer debounce. Frame-rate nudges already coalesce typing.

**Request admission.** A request on a document names the buffer version
*V* it was computed on (§5). The actor drains first, so the sent version *S*
is at least *V*: events up to *V* were emitted before *V* existed.

- If **S = V**, it sends the request, and captures the ring entry for *V* (and
  the current entries of other open documents) to decode the reply.
- If **S > V** (the buffer moved on while the request was queued), it doesn't
  rebase the request. It answers `Unavailable("buffer changed")`, and for
  navigation, search then answers on `request.text`, which is *V*.

**Saving.** `BufferEvent::Saved { version }` (`buffer.rs:58`) becomes
`textDocument/didSave` through the barrier above. It includes the text when
the server's `save.includeText` asks for it. `synchronization.didSave = true`
is advertised; `willSave` and `willSaveWaitUntil` are not. rust-analyzer's
default `checkOnSave` runs `cargo check` on `didSave`, which is how compiler
diagnostics refresh. This is separate from watching files changed outside
fastedit (§7).

Tests:

- **Fake server:**
  - edits on both sides of a save, and two saves in one drain, give exactly
    the change/save sequence above
  - `includeText` carries the saved version's text, never a later one
  - flush an edit, then save, then do nothing else: `didSave` arrives, from
    the nudge, and also with the nudge suppressed, from the 250 ms backstop
    on an injected clock
- **Real server (card 10):** rust-analyzer's diagnostics change after saving a
  file with a type error introduced and then fixed.

**Full-text fallback.** Send one change with no range, holding the whole text,
when:

- the server's `textDocumentSync.change` is `Full`. (All four servers advertise
  `Incremental`: rust-analyzer `capabilities.rs:38`, pyright
  `languageServerBase.ts:651`, typescript-language-server `lsp-server.ts:260`,
  typescript-go `server.go:1075`.)
- the drained batches hold **more than 64 edits**, e.g. a multi-cursor
  replace-all
- the batches' new text exceeds half the document

**Large files.**

- A document over `lsp.max_file_bytes` (default 50 MB, the plan's
  viewport-only highlighting threshold) is never `didOpen`ed.
- A file opened in the read-only view for very large files (512 MiB and up,
  `pread`-backed, `docs/decisions/mmap-read-only.md`) has no rope, so it is
  never synced either.
- For such files, the LSP provider answers `Unavailable` ("too large for the
  language server"), so search answers. Diagnostics, hover and completion are
  off.
- A synced file that grows past the limit through edits is `didClose`d.

**Line endings.**

- `fe-text` stores `\n` only. The file's `LineEnding` is recorded for saving,
  and lone `\r` and `\r\n` in inserted text are normalised
  (`crates/fe-text/src/lib.rs:7-9`, `crates/fe-buffer/src/buffer.rs:683-689`).
- LSP counts `\n`, `\r\n` and `\r` each as one line break, and columns never
  include the break. So positions in our LF text equal positions in the
  on-disk CRLF file.
- `didOpen` sends the LF text, and that is what the server holds for open
  documents.
- For an unopened CRLF file, a column at the `\r` equals the row's length,
  because LSP columns exclude the break. That is exactly the row end in our LF
  text. A server that wrongly counted `\r` as content would send one column
  more, and that clamps to the row end, like any position past the end of a
  line (§4.4, case 11).

### 4.2 `positionEncoding`: offer utf-8, then utf-16

**What each server picks** (server probe, versions in §6.2):

| Offer | rust-analyzer `2026-10-05` | pyright 1.1.414 | typescript-language-server 6.0.1 | typescript-go 7.0.2 |
|---|---|---|---|---|
| `["utf-8","utf-16"]` | `utf-8` | field absent (utf-16) | field absent (utf-16) | `utf-8` |
| `["utf-32","utf-8","utf-16"]` | `utf-32` | absent (utf-16) | absent (utf-16) | `utf-8` |
| omitted | `utf-16` (always sends the field) | absent | absent | `utf-16` |

**Where each server decides:**

- **rust-analyzer.** `negotiated_encoding()` takes the first utf-8 or utf-32 in
  the client's list. Its comment says "NB: intentionally prefer just about
  anything else to utf-16", and the fallback is utf-16 (`capabilities.rs:292-308`).
  It always emits the field (`:28-32`).
- **pyright.** `initialize` builds its result at
  `languageServerBase.ts:649-691` with no `positionEncoding` (and no
  `serverInfo`). It is utf-16 only.
- **typescript-language-server.** Its result (`src/lsp-server.ts:258-373`)
  has no `positionEncoding`. It is utf-16 only.
- **typescript-go.** It picks utf-8 if utf-8 appears anywhere in the list and
  ignores order. Otherwise it picks utf-16, never utf-32
  (`internal/lsp/server.go:1052-1055`).

**The servers really use the negotiated encoding.** The probe asked for
definitions of a variable declared after 😀, after the ZWJ family `👨‍👩‍👧`, and
after `é` written as e + U+0301. In each case it compared the returned column
with the expected column in each encoding.

- rust-analyzer's Rust lines:

  | Line | Expected utf-8 | Expected utf-16 | Expected utf-32 |
  |---|---|---|---|
  | 😀 | 25 | 23 | 22 |
  | ZWJ | 39 | 29 | 26 |
  | é | 24 | 23 | 23 |

  rust-analyzer returned the column of whichever encoding was negotiated,
  every time.
- pyright returned 11, 17 and 11, its utf-16 columns, under every offer.
  typescript-language-server returned 23, 29 and 23 under every offer.
  typescript-go returned 25, 39 and 24 under utf-8.
- **Requests sent in the wrong encoding missed.** They returned null or a
  different symbol, on every ZWJ line, where the encodings differ by 10
  columns. Where a wrong column was past the end of the line, the server
  clamped it onto the identifier there. So a test must use positions that
  don't sit at the end of a line, or it can pass by accident.

**Recommendation: offer `["utf-8", "utf-16"]`.**

- **utf-8 first.** It is a byte column, the cheapest conversion for our
  byte-offset buffer.
- **utf-16 always works.** It is the protocol's mandatory fallback, used
  whenever the reply omits the field.
- **Not utf-32.** rust-analyzer would take it over utf-8 if it came first, and
  no server needs it. Offering it means a third conversion path to write and
  test, for nothing.

`fe-lsp` stores the negotiated encoding per session, and every conversion goes
through it.

### 4.3 Mapping onto `fe-text`

| Encoding | Offset → position | Position → offset | Status |
|---|---|---|---|
| utf-8 | `Text::offset_to_point` → `Point { row, column: bytes }` (`lib.rs:201-206`) | `Text::point_to_offset`: clamps the column to the row and clips onto a char boundary, towards the start (`lib.rs:208-216`) | **exists**. A UTF-8 `Point` is our `Point`, so no `PointUtf8` is needed |
| utf-16 | `Text::offset_to_point_utf16` → `PointUtf16` (`lib.rs:230-236`) | `Text::point_utf16_to_offset`: clamps the column to the row and moves out of surrogate pairs (`lib.rs:238-250`) | **exists**, tested in `crates/fe-text/src/tests/mod.rs:31-43` and by the property test in `crates/fe-text/src/tests/large.rs:99-125` (texts generated at `:83-97`) |
| utf-32 | would need `offset_to_point_utf32` from `rope.byte_to_char_idx` | would need `point_utf32_to_offset` | **missing**. ropey's `metric_chars` is already enabled (`Cargo.toml:105`), so it is ~20 lines, but only if utf-32 is ever offered |

The `PositionEncoding` enum, and the
`to_lsp(text, offset, enc) -> lsp_types::Position` and
`from_lsp(text, pos, enc) -> usize` functions, live in `fe-lsp`. `fe-text`
stays LSP-agnostic.

**Cost on long lines.** None of the conversions scans the row:

- **utf-8.** One `byte_to_line_idx` plus one `line_to_byte_idx`, then a
  subtraction.
- **utf-16.** Two `byte_to_utf16_idx` calls on top of that, answered from the
  rope's `metric_utf16` counts (enabled at `Cargo.toml:105`).

Each is O(log n) in the rope plus a scan within one chunk. So a position on
column 5,000,000 of a one-line 10 MB file costs the same as on column 5.

A real gap: **gpui-kit's helpers are wrong.**

- `RopeExt::position_to_offset` reads `Position.character` as a *char* index
  (`crates/base/src/input/base/rope_ext.rs:330-342`; the trait's docs at
  `:192-197` say "The column is in characters"). That is right only under utf-32, which nothing
  negotiates there.
- Its `apply_lsp_edits` applies `TextEdit`s first to last against the already
  edited text (`lsp/mod.rs:117-131`).

Copying gpui-kit's `DiagnosticSet` (§7) must drop both.

### 4.4 The UTF-16 emoji tests

**Cases.** Each case is a document and a byte offset in it, with the expected
LSP position in each encoding. Rows and columns are 0-based. All three
encodings are listed, so utf-32 is covered if it is ever offered. Expected
columns were computed by encoding the prefix (`len()`, `encode_utf16().count()`,
`chars().count()`).

| # | Case | Text (escaped) | Offset is before | utf-8 | utf-16 | utf-32 |
|---|---|---|---|---|---|---|
| 1 | astral emoji | `a\u{1F600}b` | `b` (byte 5) | 0:5 | 0:3 | 0:2 |
| 2 | two astral in a row | `\u{1F600}\u{1F600}x` | `x` (byte 8) | 0:8 | 0:4 | 0:2 |
| 3 | ZWJ family | `x\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}y` | `y` (byte 19) | 0:19 | 0:9 | 0:6 |
| 4 | skin-tone modifier | `\u{1F44D}\u{1F3FD}x` | `x` (byte 8) | 0:8 | 0:4 | 0:2 |
| 5 | flag (regional indicators) | `\u{1F1E8}\u{1F1E6}x` | `x` (byte 8) | 0:8 | 0:4 | 0:2 |
| 6 | variation selector | `\u{2764}\u{FE0F}x` | `x` (byte 6) | 0:6 | 0:2 | 0:2 |
| 7 | combining mark | `e\u{301}x` | `x` (byte 3) | 0:3 | 0:2 | 0:2 |
| 8 | precomposed BMP | `\u{E9}x` | `x` (byte 2) | 0:2 | 0:1 | 0:1 |
| 9 | CJK (3-byte BMP) | `\u{4E2D}x` | `x` (byte 3) | 0:3 | 0:1 | 0:1 |
| 10 | emoji on a later row, CRLF file | file bytes `a\r\n\u{1F600}b\r\n`, loaded as `a\n\u{1F600}b\n` | `b` (byte 6 of the loaded text) | 1:4 | 1:2 | 1:1 |
| 11 | past end of line | `a\u{1F600}\nz` | position 0:99 → offset 5 (row end), every encoding | — | — | — |
| 12 | past end of text | `a\u{1F600}\nz` | position 9:0 → offset 7 (text end), every encoding | — | — | — |
| 13 | split code unit | `a\u{1F600}b` | utf-16 0:2 (between the surrogates) → offset 1; utf-8 0:3 (inside the emoji's bytes) → offset 1 | — | — | — |

**How the tests check them.**

- **Unit tests in `fe-lsp`.**
  - For cases 1–10, under each encoding: `to_lsp(text, offset, enc)` equals the
    expected position, and `from_lsp(text, expected, enc)` equals the offset.
    Also, the row end (offset of `\n`, or the text end) round-trips.
  - Cases 11–13 check `from_lsp` only. They are clamps: positions a server may
    send but we never produce.
  - Case 10 loads its bytes through `Text::from_file_contents`, so the CRLF
    normalisation is part of the test.
  - The existing `fe-text` test (`utf16_points_handle_astral_chars`) already
    pins case 1 and the clamps, for utf-16 only.
- **Sync round-trip, fake server, in the gate.** Random edit batches
  (insertions and deletions of strings drawn from the cases above, including
  splitting and joining rows) run against a buffer synced to the fake server.
  The fake applies each `contentChange` to its own `String`, in the negotiated
  encoding, with a reference converter written independently over
  `str::encode_utf16` and `char_indices`. After every `didChange` its text must
  equal the buffer's. This runs once with the fake negotiating utf-8 and once
  with utf-16. A proptest at a small case count keeps the gate fast. For more
  cases, `scripts/soak.sh` gains a separate `fe-lsp` line that passes the
  proptest's name as a filter. Its current `-- --include-ignored` run would
  also pick up the `#[ignore]`d `real_server_*` tests (§6.3).
- **Real servers, slow group.** rust-analyzer under utf-8 and under utf-16
  (offer `["utf-16"]`), and pyright and typescript-language-server (utf-16),
  each run a definition across cases 1, 3 and 7. The expected columns are the
  ones the probe measured (§4.2). The use site must not be at a line's end, so
  a past-end clamp can't pass the test by accident.

## 5. Hand-off from search-based navigation

The fe-nav cards are filed (`rgt task search nav`, `rgt task search
NavigationProvider`, 2026-10-06):

- **T102**: definitions in `fe-syntax`.
- **T103**: the `fe-nav` symbol index.
- **T104**: keeps the index current from watcher batches.
- **T105** (rev 2): the `NavigationProvider` seam and `SearchNavigation`, with
  definitions and workspace symbols.
- **T106** (rev 2): adds `references`, with a streaming sink.
- **T107** (rev 6): the workbench UI and `Workbench::navigation()`.
- **T108**: measurements.
- **T112** (rev 1): the references panel.
- **T113**: outline and definition symbols for eight more languages (index
  coverage only).
- **T114** (rev 1): cmd-click go-to-definition, built on T107 and T112.

T107, T112 and T114 consume `Navigation` and pick up the LSP provider
unchanged.

`fe-lsp` aligns with T105's seam as written:

- **Precedence.** `Navigation::add(provider, precedence)`. `LspNavigation` is
  added at **100**; `SearchNavigation` sits at 0. One `LspNavigation` serves
  every language, and asks the `LspManager` which server, if any, handles
  `NavRequest.language`.
- **Readiness.** `readiness(language)`:
  - `Ready` when that language's server for the workbench's root is `Ready`
    (§3.4). T105's `readiness(language)` gets no root. That is fine because
    M4 is single-root and `LspNavigation` is one provider per workbench, bound
    to its root. Multi-root would need a root in `readiness` (gap 4 below).
  - `NotYet` while it is `Starting` or `Restarting`
  - `Unsupported` when no server is configured for the language, LSP is
    disabled, or the server is `Stopped`

  T105's router skips anything not `Ready`. So until the server is ready,
  search answers within its 100 ms budget, and after that LSP answers.
- **Calls block** on the caller's background thread, as T105 requires
  ("an async provider blocks on its own response inside the call, so `fe-nav`
  stays runtime-free"). `definitions` does four things:
  1. Hands the request to the actor with its `(BufferId, Version)` (gap 2).
     The actor drains and admits it as in §4.1: if the buffer has moved past
     the request's version, the answer is `Unavailable`, and search answers on
     `request.text`.
  2. Sends `textDocument/definition` with the caret converted against the
     ring's text for that version, which is `request.text`.
  3. `block_on`s the reply, polling `cancel`.
  4. Converts each `Location` or `LocationLink` against the snapshots
     captured at admission (see **Offsets**).
- **Errors.** Timeouts, `ServerDown`, `ContentModified` and a file too large
  to sync all return `Err(Unavailable(..))`, so T105's router asks search
  next. A provider that doesn't implement an operation returns `Unsupported`.
- **Replace, not merge** (T105: "one provider answers each request").
  - A non-empty LSP answer is final.
  - An empty one is final only from a server in `server-status` mode whose
    latest status is `quiescent: true` (§3.4, **Empty answers**). In every
    other case the provider returns `Err(Unavailable(..))`, and search answers.
  - So an empty answer from a quiescent rust-analyzer isn't second-guessed by
    search, and no heuristic can turn indexing into a final "no definition".

  The UI labels answers by `Answer.precision`, so "by search" disappears once
  LSP answers.
- **Stale requests** are cancelled by the caller setting the old request's
  `AtomicBool` (T105, T107 C7). The provider sends `$/cancelRequest` and
  returns `Err(Cancelled)`.
- **References.** `references(req, cancel, sink)` sends `textDocument/references`
  (`includeDeclaration: true`). When the reply arrives, it emits
  `FileReferences` batches, one per file, the request's file first (as T106
  does). The fields are filled like this:
  - `verified: true`
  - `is_definition` when the range equals a definition location (a
    `definition` request runs alongside, and it costs one more round trip)
  - `preview` and `preview_ranges` from the target file's line
- **Workspace symbols.** `workspace/symbol` maps onto `WorkspaceSymbol`.
  `indices` is computed locally, with the same fuzzy matcher T105 uses, so
  highlighting looks the same as search's.
- **Offsets.** T105's `Location.range` is bytes in the file's *loaded* text
  (LF-normalised, T103's convention), and LSP rows and columns map onto that
  text exactly (§4.1).
  - **The request's own file.** Converted against the ring's text for the
    request's version, which is `request.text`. So same-file offsets index
    `request.text`, as T105 requires.
  - **Other open documents.** Converted against the ring entry the actor
    captured for each at admission: the text the server held when it got the
    request. The location then carries that `(BufferId, Version)` (gap 3).
    Before jumping, the UI rebases the range with `map_offset` from that
    version, or uses the row when the version is below the edit-log floor.
  - **Unopened files.** Read the way the loader does (`Text::from_reader`), on
    the calling thread, once per file per answer. The UI opens them fresh, so
    there is nothing to rebase.

**Seam gaps.** Three for the lead to close in T105 or in the `fe-lsp`
navigation card:

1. **`Definition.kind` is a required `OutlineKind`, and `tier` is required.**
   LSP locations carry neither. The proposal:
   - `kind` becomes `Option<OutlineKind>`. The alternative, looking the range up
     in `fe_syntax::symbols()` of the target file, means a parse per target.
   - `LspNavigation` sets `tier: SameFile` for the request's own file and
     `Project` otherwise.
2. **`NavRequest` has no buffer identity or version.** The LSP provider must
   know exactly which text the caret offset refers to.
   - Proposal: `buffer: Option<(BufferId, fe_buffer::Version)>` on
     `NavRequest`, set by T107 from the active editor's snapshot, which
     `request.text` is taken from. Admission is defined in §4.1.
   - With `None` (a host request without a buffer), `LspNavigation` returns
     `Unavailable`, and search answers. There is no length or content
     heuristic.
3. **`Location` can't say which buffer version its range belongs to.**
   Proposal: `snapshot: Option<(BufferId, fe_buffer::Version)>` on `Location`.
   `None` means the file's loaded text on disk (search's case, and unopened
   files). T107 and T112 rebase through it before selecting.
4. **Not needed in M4: `readiness` has no root.** Multi-root workspaces would
   need `readiness(language, root)`, or one provider per root.

## 6. Testing

### 6.1 A fake in-process server, for the gate

**Built on** async-lsp's server side (`MainLoop::new_server` plus `Router`),
the crate we already depend on. It is wired to the client through an in-memory
duplex: two `piper` pipes (`piper` 0.2.5 is already in the lock), or a
~40-line pair over `futures::channel::mpsc`. There is no subprocess.

**Plugged in** through a `FakeLauncher: ServerLauncher` that returns that
duplex as `ServerIo`, so the launcher seam is tested too. It runs on the same
per-server thread as a real server.

**What it supports:**

- **Initialize.**
  - a scripted `InitializeResult`: chosen `positionEncoding`, sync kind,
    capabilities
  - a delay before replying
  - failing initialize
- **A document replica.** It applies `didOpen`, `didChange`, `didSave` and
  `didClose` with the independent reference converter (§4.4). Tests can read
  the replica's text, the version of each document, and the order of saves
  relative to changes.
- **Scripted replies.** Definition, references, workspace/symbol, hover and
  completion each get a canned reply, a delay, or no reply at all. Cancelled
  requests are either answered with `RequestCancelled`, or ignored forever
  (for the orphan budget of §3.5).
- **A record of every message received,** including `$/cancelRequest` ids and
  the request ids, which checks the id-mirroring of §2.1.
- **Server-initiated traffic.** It can send `$/progress` begin and end,
  `experimental/serverStatus`, `publishDiagnostics` for a chosen version,
  `workspace/configuration`, `window/workDoneProgress/create`,
  `client/registerCapability` and `unregisterCapability` (supported and
  unsupported methods), `workspace/applyEdit`, and an unknown notification (it
  must not stop the loop).
- **Crash on cue.** It closes the transport (EOF), or answers `exit` with a
  non-zero code, for the restart and backoff tests. Backoff tests use a
  manual clock: the delays come from an injected `Clock`, not
  `async_io::Timer`.

The gate's `fe-lsp` tests all use it:

- lifecycle: initialize, both readiness modes and the empty-answer rule,
  shutdown, crash, the rolling restart window, give-up
- sync: the round-trip proptest, request admission, diagnostics versioning
  and incarnations, the save barrier, and the nudge and backstop wakeups
- settings: the project-layer trust filter
- encoding: the §4.4 unit tests
- cancellation, timeouts and the orphan budget
- server requests and dynamic registration
- `LspNavigation` against T105's router

They need no Node, no network and no server binary.

### 6.2 Real servers

| Server | Version pinned for M4 | Licence | macOS dev machine | `macos-15` CI runner |
|---|---|---|---|---|
| rust-analyzer | The rustup component of the pinned toolchain (1.98.1, `rust-toolchain.toml`), so its version follows toolchain bumps. The probe used the standalone release `2026-10-05`, `0.3.3073`; the card re-runs the checks with the component | MIT OR Apache-2.0 (`Cargo.toml`; `LICENSE-MIT`, `LICENSE-APACHE`) | `rustup component add rust-analyzer` (once); tests run `rustup which rust-analyzer` | `scripts/lsp-real.sh` runs the same `rustup component add`. The workflows cache `~/.cargo` and `target` (`Swatinem/rust-cache`), not `~/.rustup`, so it downloads on each run (tens of MB), unless `lsp.yml` adds an `actions/cache` step for `~/.rustup/toolchains` keyed on `rust-toolchain.toml`. It isn't added to `rust-toolchain.toml`'s `components`, so everyday builds don't download it |
| pyright | 1.1.414 | MIT | `npm ci --prefix tools/lsp` from a checked-in `tools/lsp/package.json` and `package-lock.json` (exact versions) | Same `npm ci`. Node comes from `jdx/mise-action` reading `tools/lsp/mise.toml`. The npm cache is keyed on `hashFiles('tools/lsp/package-lock.json')` plus the Node version |
| typescript-language-server | 6.0.1 | Apache-2.0 | same `tools/lsp` lockfile | same |
| typescript | 6.0.3, the last release with `lib/tsserver.js` | Apache-2.0 | same `tools/lsp` lockfile | same |
| Node | 24 LTS, exact patch in `tools/lsp/mise.toml` (mise is how this Mac gets Node) | MIT | `mise install` in `tools/lsp` | `jdx/mise-action` |

**TypeScript 7.** npm's `typescript` `latest` is now 7.0.2, the Go port, and it
ships no `tsserver.js`. typescript-language-server 6.0.1 then fails
`initialize` with -32603: "Could not find a valid TypeScript installation"
(`src/lsp-server.ts:133`). So `typescript` must be pinned to 6.0.3. TS 7's own
server (`tsc --lsp --stdio`, serverInfo `typescript-go 7.0.2`) worked in the
probe, negotiated utf-8, and exits with code 1 after `exit` (§3.4). Which
server is the default is Q5 (typescript-go).

Whether fastedit ever installs or bundles servers for users is Q2. The
`tools/lsp` lockfile is for tests only.

### 6.3 Gate versus slow group

- **Gate** (`cargo nextest run --workspace`, and `cargo test --workspace` in
  CI): all fake-server tests and the §4.4 unit tests. No Node, no network, no
  server binaries.
- **Slow group:** real-server tests are `#[ignore = "needs language servers:
  scripts/lsp-real.sh"]`, named `real_server_*`. This is the pattern the soak
  tests already use (`scripts/soak.sh`, `.github/workflows/soak.yml`).
  `scripts/lsp-real.sh` installs as in §6.2, then runs
  `cargo nextest run -p fe-lsp --run-ignored only -E 'test(/real_server_/)'`.
  The tests are:
  - definition and references through `LspNavigation` against the probe's
    fixtures
  - the emoji columns of §4.4
  - readiness from launch to `Ready` within 60 s
  - clean shutdown
- **A separate workflow,** `.github/workflows/lsp.yml`, on `macos-15`, with a
  45-minute timeout. Its trigger is Q4: `workflow_dispatch` only, with no
  `schedule`.
- **Skipping cleanly.** Each real-server test first resolves its binary:
  - `FE_LSP_<SERVER>` env var, then `tools/lsp/node_modules/.bin/…`, then
    `rustup which`
  - if missing, it prints `skipped: pyright-langserver not found (run
    scripts/lsp-real.sh)` and returns without failing
  - the workflow sets `FE_LSP_REQUIRE_SERVERS=1`, which turns a missing
    server into a failure, so CI can't go green by skipping everything

## 7. Feature scope for M4

| Feature | LSP methods and client capabilities | `fe-lsp` | `fe-editor` / `fe-display` | `fe-gpui` / `fe-workbench` |
|---|---|---|---|---|
| Diagnostics | push `textDocument/publishDiagnostics`; `publishDiagnostics { versionSupport, relatedInformation, tagSupport [Unnecessary, Deprecated], codeDescriptionSupport }`. Pull `textDocument/diagnostic` for open documents (T39, below); `diagnostic { dynamicRegistration: false, relatedDocumentSupport: false }`, `workspace.diagnostics { refreshSupport: true }` | Converts to `fe_lsp::Diagnostic { range: Range<usize>, severity, message, source, code, tags }` decoded in the ring's text for the published version (§4.1). Emits `LspEvent::Diagnostics { buffer, session, incarnation, version, server, items }`, with ranges in that buffer version, and `LspEvent::DiagnosticsCleared { buffer, session, incarnation, server }` at close and session end. Unversioned publications are dropped unless the spec names a verified guarantee (§4.1) | **`DiagnosticSet` copied from gpui-kit** (`editor/diagnostics.rs:186`, as the plan's reuse map says), but holding `fe-buffer` anchors over byte ranges, not `lsp_types::Position` ranges, and keyed by server. On delivery, a `Diagnostics` is applied only if its `(session, incarnation)` is the active one (`LspManager::active_identity`) and not already cleared; its ranges are then rebased from the event's version with `map_offset` and anchored. A `DiagnosticsCleared` (whose identity is already retired) empties the server's set only if the set carries that identity, and records the identity as cleared. Lives in `fe-editor` (GPUI-free) | Squiggles and gutter marks from the set; the message on hover. Plain text in M4 |
| Definition | `textDocument/definition`; `definition.linkSupport = true` | `LspNavigation::definitions` (§5) | — | T107's F12 and picker, unchanged |
| References | `textDocument/references` | `LspNavigation::references` (§5) | — | T112's panel, unchanged |
| Workspace symbols | `workspace/symbol` (`symbol.symbolKind` the full set) | `LspNavigation::workspace_symbols` | — | T107's cmd-T, unchanged |
| Hover | `textDocument/hover`; `hover.contentFormat = [markdown, plaintext]` | `hover(doc, offset) -> Future<Option<Hover { text, range: Range<usize> }>>`. Markdown is kept as a string | — | Popover after a hover delay, and a `editor.show_hover` command. Markdown shown as text with fenced code in monospace; no rich markdown renderer in M4 |
| Completion | `textDocument/completion` and `completionItem/resolve`; `completionItem { snippetSupport: false, insertReplaceSupport: false, resolveSupport: [documentation, detail], labelDetailsSupport: true }`; server `triggerCharacters` | `completion(doc, offset, trigger)` and `resolve(item)`. Converts `textEdit` and `additionalTextEdits` to byte ranges in the request's version | Completion session: filtering, selection, accept. Accept applies the main and additional edits as **one `Origin::User` transaction** through `edit_from` on the request's snapshot | The menu |
| Lifecycle and status | §3 | `LspManager` | — | Status item, `lsp.restart`, `lsp.show_log` |

**Out of M4:**

- code actions, rename, formatting
- semantic tokens, inlay hints, signature help
- document highlights, code lens, folding ranges, call and type hierarchy
- workspace pull diagnostics (`workspace/diagnostic`), related documents,
  result ids and dynamic registration of `textDocument/diagnostic`
- snippets, `workspace/executeCommand` (but for typescript-language-server's
  diagnostics, §4.1), applying `workspace/applyEdit`
- `workspace/didChangeWatchedFiles`
- multi-root workspaces
- diagnostics for files that aren't open (a problems panel)

`didSave` is in M4 (§4.1). It drives rust-analyzer's `checkOnSave`
diagnostics.

**Pull diagnostics (T39, `crates/fe-lsp/src/pull.rs`).** Added for
rust-analyzer, whose own diagnostics are pull-only, and typescript-go, which
pushes none for source files (§10). Open documents only:

- **Who.** A session pulls when its `InitializeResult` has
  `diagnosticProvider` (options or registration options); otherwise nothing
  changes for it. The client offers `textDocument.diagnostic
  { dynamicRegistration: false, relatedDocumentSupport: false }` and
  `workspace.diagnostics { refreshSupport: true }`. lsp-types 0.95.1 names
  the latter `workspace.diagnostic`, so `initialize` goes out as JSON with
  the protocol's name. Of the four servers, rust-analyzer and typescript-go
  pull (§10).
- **When.** After a document's `didOpen`, after each flushed `didChange`,
  after `didSave`, when the session turns `Ready`, and on
  `workspace/diagnostic/refresh` (every open document of the session). The
  triggers of one drain make one pull, queued right behind its
  notifications. At most one pull per document is in flight: a trigger
  meanwhile marks the document, and it's pulled once more when the reply
  lands. A pull sends `{ textDocument, identifier }` (`identifier` from the
  server's `DiagnosticOptions`, left out when it has none), never
  `previousResultId` (left out, not `null`: typescript-go refuses a
  `null`), and is cancelled after 30 s.
- **Which version.** The LSP version of the last `didOpen` or `didChange`
  sent before the request, since the server takes them in order. The reply
  is decoded like a versioned publication (§4.1): against the ring's text
  for that version, and dropped if the version has left the ring, if a newer
  pull reply was applied, or if its incarnation has ended (closed, reopened,
  or its session restarted). A `full` report gives the items; an `unchanged`
  report, a partial result and an error (`ServerCancelled`,
  `ContentModified`, the timeout) are dropped, and the next trigger pulls
  again.
- **Pushed and pulled.** Each incarnation keeps its latest accepted pushed
  list and pulled list, each at its LSP version and each with its own
  older-than-newest rule. Whenever either changes, one
  `LspEvent::Diagnostics` holds both, at the newer version. The older
  list's ranges are mapped through the ring's `edits` to it, start biased
  right and end biased left as `DiagnosticSet::apply` maps them; if its
  version has left the ring (or a gap broke its edits), it's left out until
  it's replaced. `DiagnosticsCleared` and a session's end clear both. No UI
  change: fe-editor still sees one list per (buffer, server).

Watching files changed outside fastedit is a separate question. Without
`workspace/didChangeWatchedFiles`, it isn't yet known whether rust-analyzer
watches the disk itself (§10). Card 2 or 4 checks this; if it doesn't, a later
card feeds fe-project's watcher batches in.

## 8. Proposed fe-lsp cards

In order. Each card is GPUI-free unless marked (GPUI), and none adds a
dependency beyond §2's.

1. **fe-lsp skeleton: server thread, `ServerLauncher`/`LocalLauncher`, initialize and shutdown, fake server.** Creates the crate with `async-lsp =0.2.4` and `lsp-types =0.95.1`, the router overrides and the id-mirroring test (§2.1). Depends on Q1 and Q3.
2. **Position encodings and document sync.** Negotiation, `to_lsp`/`from_lsp` with the §4.4 cases, and incremental didOpen/didChange/didSave/didClose drained by the actor, with the full-text fallback, the version ring, request admission, the 50 MB limit and the round-trip proptest. Depends on 1.
3. **Server registry and `[lsp]` settings.** Built-in specs for the three servers, a server per file, `workspace/configuration` answers, the project-layer trust filter, and lazy start per (server, root) in `fe-config` and `fe-lsp`. Depends on 1, Q6 and Q7.
4. **Readiness, crash restart, timeouts and cancellation.** Both readiness modes and the empty-answer rule, the rolling restart window behind an injected clock, re-opening documents, `$/cancelRequest`, timeouts and the orphan budget, dynamic registration, and status and log events. Depends on 2 and 3.
5. **Wire fe-lsp into the workbench (GPUI).** One `LspManager` per workbench, buffers opened with their subscriptions and nudged per frame, events over `async_channel`, a status item, and `lsp.restart` / `lsp.show_log`. Depends on 4 and T107.
6. **LSP navigation provider.** Definition, references and workspace symbols at precedence 100 through T105's seam, plus §5's seam additions and the manual check "go-to-def before and after the LSP is ready". Depends on 5, T105, T106 and T107; T112 and T114 use it unchanged.
7. **Diagnostics (GPUI for rendering).** gpui-kit's `DiagnosticSet` copied into `fe-editor` on anchored byte ranges (§4.3), the active-identity check and rebasing of versioned ranges on delivery (§4.1), and squiggles, gutter marks and hover messages in `fe-gpui`. Depends on 2 and 5.
8. **Hover popover (GPUI).** `textDocument/hover` and a popover that shares card 7's hover surface. Depends on 5.
9. **Completion (GPUI for the menu).** Request, resolve, menu, and accept as one `Origin::User` transaction through `fe_buffer::Buffer::edit_from` (`crates/fe-buffer/src/buffer.rs:346`). Depends on 5.
10. **Real-server tests and `lsp.yml`.** The `tools/lsp` lockfile and mise pin, `scripts/lsp-real.sh`, the `real_server_*` tests and emoji columns (§6), and M4's "precisely via rust-analyzer, pyright and tsserver" check, which the headless `LspNavigation` definition tests stand in for (the in-app F12 check is human card T323). Depends on 6 and Q4/Q5; the workflow change needs the user's approval under Regatta's rules.

## 9. Questions for the user

**Q1. Who owns language-server processes when fastedit is embedded?**

- **(a) fastedit spawns by default, and the host may replace the launcher.**
  `LspManager` takes an `Arc<dyn ServerLauncher>`. Standalone fastedit passes
  `LocalLauncher`, and the host passes its own, or `LocalLauncher`. Downstream:
  §3.1 as written. The host can sandbox or proxy without forking `fe-lsp`.
- **(b) The host must always supply servers.** There is no `LocalLauncher` in
  `fe-lsp`; it moves to the `fastedit` binary. Downstream: fe-lsp can't spawn
  anything itself, and the fastedit binary carries the launcher.
- **(c) fastedit always spawns.** There is no seam. Downstream: simpler, but
  the host can't containerise servers, and adding the seam later changes
  `LspManager::new`.

Recommendation: **(a)**.

**Decided (user, 2026-10-06): (a).**

**Q2. Does fastedit ever install or bundle language servers?**

- **(a) Never in M4.** Servers come from `PATH`, from `[lsp.servers.*]
  command`, or from the host. Missing servers show "stopped: rust-analyzer
  not found", and navigation stays search-based. Downstream: nothing to ship.
- **(b) Download on demand into fastedit's data directory** (pinned versions
  and checksums). Downstream: a downloader card, and the Node runtime
  question for pyright and typescript-language-server.
- **(c) Bundle in the app.** The licences allow it: rust-analyzer is MIT or
  Apache-2.0, pyright MIT, typescript-language-server and typescript
  Apache-2.0, Node MIT. Downstream: app size (Node alone is tens of MB), a
  notices update in `THIRD_PARTY.md`, and an update channel.

Recommendation: **(a)**, revisited when the host's packaging is known. The host
probably wants to own this anyway (Q1).

**Decided (user, 2026-10-06): (a).**

**Q3. May a core crate depend on tokio?**

- **(a) No.** `fe-lsp` uses async-io and async-process on its own threads
  (§2.2). Downstream: as recommended.
  - The macOS build compiles 7 more crates (6 runtime crates plus
    `serde_repr`), all already in the lock.
  - The id-mirroring workaround for cancellation is needed.
- **(b) Yes,** a `current_thread` tokio runtime per server thread, with
  async-lsp's `tokio` feature. Downstream: no new lock lines, but tokio
  (process, io, time) becomes a normal dependency of a core crate. It also
  seems pointless while async-lsp works without it.
- **(c) No async at all.** A hand-written client on `std::process` and threads
  (§2.1 fallback). Downstream: ~500 more lines in card 1, and async-lsp is
  dropped.

Recommendation: **(a)**.

**Decided (user, 2026-10-06): (a).**

**Q4. When do real-server tests run in CI?**

- **(a) Nightly and on demand** (`schedule` plus `workflow_dispatch` in
  `lsp.yml`). Downstream: a server regression shows up within a day, and PRs
  stay as fast as today.
- **(b) On every PR**, as a separate job. Downstream: every PR waits on Node
  setup, `npm ci` and rust-analyzer indexing, and gets flaky if a server is
  slow.
- **(c) On demand only,** like `soak.yml`. Downstream: cheapest, but
  regressions wait until someone runs it.

Recommendation: **(a)**. The gate stays free of Node and network either way.

**Decided (user, 2026-10-06): (a).** **Changed (user, 2026-10-10): (c).**
`lsp.yml` runs on `workflow_dispatch` only; a `schedule` can be added later.

**Q5. Which TypeScript server is the default?**

- **(a) typescript-language-server 6.0.1 with `typescript` 6.0.3** (tsserver).
  This matches M4's wording. Downstream: it needs a pinned TS 6, it is utf-16
  only, and the TS 6 line will age out.
- **(b) typescript-go 7.0.2** (`tsc --lsp --stdio`). Downstream: utf-8, one
  native binary, and it's where TypeScript is heading. Newer and chattier
  (about 35 log messages in 10 s), and it exits with code 1 after `exit`.
- **(c) Both configured,** with (a) the default and (b) a settings switch.
  Downstream: the real-server job tests both.

Recommendation: **(c)**.

**Decided (user, 2026-10-06): (c).** **Changed (user, 2026-10-11): (b).**
(T305 was later done as T73, 2026-10-12: §4.1.)
typescript-language-server never versions its diagnostics, so TypeScript and
JavaScript files showed none under the default (§10). typescript-go answers
`textDocument/diagnostic`, which T306 sends. typescript-language-server stays
behind `lsp.typescript_server`. T305, pulling its diagnostics through
`typescript.tsserverRequest`, is parked. typescript-go's two quirks need no new
handling, and the real-server run on Linux re-checked both: the exit code 1
after a clean shutdown is a normal stop (§3.4), and the lowercased
`tsconfig.json` path comes in one empty unversioned push that fe-lsp drops, with
definitions and references into other files at the right path.

**Q6. When do servers start?**

- **(a) Lazily, when the first file of their language opens.** Downstream:
  no rust-analyzer CPU load for a Python-only browse.
- **(b) When the project opens, for each language the project contains.**
  Downstream: precise results are ready sooner, at the cost of CPU and memory
  for languages never looked at.

Recommendation: **(a)**. Search covers the gap.

**Decided (user, 2026-10-06): (a).**

**Q7. Do language servers start in a workspace nobody has marked as trusted?**

Background: even with the project layer filtered (§3.3), a server run on a
repository may execute that repository's code by design. rust-analyzer runs
`build.rs` scripts and proc macros, and `cargo check` on flycheck.

- **(a) Yes, with project-layer LSP settings ignored.** The host can veto per
  root through `ServerLauncher::launch` returning an error. Downstream: it
  works like most editors' defaults, and no trust UI is needed in M4. Opening
  a hostile Rust repository can run its build scripts.
- **(b) Not until the user trusts the workspace.** A "trust this folder"
  prompt, remembered per root in user state. It also unlocks project-layer
  LSP settings. Downstream: a trust card (UI and storage) before LSP is
  useful, and search-only navigation until trusted.
- **(c) Leave it to the host entirely.** fastedit standalone behaves as (a),
  and the embedding host decides through its launcher. Downstream: as (a) in
  fastedit, with the policy owned by the AI dev tool.

Recommendation: **(c)**, which is (a) for standalone fastedit. A
workspace-trust feature can come later and lift the project-layer filter per
root.

**Decided (user, 2026-10-06): (c).**

**Revised (user, 2026-10-07): (b)**, without its unlocking of project-layer
LSP settings (T177). A push security review of T141 found that `LspManager::open`
starts a server for any document under a root, so opening a hostile repository
would run its code once the workbench drives fe-lsp (T143).

- **The gate.** `LspManager::set_trust(predicate)` says which roots may have
  servers. A routed document under a root it rejects waits, as one with no
  server for its language does. A root that becomes untrusted has its routed
  servers shut down, and nothing (settings, specs, a crash restart,
  `restart`) starts one there until it's trusted again. `start` and `open_on`
  aren't gated: there the host names the server. The default trusts every
  root, so embedding hosts keep (c); the workbench always installs a closed
  gate and opens it per root.
- **Asked on first open.** The first time a folder is opened as one (a folder
  launch, or File > Open Folder…), the workbench asks "Do you trust the
  authors of the files in this folder?" with Trust Folder and Don't Trust;
  Escape is a remembered Don't Trust. A launch with a file (`fastedit
  some/file`, or as `$EDITOR`) doesn't ask: its parent folder follows its
  nearest decided ancestor, and is untrusted if none has a decision.
- **Remembered per canonical folder** in user state,
  `<state_dir>/trust.toml` (`$FASTEDIT_STATE_DIR`, else
  `$XDG_STATE_HOME/fastedit`, else `~/.local/state/fastedit`), never in the
  project. A folder's subfolders inherit its decision unless they have their
  own: the nearest decision wins. A trust file fastedit can't read or parse
  loads as empty (every folder untrusted, fail closed) and is never
  overwritten. Writes go off the main thread, one at a time, and each applies
  only the decisions not yet written to the file as it is then, so another
  fastedit's decisions are kept; quitting waits for a write under way.
- **Changed later** with File > Trust This Folder and Untrust This Folder
  (`workbench.trust_folder`, `workbench.untrust_folder`).
- **Project-layer LSP settings stay filtered** even in trusted folders (§3.3).

## 10. Unanswered

- **Memory and CPU of real servers on large projects.** Not measured. The
  probe projects were one file each, and rust-analyzer's 2.3–3.3 s to quiet is
  for a one-crate project. The 500 ms settle, the flycheck exclusion and the
  "no upper bound" rule need checking on a large repository (this one, for
  example) in card 4 or card 10.
- **The 500 ms settle's margin on large projects.** The widest no-progress gap
  seen was 224 ms, just before flycheck. Larger projects may have wider gaps
  between rust-analyzer's phases, and a gap over 500 ms would latch `Ready`
  early in `progress` mode. Because empty answers in `progress` mode are
  never final, that costs only routing, not correctness. Not measured.
- **rust-analyzer's `experimental/serverStatus`: checked in card 4
  (2026-10-07).** A scratch harness (not committed) ran the 1.98.1 rustup
  component (`rust-analyzer 1.98.1 (48a229ce 2026-09-01)`) on this
  repository, with `experimental.serverStatusNotification = true` and
  `cargo.targetDir = true`, at a load average of about 260. Times are seconds
  after `initialize` was sent:
  - **It arrives.** `quiescent: false` at 0.15 s, with the first
    `Fetching` progress, then `quiescent: true` at 107.35 s. Those were the
    only two statuses in the 254 s run (`health` "ok" both times).
  - **`quiescent` waits for cache priming.** Startup ran Fetching, Building
    CrateGraph, Roots Scanned, Building compile-time-deps (2.4 → 62.3 s),
    Loading proc-macros, Fetching again, then cache priming ("Indexing",
    `rustAnalyzer/cachePriming`) from 90.17 to 107.32 s. `quiescent: true`
    came 30 ms after cache priming ended, with no token open.
  - **`quiescent` ignores flycheck.** Flycheck (`rust-analyzer/flycheck/0`,
    "cargo check") began 0.2 s after the quiescent status and ran to 127.5 s;
    a `didSave` started a second run from 127.8 to 241.8 s. No status came
    during either, so `quiescent` stayed true.

  Both checks hold, so the built-in rust-analyzer spec keeps
  `server-status` mode. For the 500 ms settle (next item): the gaps with no
  progress open in this run were 102, 0, 15, 1 and 76 ms during startup, and
  237 ms between cache priming and the first flycheck, so the progress rule
  would have latched at about 107.8 s here too, with flycheck ignored.
- **How often "search instead of empty" helps or hurts.** For pyright and
  typescript-language-server, a true "no definition" (a builtin, say) shows
  search's guesses, labelled "by search". Not measured for frequency.
- **Which servers version their diagnostics: checked in card 10
  (2026-10-09), re-checked with pull diagnostics in T39 (2026-10-11).** The
  real-server tests watch the wire and record it (`real_server_*` in
  `crates/fe-lsp/tests/real_server.rs`, run by `scripts/lsp-real.sh`;
  evidence in `docs/decisions/42t4r87mwk-pull-diagnostics.md`). Each test
  opens a file and adds an error without saving it, then watches 30 s for
  publications and pull replies holding a diagnostic for that file, and for
  the error reaching the sink at the edited version:
  - **rust-analyzer** (1.98.1 component) advertises `diagnosticProvider`
    and answers pulls. It pushes only `cargo check`'s diagnostics (`source`
    `rustc`), versioned, after a save. Its own come by pull and do reach an
    unsaved buffer: a type mismatch (`source` `rust-analyzer`, E0308) was
    shown 4 ms after the edit. Its unresolved-import diagnostic (E0432) is
    experimental, off unless `diagnostics.experimental.enable` is set in
    its `initializationOptions` or settings: under the built-in spec no pull
    reply holds it, so an unresolved import shows only after a save, as
    `rustc`'s E0432. With experimental diagnostics on, it reached an unsaved
    buffer by pull 25 ms after the edit. The first pull after an edit is
    often answered `ServerCancelled`; the next trigger pulls again.
  - **pyright** 1.1.414 advertises no `diagnosticProvider`, even with the
    client offering pull, and pushes versioned diagnostics as before (shown
    0.3 s after the edit).
  - **typescript-language-server** 6.0.1 advertises no `diagnosticProvider`
    and never sets `version` on its pushes. Unversioned publications are
    dropped (§4.1). It is no longer the default (Q5). **Since T73
    (2026-10-12)** its diagnostics are pulled through
    `typescript.tsserverRequest` (§4.1): on Linux the type error (TS2322)
    reached the sink at the edited version 23 ms after the edit, by
    `semanticDiagnosticsSync`, while its one push for the file still came
    unversioned and was dropped. The server source behind it
    (typescript-language-server 6.0.1 `lib/cli.mjs`, tsserver from
    `typescript` 6.0.3): `FileDiagnostics` merges its three kinds into each
    push without clearing them on a change; `SingleTsServer.fenceCommands`
    is `change`, `close`, `open` and `updateOpen`; `TSServerRequestCommand`
    maps the `file` URI to the open document's path and returns tsserver's
    response object. Run and evidence in
    `docs/decisions/rq81bj4nvm-tsserver-diagnostics.md`.
  - **typescript-go** 7.0.2 pushes none for source files and answers pulls:
    the type error (TS2322) was shown 8 ms after the edit. It refuses a
    pull whose `previousResultId` is `null` (`InvalidParams`), which
    lsp-types 0.95.1 serializes, so fe-lsp sends the params as JSON with the
    unset fields left out.
  - **Duplicates.** rust-analyzer's pushed and pulled lists repeat each
    other for a saved type mismatch: `rustc`'s E0308 ("mismatched types",
    plus its "expected … because of return type" note) and
    rust-analyzer's own E0308 ("expected &'static str, found i32") are all
    shown, one event holding both lists. No other server does both.
- **rust-analyzer from the 1.98.1 rustup component: checked in card 10
  (2026-10-09).** Version `rust-analyzer 1.98.1 (48a229ce 2026-09-01)`.
  - Offered `["utf-8", "utf-16"]`, it negotiates **utf-8**, as the
    2026-10-05 probe release did.
  - Offered `["utf-16"]` alone, it negotiates utf-16.
  - Under each, definitions of §4.4 cases 1, 3 and 7 land on §4.2's columns
    for that encoding: 25, 39, 24 under utf-8 and 23, 29, 23 under utf-16.
- **Launch to `Ready` on this repository: measured in card 10
  (2026-10-09).** A scratch run (not committed) used the built-in
  rust-analyzer spec on the 1.98.1 component, `server-status` mode, plus
  `cargo.targetDir = true`. It opened `crates/fe-lsp/src/lib.rs` at the
  workspace root. Ready came after 68.4 s with build scripts not yet built,
  and after 60.0 s with them built, at a load average of 24 to 91. The time
  went to:
  - compile-time deps: about 16 s
  - proc macros and fetching: about 22 s
  - cache priming ("Indexing"): about 18 s

  Each server's launch to `Ready` over its one-file fixture, from the same
  runs:
  - rust-analyzer: 3.5–23.7 s (the slowest under heavy load)
  - pyright: 0.6–1.3 s
  - typescript-language-server: 0.8–3.1 s
  - typescript-go: 0.5–0.8 s

  So the 60 s bound in the tests holds for the fixtures, but not for this
  repository: the first `Ready` here can take a minute or more.
- **rust-analyzer without client file watching: checked in card 4
  (2026-10-07).** In the same run, fe-lsp's capabilities (no
  `didChangeWatchedFiles`), a function appended on disk to a file the client
  never opened was found by `workspace/symbol` at the first query, 6.3 s
  later. So rust-analyzer watches the disk itself when the client doesn't
  offer file watching (§7).
- **Whether async-lsp upstream would take a cancel-on-drop or
  request-id change** (§2.1, option b). Not asked.
- **Cost of the cross-file offset conversion** for references in unopened
  files. Each referenced file is loaded once per answer (§5). Measured
  against T108's budgets: PERF.md, "Language server answers: locations in
  unopened files". All cases are well inside them.
- **The finished `PATH` resolution for Finder-launched apps** (§3.3). Noted,
  not designed.
- **Windows and Linux.** Process groups, `SIGCHLD` chaining and `PATH`
  lookup are macOS-only here. Linux already has async-process through GPUI;
  Windows is unexamined.
