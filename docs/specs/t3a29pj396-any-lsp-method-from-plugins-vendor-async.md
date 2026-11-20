---
task_uid: "01m7rb1zr6jdv451t3a29pj396"
task: "T1963"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "async-lsp 0.2.4 is vendored at vendor/async-lsp/ and used through `[patch.crates-io]` in Cargo.toml, beside tree-sitter and tree-sitter-yaml. The packaged files are kept except `.cargo_vcs_info.json`, `.cargo-ok`, `Cargo.toml.orig` and `Cargo.lock`. LICENSE-MIT and LICENSE-APACHE are kept. Cargo.toml is the package manifest, unchanged, so the version stays 0.2.4 and the workspace `=0.2.4` pin holds. README.md is replaced with ours: upstream version, every change marked `fastedit patch` in a comment, and when to drop it."
    required: true
    manual: false
  - id: "C2"
    text: "The patch adds `request_raw(method: String, params: serde_json::Value) -\u003e Result\u003cserde_json::Value\u003e` and `notify_raw(method: String, params: serde_json::Value) -\u003e Result\u003c()\u003e` to `ServerSocket` and `ClientSocket` (through `impl_socket_wrapper!`). They build `AnyRequest`/`AnyNotification` and send through the same `MainLoopEvent::OutgoingRequest`/`Outgoing` path as `request::\u003cR\u003e`/`notify::\u003cN\u003e`, so wire ids come from the same counter and fe-lsp id mirroring (docs/decisions/lsp-client.md §2.1) holds. Nothing else in async-lsp changes."
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp sends any method string: `raw_requests!` in crates/fe-lsp/src/custom.rs and `raw_notifications!` in crates/fe-lsp/src/notify.rs are gone, and an unlisted method no longer answers Unavailable. Raw requests are polled once at issue as `send_request` does, and keep their timeouts and `$/cancelRequest` by wire id. A plugin notification keeps its order with the requests and document sync queued before it and rides the QUEUED_MAX window (`Taken`) as `Command::Notify` does. T1701 rules are unchanged: fe-plugin `is_routed` denylist, `lsp:\u003cserver\u003e` grant, (server, root) session choice, pending cap, 16 MiB size limits."
    required: true
    manual: false
  - id: "C4"
    text: "New tests in crates/fe-lsp/src/tests/raw_methods.rs against the fake server (`set_reply`): `a_method_in_no_table_gets_the_fake_servers_reply`, `a_notification_in_no_table_reaches_the_fake_server`, `a_timed_out_raw_request_is_cancelled_by_its_wire_id`."
    required: true
    manual: false
  - id: "C5"
    text: "New tests in crates/fe-workbench/src/plugin_lsp_raw_tests.rs, declared from plugin_lsp.rs: `plugin_lsp_request_in_no_table_gets_the_fake_servers_reply` (a plugin lsp.request with a method no table lists, such as `acme/unlistedMethod`, reaches the fake server session through the real host and returns the fake reply) and `plugin_lsp_notify_in_no_table_reaches_the_fake_server`."
    required: true
    manual: false
  - id: "C6"
    text: "Licensing: THIRD_PARTY.md gets a footnote for the vendored async-lsp in its Dependencies section, and the bundled-assets audit line counts ten vendored crates, not nine. CLEAN_ROOM.md patched table gets an async-lsp row (the heading may become \"Patched crates\"). NOTICE is unchanged because async-lsp ships no NOTICE file, and the decision file says so. `cargo deny check licenses bans sources` passes."
    required: true
    manual: false
---

# Any LSP method from plugins: vendor async-lsp 0.2.4 with a raw request and notify send, and drop fe-lsp's raw_requests!/raw_notifications! allowlists (T1701 follow-up)

# Any LSP method from plugins: vendor async-lsp with a raw send, drop the allowlists

T1701 (B5, plugins stage 3 §3.6, docs/decisions/dqsjfernkt-plugins-stage-3.md) gives plugins `lsp.request`/`lsp.notify`. In async-lsp 0.2.4, `ServerSocket::request::<R>`/`notify::<N>` only send a compile-time `R::METHOD`. `AnyRequest`/`AnyNotification` are `#[non_exhaustive]`, and `PeerSocket::send` (src/lib.rs ~735) is private. So T1701 lands a compile-time `raw_requests!` allowlist in crates/fe-lsp/src/custom.rs, mirroring `raw_notifications!` in crates/fe-lsp/src/notify.rs, and every other method answers Unavailable (T1701 rev 2 C1, a known limit). §3.6 says a plugin sends "a request the language server defines", any method. This card does that.

## The patch

Copy async-lsp 0.2.4 from the crates.io package (`~/.cargo/registry/src/*/async-lsp-0.2.4`) to vendor/async-lsp/. Do it the way vendor/tree-sitter/ and vendor/tree-sitter-yaml/ are done: read their README.md files and the `[patch.crates-io]` comment block in Cargo.toml (~line 330) first. The workspace already `exclude`s vendor/. In src/lib.rs, inside `impl_socket_wrapper!`, add `request_raw` and `notify_raw`. They build an `AnyRequest` (id `RequestId::Number(0)`, which `dispatch_event` replaces, as for typed requests) or an `AnyNotification`, and send through the existing `MainLoopEvent` variants. Decode a reply as `resp.result.unwrap_or_default()`, as `PeerSocketRequestFuture` does. Mark each change `fastedit patch`. The README's "when to drop" is an async-lsp release with a public untyped send. Bump the workspace dependency then and move fe-lsp to it. Update the `async-lsp = { version = "=0.2.4" ... }` comment in Cargo.toml to point at the vendored copy.

## fe-lsp

Replace the macro dispatch in custom.rs and notify.rs with the raw calls (`notify::send` keeps its callers in server.rs). `Command::Notify { method: &'static str }`, `Call.method: &'static str` (requests.rs) and `send_request` (server.rs) may need an owned or `Cow` method for plugin traffic. Keep every ordering rule: one poll at issue for id order, QUEUED_MAX and `Taken`, cancel by wire id. The method gate stays in fe-plugin (`is_routed`: `textDocument/`, `$/`, `initialize`, `exit` and the rest stay denied). fe-lsp does not add another.

## Tests

New files only, as the criteria name them. The fe-lsp fake (crates/fe-lsp/src/tests/fake.rs) answers any method through `unhandled_request` and `set_reply`. Reuse T1701's workbench harness from plugin_lsp_tests.rs (make its helpers `pub(super)` if needed, nothing more). Use a method no table ever listed.

## Licensing

async-lsp is `MIT OR Apache-2.0` and ships no NOTICE file. Keep both licence files in vendor/async-lsp/. THIRD_PARTY.md gets a footnote like ¹⁰ (tree-sitter) under "## Dependencies". CLEAN_ROOM.md "Patched grammars" table, the patched-crate policy, gets a row. No deny.toml change is expected: path crates are outside `[sources]`, and the licence is on the allow list. Check with `cargo deny check licenses bans sources`, not a bare `cargo deny check`, which also fails on advisories the gate skips.

T1701's file names (custom.rs, plugin_lsp.rs, plugin_lsp_tests.rs) were planned, not landed, when this card was filed: re-check them and the scope once T1701 lands.

Write design calls in the decision file (docs/decisions/<lane>-*.md). Do not widen --scope. If a file outside it is needed, stop and say so.
