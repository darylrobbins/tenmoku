# Any LSP method from plugins (T1963)

T1701 gave plugins `lsp.request` and `lsp.notify` (plugins stage 3 §3.6,
`dqsjfernkt-plugins-stage-3.md`), but async-lsp 0.2.4 sends only methods typed
at compile time, so fe-lsp kept allowlists (`raw_requests!` and `raw_notices!`
in `crates/fe-lsp/src/custom.rs`, `raw_notifications!` in
`crates/fe-lsp/src/notify.rs`) and answered `Unavailable` for anything else.
This card vendors async-lsp with an untyped send and drops the allowlists.

## The async-lsp patch

`vendor/async-lsp/` is the crates.io package of 0.2.4 (upstream `da6ef29`),
wired in through `[patch.crates-io]`, with `request_raw` and `notify_raw`
added to `ServerSocket` and `ClientSocket` in `impl_socket_wrapper!`, and two
private sends behind them on `PeerSocket`. They build `AnyRequest` and
`AnyNotification` and send through `MainLoopEvent::OutgoingRequest` and
`MainLoopEvent::Outgoing`, as the typed sends do, so a raw request's id comes
from the one counter fe-lsp mirrors (`lsp-client.md` §2.1). The raw reply
reuses `PeerSocketRequestFuture<serde_json::Value>`, which decodes
`resp.result.unwrap_or_default()` as for a typed request: a missing result is
`null`. Every change is marked `fastedit patch`. `vendor/async-lsp/README.md`
says when to drop it: a release with a public untyped send.

## fe-lsp

- **Requests.** `requests::call_raw` builds the same `Call` issuer `call` does,
  with `server::send_raw_request` as its send. That shares `send_request`'s
  single poll at issue (`polled_once`), so ids follow issue order; the actor
  takes the mirrored id as for any request, and a dropped or timed-out caller
  cancels by that wire id.
- **Timeouts.** No method a plugin may send has an `lsp.timeouts` entry: the
  table names only `textDocument/`, `workspace/`, `completionItem/` and like
  methods, all of which fe-plugin's `is_routed` denies. So a custom request
  keeps T1701's `None` timeout, and the plugin host's call deadline is what
  drops (and cancels) it. Looking the method up in the table would need the
  manager's settings, and `manager.rs` is outside this card's scope; it would
  change nothing for plugin traffic.
- **Notifications.** `Command::Notify`'s method is now a `Cow<'static, str>`.
  The editor's own (borrowed) methods still pass through
  `watched_files::for_session`, which turns the internal `fe-lsp/filesChanged`
  marker into `workspace/didChangeWatchedFiles` filtered by the session's
  registrations. A plugin's (owned) method is sent as named and is never taken
  for that marker. Both are sent with `notify_raw` from `notify::send` and
  followed by `Taken`, so they ride the `QUEUED_MAX` window in order with what
  was queued before them. An unknown method used to send nothing (and emit no
  `Taken`); every method is sent now.
- **Test-only method names.** `Issuer::method` (cfg(test)) returns
  `&'static str`, and `tsserver.rs` (out of scope) returns its wrapped
  issue's through it, so the signature stays. A raw call interns its method
  in a test-only table, one leaked string per distinct method.
- **No new gate.** fe-lsp sends whatever it is handed; fe-plugin's `is_routed`
  denylist, the `lsp:<server>` grant, the (server, root) session choice, the
  pending cap and the 16 MiB limits are unchanged from T1701.

T1701's workbench test that expected `experimental/unknownMethod` to fail now
checks a routed method (`textDocument/hover`) is denied instead, since any
other method goes through.

## Licensing

async-lsp is `MIT OR Apache-2.0`. `LICENSE-MIT` and `LICENSE-APACHE` are kept in
`vendor/async-lsp/`. It ships no NOTICE file, so NOTICE is unchanged: there is
no NOTICE text of upstream's to carry, and Apache-2.0 §4(d) asks only for that.
THIRD_PARTY.md has footnote ¹⁷ under Dependencies and counts ten vendored
crates; CLEAN_ROOM.md's patched table, now "Patched crates", has its row.
`cargo deny check licenses bans sources` passes with no `deny.toml` change.
