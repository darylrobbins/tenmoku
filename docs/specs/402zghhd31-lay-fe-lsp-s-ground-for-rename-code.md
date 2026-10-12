---
task_uid: "01m4r18a2k1n1jt4402zghhd31"
task: "T75"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test workspace_edit::tests::capabilities_are_declared: the initialize request the fake server records declares rename with prepareSupport, codeAction with literal support and resolve of edit, formatting, rangeFormatting, signatureHelp with labelOffsetSupport, workspace.applyEdit and workspaceEdit.documentChanges"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test workspace_edit::tests::decodes_changes_and_document_changes: edits to an open UTF-16 document with an emoji decode to the right byte ranges of its version; edits to a closed file come back as Closed and decode_closed places them in its text; both changes and documentChanges forms work"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test workspace_edit::tests::refuses_bad_edits: a TextDocumentEdit at a version the server doesn't have, a create or rename file operation, overlapping edits in one file and a non-file URI each refuse the whole edit"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test workspace_edit::tests::apply_edit_round_trip: a server workspace/applyEdit reaches the sink as LspEvent::ApplyEdit, and answering applied or failed reaches the server as the response; an unanswered reply answers applied false after 5 s on the manual clock"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-lsp and cargo clippy -p fe-lsp --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Lay fe-lsp's ground for rename, code actions, formatting and signature help: client capabilities, decoded workspace edits, workspace/applyEdit as an event, and a stub module per feature (Mac T465)

## Problem

The user (2026-10-12) asked for LSP rename, code actions, formatting and signature help, beyond the plan's M4 list
(plan line 108). fe-lsp can't support any of them yet:

- `client_capabilities` (`crates/fe-lsp/src/server.rs` :1861) declares no `rename`, `codeAction`, `formatting`,
  `rangeFormatting` or `signatureHelp`, and no `workspace.workspaceEdit`, so servers won't offer them.
- Rename and code actions answer with a `WorkspaceEdit` (edits to many files, some not open), and nothing decodes
  one into byte edits.
- Servers apply code-action commands by sending `workspace/applyEdit`, which the router refuses outright
  (`crates/fe-lsp/src/router.rs` :147).
- Each feature would add a module and its exports to `crates/fe-lsp/src/lib.rs` and capabilities to server.rs, so
  four feature cards would all collide on the same lines.

## Change

This card lays the shared ground so the four feature cards that follow touch only their own files.

1. **Capabilities** (server.rs): `textDocument.rename { prepareSupport: true }`; `codeAction` with
   `codeActionLiteralSupport` (every kind in lsp-types 0.95.1's `CodeActionKind` constants), `isPreferredSupport`,
   `resolveSupport { properties: ["edit"] }` and `dataSupport`; `formatting` and `rangeFormatting`; `signatureHelp`
   with `signatureInformation { documentationFormat: [markdown, plaintext], parameterInformation
   { labelOffsetSupport: true }, activeParameterSupport: true }` and `contextSupport: true`; and
   `workspace.workspaceEdit { documentChanges: true, resourceOperations: [] , failureHandling: abort }`,
   `workspace.applyEdit: true`. All with `dynamicRegistration` off, as today.
2. **Workspace edits**: new `pub mod workspace_edit` (`crates/fe-lsp/src/workspace_edit.rs`):
   `pub struct FileEdits { pub path: PathBuf, pub target: EditTarget }` with
   `EditTarget::Open { buffer: BufferId, version: Version, edits: Vec<(Range<usize>, String)> }` (decoded against the
   text the server has for that version, as hover decodes ranges) and
   `EditTarget::Closed { edits: Vec<lsp_types::TextEdit>, encoding: PositionEncoding }` for files not open, with
   `pub fn decode_closed(text: &Text, edits: &[TextEdit], encoding) -> Result<Vec<(Range<usize>, String)>, String>`
   for the host to apply against the text it reads. `LspManager::decode_workspace_edit(server, WorkspaceEdit) ->
   Result<Vec<FileEdits>, LspError>` takes both `changes` and `documentChanges`; a `TextDocumentEdit` whose version
   isn't the one the server has, a resource operation (create, rename, delete file), overlapping edits in one file
   or a URI that isn't a `file:` path refuse the whole edit. Edits in a file come sorted by start.
3. **applyEdit**: the router answers `workspace/applyEdit` by decoding it and sending
   `LspEvent::ApplyEdit { server, label, edits: Vec<FileEdits>, reply: ApplyReply }` to the sink. `ApplyReply`
   (Send, answered once) carries `applied` or `failed(reason)` back as the response; dropped unanswered, or not
   answered within 5 s on the injected clock, it answers `applied: false`. A decode failure answers
   `applied: false` with the reason, with no event.
4. **Module stubs**: `pub mod rename; pub mod code_action; pub mod formatting; pub mod signature_help;` in lib.rs,
   each file holding only its module doc comment, saying which card fills it. The feature cards add `impl LspManager`
   blocks and types there, reached as `fe_lsp::rename::…` (no `pub use` edits), and declare their own test files
   with `#[path]`.

## Tests

New file `crates/fe-lsp/src/workspace_edit_tests.rs`, declared from workspace_edit.rs with `#[path]`, on the fake
server (`fake.rs`: `set_reply`, and `request::<ApplyWorkspaceEdit>` for the server-sent request).

## Implementation notes

- `LspManager::decode_workspace_edit(server, edit)` returns a `Send + 'static` future of
  `Result<Vec<FileEdits>, LspError>`, not the result itself: only the server's actor knows what each open document
  was sent as, so it asks the actor (a new `Command::Documents`, which doesn't drain the buffers first).
- The actor (for `workspace/applyEdit` and `Command::Documents`) decodes against each document as the last note
  handed to async-lsp left it (`server::Handed`), not against its ring: each `didOpen`/`didChange` note carries the
  document snapshot it makes, recorded when the note is handed, and a handed `didClose` drops it (only for the
  incarnation it closes). Notes still waiting in the outbox, ring eviction during a big drain, and a queued close or
  reopen therefore don't change what an edit is read against. Every version a `TextDocumentEdit` names must be the
  handed one, or the whole edit is refused.
- async-lsp's main loop doesn't read the server's input while its own writes are stuck, so the fake can't time an
  applyEdit into the gap where notes wait in the outbox; `apply_edit_reads_what_was_handed` hands `Handed` the notes
  the actor would, with the actor's own snapshots (past the ring's length, and a close then reopen).
- For a reply to a request on a document (rename, a code action), the feature cards should prefer the synchronous
  `Admitted::decode_workspace_edit(edit)`, which decodes against what the request went out with; `Opened` has the
  same method.
- A refused edit is the new `LspError::RefusedEdit(reason)`. Versions are checked for every `TextDocumentEdit`, even
  one with no edits; a range ending before it starts is refused, in `decode_closed` too.
- Several `TextDocumentEdit`s for one file are merged, all read against the same text, and overlaps across them
  refuse the edit. `changes` (a map) comes back in path order, `documentChanges` in the server's order.
- `ApplyReply`'s answer slot is shared with the 5 s timeout: whichever answers first takes it, so `is_answered()` is
  true after a timeout, and an applyEdit that timed out (or whose session ended) before the actor took it never
  reaches the sink.
- A `workspace/applyEdit` with nothing in it is answered `applied: true` at once, with no event.
- Outside the listed scope: `manager.rs` (`send` is now `pub(crate)`), `restart.rs` (`Command::Documents` while no
  session runs answers `ServerDown`), `document.rs` (`didOpen`/`didChange` notes carry their snapshot), and `tests/router.rs`
  (the empty applyEdit's answer is now `applied: true`).
