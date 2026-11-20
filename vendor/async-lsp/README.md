# async-lsp 0.2.4, patched

This is the `async-lsp` crate 0.2.4 from crates.io, built from upstream commit
[`da6ef29`](https://github.com/oxalica/async-lsp/commit/da6ef291e5d9b8a7508548a02cb0d7b78e0cf4b4),
with the patch below. `Cargo.toml` at the repository root uses it in place of
the crates.io crate through `[patch.crates-io]`. It's `MIT OR Apache-2.0`; see
`LICENSE-MIT` and `LICENSE-APACHE`. It ships no NOTICE file.

## Why

Plugins send language-server requests and notifications through `lsp.request`
and `lsp.notify` (plugins stage 3 §3.6,
`docs/decisions/dqsjfernkt-plugins-stage-3.md`): any method the server defines,
so the method is a string known only at run time. In 0.2.4,
`ServerSocket::request::<R>` and `notify::<N>` send only a compile-time
`R::METHOD`, `AnyRequest` and `AnyNotification` are `#[non_exhaustive]`, and
the untyped `PeerSocket::send` is private, so there is no way to send a method
`lsp-types` has no type for.

## What changed

Every change is marked `fastedit patch` in a comment, and nothing else differs
from upstream.

`src/lib.rs`:

- `impl_socket_wrapper!` gives `ServerSocket` and `ClientSocket` two methods:
  - `request_raw(method: String, params: serde_json::Value) -> Result<serde_json::Value>`
    (async), which sends a request with that method and returns its reply's
    `result`, `null` when the reply has none, or `Error::Response` for an error
    reply.
  - `notify_raw(method: String, params: serde_json::Value) -> Result<()>`, which
    queues a notification with that method.
- `PeerSocket` gets the two private sends behind them. They build an
  `AnyRequest` (id `RequestId::Number(0)`, which `dispatch_event` replaces, as
  for a typed request) or an `AnyNotification`, and send it through the same
  `MainLoopEvent::OutgoingRequest` and `MainLoopEvent::Outgoing` events as
  `request` and `notify`. So a raw request's wire id comes from the same counter
  as a typed one's, and fe-lsp's mirror of that counter
  (`docs/decisions/lsp-client.md` §2.1) holds for both. The reply decodes as in
  `PeerSocketRequestFuture`, which the raw request reuses.

Besides the patch, this copy differs from the crates.io package in a few ways.
The packaged files are kept except the cargo metadata files
(`.cargo_vcs_info.json`, `.cargo-ok`, `Cargo.toml.orig`, `Cargo.lock`), and
this README replaces upstream's. `Cargo.toml` is the package's manifest,
unchanged, so the version stays 0.2.4 and the workspace's `=0.2.4` pin holds.

## When to drop it

Drop this directory and the `[patch.crates-io]` entry once an async-lsp release
has a public untyped send: a way to send a request and a notification whose
method is a run-time string, through the same id counter as typed requests.
Bump the workspace dependency to that release, move fe-lsp
(`crates/fe-lsp/src/custom.rs`, `crates/fe-lsp/src/notify.rs`) to its API, and
check that fe-lsp's `raw_methods` tests and fe-workbench's `plugin_lsp_raw_tests`
still pass. Since fe-lsp mirrors the request-id counter, check the release
still numbers outgoing requests from 0, one per request, as 0.2.4 does. Until
then, an async-lsp bump means re-applying this patch to the new version's
files.
