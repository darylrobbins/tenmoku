---
task_uid: "01m55nkcxrs2vgj886te835wf8"
task: "T190"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::provider_wait::rename_waits_until_capabilities_are_known passes: with initialize delayed and no renameProvider, prepare_rename and rename made during initialization answer Err(Unavailable(\"rename\")) and the fake records no textDocument/prepareRename or textDocument/rename; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::provider_wait::prepare_rename_asks_a_preparing_server_once_known passes: with initialize delayed and prepareProvider true, prepare_rename made during initialization sends one textDocument/prepareRename and answers its scripted range and placeholder; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test tests::provider_wait::related_waits_until_capabilities_are_known passes: with initialize delayed and no declaration, typeDefinition or implementation provider, LspNavigation::related for each kind made during initialization answers Err(NavError::Unsupported) and the fake records none of the three methods; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "rename, prepare_rename and LspNavigation::related decide on a provider only once capabilities are known, through T172's waiting accessor (or a primitive split from it in manager.rs); LspManager::provider_capabilities is removed, having no caller left"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-lsp -p fe-nav passes with no existing test edited (tests/mod.rs only gains the mod line); cargo clippy -p fe-lsp -p fe-nav --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# Send rename, declaration, type definition and implementation requests only once the server's capabilities are known to provide them

## Problem

`LspManager::provider_capabilities` (`crates/fe-lsp/src/manager.rs:1385`, added by T170) answers `Ok(None)` while
the buffer's server is still initializing: its capabilities aren't known yet. Its callers treat `Ok(None)` as "go
ahead", so a request made during initialization is queued and sent once `initialize` is answered, even when the
capabilities the server then reports lack that provider. T172's review (review 6gxmg9, F1 medium, 2026-10-17) found
this in `document_highlights`; the lead's decision was that T172 fixes it for highlights with a manager accessor
that waits until the capabilities are known (in T172's worktree as
`LspManager::document_request_if_provided::<R>(buffer, version, feature, provides, params)`, which polls the
session's capabilities on the manager's clock, answers `Unavailable(feature)` when `provides` is false, and only
then admits the request), and that the other callers get the same fix in this card.

The other callers (every use of `provider_capabilities` in fe-lsp once T171 and T172 land):

- `crates/fe-lsp/src/rename.rs:124`, `LspManager::rename`: a `textDocument/rename` made during initialization is sent
  to a server whose capabilities turn out to have no `renameProvider`.
- `crates/fe-lsp/src/rename.rs:77`, `LspManager::prepare_rename`: during initialization the capabilities are
  `None`, so `!capabilities.is_some_and(prepares)` takes the no-request path. A server with no `renameProvider`
  gets a word target, not `Unavailable("rename")`, and a server whose `prepareProvider` is true is never asked
  `textDocument/prepareRename`.
- `crates/fe-lsp/src/navigation.rs` (T171, `LspNavigation::related`, about lines 204-217):
  `provided.ok_or_else(gone)?.is_err()` lets `Ok(None)` through, so `textDocument/declaration`, `typeDefinition`
  or `implementation` made during initialization is sent to a server without that provider. Through
  `fe_nav::Navigation` the window is narrower (`route` skips a provider that isn't `Readiness::Ready`,
  `crates/fe-nav/src/provider.rs:481`), but `related` is a public trait method and is not guarded itself.

No other fe-lsp code calls `provider_capabilities`. (`formatting.rs:66` reads `capabilities()` directly and treats
unknown as not provided, so it sends nothing: not in this card.)

## Change

1. `crates/fe-lsp/src/rename.rs`:
   - `rename` sends `Rename` through T172's waiting accessor (`document_request_if_provided`, or whatever name it
     lands with) with feature `"rename"` and `renames`, so a server that turns out not to rename gets no request
     and the answer is `Err(LspError::Unavailable("rename"))`.
   - `prepare_rename` decides once the capabilities are known: none with `renames` false is
     `Err(Unavailable("rename"))` with nothing sent; `prepares` true sends `PrepareRenameRequest`; otherwise the
     word fallback as now (an admit with no request). To do that without the manager's borrow inside the future,
     split the accessor in `crates/fe-lsp/src/manager.rs` into a pub(crate) wait-then-admit primitive (it waits for
     the buffer's server's capabilities as T172's loop does, then admits the `Admit` the caller builds from them,
     or answers the caller's `Err`) and the request built on it, which T172's `document_highlights` keeps using
     unchanged.
2. `crates/fe-lsp/src/navigation.rs`: `LspNavigation::related` sends `GotoDeclaration`, `GotoTypeDefinition` or
   `GotoImplementation` through the waiting accessor with the kind's feature name and provider check (the three
   closures there capture nothing, so they become `fn`s), and maps that `Unavailable(<feature>)` to
   `NavError::Unsupported` as now. `goto` takes the request's future from its caller (or a flag) so `definitions`
   still uses plain `document_request`.
3. Remove `LspManager::provider_capabilities` from `manager.rs`: no caller is left, and clippy's dead-code lint
   would fail on it.

New test file `crates/fe-lsp/src/tests/provider_wait.rs`, declared by `mod provider_wait;` in
`crates/fe-lsp/src/tests/mod.rs`, using `FakeServer::with_clock` and `Script { initialize_delay, .. }` with
`ManualClock`s as `tests/lifecycle.rs` does (`a_stop_while_starting_kills_it_ahead_of_queued_requests`). Each
request is made while `initialize` is held, then the fake's clock is advanced past the delay and the manager's
clock in steps (well under the method's timeout) until the answer comes. Script a `null` reply for each method
the test forbids, so at the base, where the request does go out, the call returns and the test fails on an
assertion rather than hanging. Only `assert_eq!` / `assert!` without a custom message: no `unwrap()` or
`expect()` on the answers. Write the tests against APIs that exist at the base (main with T171 and T172):
`LspManager::rename`, `prepare_rename`, `LspNavigation::related`, the fake and the clocks, not the new primitive.

- `rename_waits_until_capabilities_are_known`: capabilities without `renameProvider`; `prepare_rename` and
  `rename` called during initialization both answer `Err(LspError::Unavailable("rename".into()))`, and the fake
  records no `textDocument/prepareRename` or `textDocument/rename`.
- `prepare_rename_asks_a_preparing_server_once_known`: `renameProvider: { prepareProvider: true }`, and
  `textDocument/prepareRename` scripted to answer a range with a placeholder that differs from the word at the
  offset; `prepare_rename` called during initialization answers that range and placeholder, and the fake records
  one `textDocument/prepareRename`.
- `related_waits_until_capabilities_are_known`: capabilities without declaration, type definition or
  implementation providers; `LspNavigation::related` for each `Related` kind, called during initialization on a
  thread of its own (it blocks), answers `Err(NavError::Unsupported)`, and the fake records none of
  `textDocument/declaration`, `textDocument/typeDefinition`, `textDocument/implementation`.
