---
task_uid: "01m4nxsn75mnysk9f8nrg8x7bs"
task: "T39"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::pull_diagnostics::pulls_after_open_change_and_save: initialize offers textDocument.diagnostic and workspace.diagnostics.refreshSupport; with a fake server advertising diagnosticProvider, textDocument/diagnostic is sent after didOpen, each flushed didChange and didSave, at most one in flight per document (a change during one is pulled once after its reply); a full report reaches the sink as LspEvent::Diagnostics at the buffer version of the last didOpen/didChange sent before the request, decoded in the ring text; a server without diagnosticProvider gets no pull"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::pull_diagnostics::pushed_and_pulled_merge: a push for one version and a pull reply two edits later arrive as one LspEvent::Diagnostics at the newer version holding both, the older list mapped through the ring edits; a later push keeps the pulled list; a pull reply older than one applied, one whose version left the ring, and one for a closed, reopened or restarted incarnation are dropped"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::pull_diagnostics::refresh_ready_and_errors: workspace/diagnostic/refresh is answered and re-pulls every open document of that session; an error reply or an unchanged report is dropped, and the document is pulled again when the server turns Ready and at the next change, save or refresh"
    required: true
    manual: false
  - id: "C4"
    text: "FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh passes on Linux: real_server_rust_analyzer sees an unresolved import in an unsaved buffer as LspEvent::Diagnostics at the edited version, real_server_typescript_go sees the type error in the unsaved file the same way, and versions_diagnostics matches what the servers now do; the output is in the decision record and lsp-client.md §7 and §10 are updated"
    required: true
    manual: false
  - id: "C5"
    text: "No file outside the scope changes, apart from the task's own spec under docs/specs/ and its decision record; Cargo.toml and Cargo.lock are unchanged; every earlier fe-lsp, fe-editor and fe-workbench test passes with its files unchanged"
    required: true
    manual: false
---

# Pull diagnostics from servers that advertise diagnosticProvider, so rust-analyzer's own and typescript-go's show in unsaved files (Mac T306)

# fe-lsp: pull diagnostics (`textDocument/diagnostic`) for servers that advertise `diagnosticProvider`

## Problem

Found by T148's real-server tests (2026-10-09, `docs/decisions/lsp-client.md` §10, "Which servers
version their diagnostics"). fe-lsp only takes pushed `textDocument/publishDiagnostics`; pull
diagnostics were out of M4 (§7).

- **rust-analyzer** 1.98.1 advertises `diagnosticProvider` whatever the client offers and sends
  `workspace/diagnostic/refresh`. It pushes only `cargo check`'s diagnostics (source `rustc`); its
  own (an unresolved import in an unsaved buffer) are pull-only, so Rust files show diagnostics only
  after a save.
- **typescript-go** 7.0.2 pushes none for source files (one empty list for `tsconfig.json`), so
  TypeScript files show none under it. It answers pull requests (`real_server.rs`,
  `versions_diagnostics`).

Today `router.rs` answers `WorkspaceDiagnosticRefresh` with `Ok(())` and does nothing else;
`client_capabilities()` (`server.rs`) offers no `textDocument.diagnostic`.

## Change

Pull for open documents only. Workspace diagnostics (`workspace/diagnostic`), `relatedDocuments`,
`previousResultId` and dynamic registration stay out. No new dependency (`lsp-types =0.95.1` has the
types). No UI change: pulled diagnostics reach the UI as today's `LspEvent::Diagnostics`.

### Capabilities

`client_capabilities()` adds `textDocument.diagnostic { dynamicRegistration: false,
relatedDocumentSupport: false }` and `workspace.diagnostics { refreshSupport: true }`. A server pulls
when its `InitializeResult` has `diagnosticProvider` (options or registration options); otherwise
nothing changes for it.

### When fe-lsp pulls (new `crates/fe-lsp/src/pull.rs`)

