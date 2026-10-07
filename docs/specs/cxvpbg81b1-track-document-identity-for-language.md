---
task_uid: "01m49phdfrahrcg6cxvpbg81b1"
task: "T140"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::identity::versions_and_incarnations: a reopen's didOpen uses that URI's last version plus 1; session and incarnation ids are never reused; LspManager::active_identity(buffer, server) is set before the fake sees didOpen and is None as soon as LspManager::close returns, before didClose is sent; ending a session retires that session's identities and no others"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::identity::stale_opens_and_closes: with the actor's open paused before activation and the buffer then closed, no identity is inserted and no didOpen is sent; with the buffer closed and reopened before the paused open resumes, only the new open's identity becomes active; with a size-limit close paused before retiring and the buffer closed and reopened meanwhile, the new open's generation and entry are untouched and it stays active"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::admission::request_admission_by_version: a request naming the sent version drains first, is sent, and holds that version's ring entry and the current entries of the other open documents; a request naming an older version answers Unavailable(\"buffer changed\") without reaching the fake; after 40 edits the ring holds exactly the last 32 versions"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::admission::large_documents_are_not_synced: with max_file_bytes set small, a larger document is never sent didOpen; a synced one that grows past it is sent didClose and not re-opened until it is back under; a request on either answers Unavailable(\"too large for the language server\"); the default is 52428800 bytes"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check-no-gpui.sh prints ok: fe-lsp; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Track document identity for language servers: sessions, incarnations, a version ring, request admission and the size limit

# Track document identity for language servers: sessions, incarnations, a version ring, request admission and the size limit

Card 2b of `docs/decisions/lsp-client.md` §8: the second half of its card 2. It follows card 2a, the
wire sync.

The design:

- §4.1 **Identity: sessions, incarnations and versions**
- §4.1 **The active identity**: the lifecycle half (the UI's checks are card 7)
- §4.1 **Request admission**
- §4.1 **Large files**
- the stale-activation and stale size-limit-close cases from §4.1's fake-server test list

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q3 (a):** all of it runs on the server's actor. No tokio.

## Change

- **`src/identity.rs` (new): the active-identity table.**
  - **Ids.** Session and incarnation ids are `u64`s from the manager, never reused.
  - **Open generation.** A per-buffer `u64` that `LspManager::open` and `LspManager::close` bump on
    the calling thread, under the table's mutex.
  - **Reading it.** `LspManager::active_identity(buffer, server) -> Option<(SessionId,
    IncarnationId)>`.
  - **Activation.** The actor's open activates only if the buffer is still open under the generation
    the open carries, and it does so before sending `didOpen`. A stale open inserts nothing and
    sends nothing.
  - **Retirement.**
    - `close` retires the buffer's identities before `didClose` is sent
    - the actor's own size-limit close retires only its own identity, and only if the generation
      and entry still match
    - a session's end retires that session's identities. Card 4's restarts call it.
- **`src/document.rs`.**
  - **The ring.** Keep the last 32 versions sent. Each entry holds the LSP version, the
    `fe_buffer::Version`, the `Text` (a rope clone) and the `edits`.
  - **Versions.** The LSP version is monotonic across reopens: a reopen sends the URI's last version
    + 1.
  - **Large files.** `max_file_bytes` defaults to 50 MB.
    - a document over it is never opened
    - one that grows past it through edits is closed, and isn't re-opened until it is back under
- **Request admission** (`src/manager.rs`, `src/server.rs`). A request names its buffer version *V*.
  The actor drains the document first. With *S* the sent version:
  - *S = V*: the request goes out, holding *V*'s ring entry and the current entries of the other open
    documents
  - *S > V*: it answers `Unavailable("buffer changed")`
  - an oversized document answers `Unavailable("too large for the language server")`

  Cards 6, 8 and 9 send their requests through this.

## Size

About 450–550 non-test lines estimated, within the 1,000 cap.

## Out of scope

- Decoding diagnostics against the ring, and the `Diagnostics` / `DiagnosticsCleared` events (card 7).
- Restarts (card 4).
- Navigation requests themselves (card 6).
