---
task_uid: "01m71sz8vq06yz0vnekhfnryp6"
task: "T1701"
rev: 5
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A plugin extension's `[contributes.lsp]` is accepted under stage 1's consent and the `lsp:\u003cserver\u003e` grant; `lsp.request` and `lsp.notify` go to the session a buffer or a root names, for the extension's own or a built-in server. Known limit (scope add approved by lead core for the C1 session bridge): async-lsp sends only compile-time method names, so fe-lsp sends the methods in its `raw_requests!` allowlist (rust-analyzer's `experimental/*` and `rust-analyzer/*` and the fake server's test method); any other method answers Unavailable. Arbitrary methods need an async-lsp patch, a follow-up card."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in crates/fe-plugin/tests/lsp.rs: `lsp_custom_method_reaches_fake_server`, `lsp_two_roots_same_server_reach_the_right_session`, `lsp_routed_text_document_method_is_denied`, `lsp_another_extensions_server_is_denied`, `lsp_builtin_server_reachable_with_its_grant`; and a workbench test that a plugin's lsp.request reaches a fake server's session through the real host."
    required: true
    manual: false
  - id: "C3"
    text: "Server requests are bounded: pending requests per plugin capped by the host; a request is dropped when the plugin unloads."
    required: true
    manual: false
  - id: "C4"
    text: "Q3 (downloaded server binaries) follows the design's current text: not supported in this card."
    required: true
    manual: false
---

# Language servers from plugins: [contributes.lsp] and lsp.request/lsp.notify (B5, 3.6)

# Language servers from plugins: [contributes.lsp] and lsp.request/lsp.notify (B5, 3.6)

Draft id B5 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 3.6. Root context rule: the session is selected by (server, root) from the buffer or root named, never by name alone.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- fe-plugin's `lsp` slice (`LspSessions` trait, `LspRequests` registry) does the grant, own-or-built-in server, routed-method, size and pending-cap checks (4 per plugin); unloading cancels waiting requests (`Unloader`, called from `HostedPlugin::shutdown`).
- The host's `LspSessions` is `plugin_lsp.rs`: the main thread finds the window manager (a buffer's placed server, or a trusted root's window) and hands the plugin's thread a future, which it waits on until its call deadline or unload; dropping it cancels the request.
- Known limit (C1): async-lsp types methods at compile time, so `crates/fe-lsp/src/custom.rs` sends only the methods in its `raw_requests!` / `raw_notices!` allowlists (rust-analyzer `experimental/*` and `rust-analyzer/*`, plus `fastedit-test/*`); any other answers `Unavailable`, reaching the plugin as `failed`. These methods have no `lsp.timeouts` entry, so the plugin's call deadline is the bound. Arbitrary methods need an async-lsp patch: a follow-up card.
- `crates/fe-workbench/src/lib.rs` is held by another claim, so `plugin_lsp` is declared from `plugin_bridge.rs` with `#[path]`.
- Q3 (downloaded server binaries): not supported here.
- Review round 1: `withdraw` now cancels the plugin's waiting requests before shutdown (the plugin mutex is held by the waiting call); only the first extension in order that contributes a server name owns it (`owned_servers`), as fe-lsp runs the first; `unload_load` compares and removes under one lock.
- Review round 2: a non-built-in server is authorized on the main thread at each call against the granted, running contribution of the plugin's own extension (`granted_extension_servers`, so consent changes while loaded count; `runs_for`); a cancelled plugin's call stops waiting at the handoff and is checked again before it is sent; the unload test now loads a WAT plugin through the host, waits for the fake server to record the request, and unloads through `unload_plugin`.
- Review round 3: ownership is read from `lsp_consent::running_extension_servers` (the servers handed to the managers), so a server whose changed invocation awaits consent, whose granted invocation still runs, stays the extension's own, and a shadowed one doesn't.
- Review round 4: custom notifications go through the actor's `Command::Notify` (`notify::send` falls back to `custom::send_notice`), so they consume no request id and a later cancel names the request's wire id; `notify.rs` was added to the scope. The consented non-built-in integration test is `a_plugin_reaches_its_extensions_consented_server_through_a_pending_change` in `plugin_lsp_tests.rs`.
- Review round 5 (F7): the load's cancel flag reaches every call (`Stop` = the request's flag or the load's), checked before queueing, before sending and while waiting, so unloading a plugin still in `init` drops its waiting request and an unsent notification. The GPUI test scheduler runs a load on the test thread, so a plugin `init` that waits on a main-thread job deadlocks it there; the tests cancel the load flag on a plugin thread instead.
- Review round 5 (F8): `LspManager::try_send` reports a missing or closed command channel, so `custom_notify_*` answers `ServerDown` (`not-found` to the plugin) for a stopped session whose handle the manager still holds; `manager.rs` was added to the scope.