A document of a pulling server is pulled after its `didOpen`, after each flushed `didChange` batch,
after `didSave`, when the server's status turns `Ready`, and on `workspace/diagnostic/refresh` (every
open document of that session; the request is still answered `Ok`). At most one pull per document
is in flight: a trigger while one is in flight marks the document dirty, and it's pulled once more
when the reply lands. A pull sends `textDocument/diagnostic { textDocument, identifier }`
(`identifier` from the server's `DiagnosticOptions`, if any), with no `previousResultId`.

### Which version a reply belongs to

The **LSP version of the last `didOpen`/`didChange` sent for the document before the request**: the
server takes notifications and requests in order. The reply is decoded as a versioned publication
for that version (§4.1, `document.rs` `publish` and `diagnostics.rs` `decode`): against the ring's
text, dropped if that version has left the ring, if a newer pull reply was already applied, or if
the incarnation or session it was sent for is no longer current (closed, reopened, restarted). A
`full` report gives the items; an `unchanged` report (fe-lsp never sends a result id) and an error
reply (including `ServerCancelled`, `ContentModified`, a timeout) are dropped, leaving the next
trigger to pull again.

### Pushed and pulled together

A server may do both (rust-analyzer: `rustc` pushed, its own pulled). Each document keeps the latest
accepted pushed list and pulled list, each with its LSP version, each with its own
older-than-newest rule. Whenever either changes, the document emits **one** `LspEvent::Diagnostics`
for (buffer, server) at the newer of the two versions, holding both lists. The older list's ranges
are mapped forward through the ring's `edits` from its version to the newer one (start biased right,
end biased left, as `DiagnosticSet::apply` maps them). If its version has left the ring, it is
omitted until it is replaced. `DiagnosticsCleared` and session end clear both lists.

### Real servers

`crates/fe-lsp/tests/real_server.rs`:

- `real_server_rust_analyzer*` checks an unresolved import in an **unsaved** buffer reaching the sink
  as `LspEvent::Diagnostics` at the edited version.
- `real_server_typescript_go` checks the type error in the unsaved file the same way.
- `versions_diagnostics` and `check_diagnostics_version` follow what the servers now do. pyright may
  switch to pull once the client offers it.
- Record any server whose pushed and pulled lists duplicate each other.

### Docs

Update `docs/decisions/lsp-client.md`: §7 (pull diagnostics move into scope, with this design) and
§10 (the rust-analyzer and typescript-go findings, re-checked). Put the run's evidence in this
task's decision record.

## Tests (new file)

`crates/fe-lsp/src/tests/pull_diagnostics.rs`, declared in `crates/fe-lsp/src/tests/mod.rs`, on the
fake server (`crate::fake`: `InitializeReply` with `diagnostic_provider`, `set_reply` for
`textDocument/diagnostic` with delays, `request::<WorkspaceDiagnosticRefresh>` from the server).

## Out of scope

typescript-language-server (T305, which builds on this), workspace and related-document diagnostics,
result ids, any UI change.

## Implementation notes

- lsp-types 0.95.1 serializes the workspace capability as `workspace.diagnostic`, not the protocol's
  `workspace.diagnostics`. `initialize` now goes out as JSON (`server.rs`, `initialize_json`) with
  `workspace.diagnostics { refreshSupport: true }` added; the typed field is left unset.
- The triggers of one drain (a change and a save, say) make one pull, queued behind all of its
  notifications, rather than one per notification.
- A pull is cancelled after 30 s (`pull::PULL_TIMEOUT`), which counts as an error reply: dropped,
  and the next trigger pulls again. Without it, a server that never answers one pull would never be
  pulled for that document again.
- `SentVersion` has a crate-private `edits_complete`, false when batches went missing before it (a
  gap, `document.rs`): the older list isn't mapped across one.
- The "older than one applied" rule can't be reached on the wire with one pull in flight per
  document, so `pushed_and_pulled_merge` checks it on `pull::Lists` directly.
- The fake now fixes a request's scripted reply before recording the request, so a test that saw a
  request recorded can change the script for the next one without racing it.
- rust-analyzer 1.98.1's unresolved-import diagnostic is experimental, off by default. The default
  check (`real_server_rust_analyzer_utf8`) uses a type mismatch for the unsaved error and asserts the
  import isn't reported; `real_server_rust_analyzer_unresolved_import_unsaved` turns experimental
  diagnostics on (`initializationOptions`) and sees the unresolved import arrive unsaved by pull.
- A pull reply is also dropped, and nothing more pulled, when the incarnation is no longer the
  manager's active identity: the manager retires it on close before the actor takes the close.
