---
task_uid: "01m49phd0gvktxfcsgzjk5wcs9"
task: "T139"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::encoding::emoji_cases_round_trip: for cases 1-10 of docs/decisions/lsp-client.md §4.4, under utf-8 and utf-16, to_lsp(text, offset, enc) equals the table's position and from_lsp(text, position, enc) equals the offset; each case's row end round-trips; case 10 loads its CRLF bytes through Text::from_file_contents"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::encoding::server_positions_clamp: §4.4 cases 11-13 through from_lsp: 0:99 gives 5, 9:0 gives 7, utf-16 0:2 gives 1 and utf-8 0:3 gives 1"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::sync::round_trip_utf8 and tests::sync::round_trip_utf16: a proptest at a small case count (the test stage stays fast) applies random batches of insertions and deletions of the §4.4 strings, including multi-edit batches and edits that split and join rows, to a buffer synced to the fake negotiating that encoding; after every didChange the fake's replica, built with its own reference converter, equals the buffer's text; scripts/soak.sh runs these two with more cases on a separate fe-lsp line that runs no real_server_ test"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::sync::open_and_close: LspManager::open makes the fake receive didOpen with the §3.2 languageId, the LF text (a CRLF file's too) and version 0; two documents each sync from their own subscription; LspManager::close makes it receive didClose, and later edits to that buffer send nothing"
    required: true
    manual: false
  - id: "C5"
    text: "fe-lsp test tests::sync::save_barrier_orders_changes_and_saves: one drain of Edited(V1), Saved(V1), Edited(V2), Saved(V2), Edited(V3) reaches the fake as change to V1, save V1, change to V2, save V2, change to V3; two consecutive Edited batches become one didChange; with save.includeText the didSave text is the saved version's text, not a later one"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test tests::sync::backstop_tick_delivers_without_a_nudge: after an edit is flushed, a save made with LspManager::changed not called reaches the fake as didSave once the injected clock passes 250 ms and not before; with changed called it arrives without advancing the clock"
    required: true
    manual: false
  - id: "C7"
    text: "fe-lsp test tests::sync::full_text_fallback: the fake receives one change with no range holding the whole text when it advertises Full sync, when a drain holds 65 edits (64 stay incremental), and when the batches' new text exceeds half the document"
    required: true
    manual: false
  - id: "C8"
    text: "scripts/check-no-gpui.sh prints ok: fe-lsp; no Cargo.toml or Cargo.lock change; crates/fe-text is unchanged"
    required: true
    manual: false
---

# Sync documents to language servers: position encodings and incremental didOpen, didChange, didSave and didClose

# Sync documents to language servers: position encodings and incremental didOpen, didChange, didSave and didClose

Card 2a of `docs/decisions/lsp-client.md` §8: its card 2, split in two. This half is the wire
sync. Card 2b adds the identity table, the version ring, request admission and the 50 MB limit.

The design:

- §4.2, §4.3 and §4.4
- §4.1's **What the buffer gives us**, **The conversion**, **Who drains**, **Ordering and
  batching**, **Saving**, **Full-text fallback** and **Line endings**
- §6.1's document replica

It builds on card 1's crate and module layout.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q3 (a):** draining runs on the server's own thread and actor. No tokio.

## Change

- **`src/encoding.rs` (new).**
  - `PositionEncoding { Utf8, Utf16 }`.
  - `to_lsp(text, offset, enc)` and `from_lsp(text, pos, enc)`, over the existing `fe_text::Text`
    APIs (§4.3).
  - No utf-32 path: it isn't offered (§4.2).
  - Conversions never scan a row.
- **`src/document.rs` (new).** Per open document:
  - **`sent: Text`.** Each `BufferEvent::Edited` batch becomes `TextDocumentContentChangeEvent`s,
    walking `edits` in reverse against `sent` (§4.1 "The conversion").
  - **Draining.** The actor drains the document's own `Subscription` on `LspManager::changed(buffer)`
    and on a 250 ms backstop tick, through card 1's `Clock`.
  - **Coalescing.** Consecutive `Edited` batches coalesce into one `didChange`.
  - **`Saved` is a barrier.** Flush, then `didSave`, then a new `didChange`. With
    `save.includeText`, `didSave` carries the text as sent up to the saved version.
  - **Full-text fallback.** Send one change with no range, holding the whole text, when:
    - the server's sync kind is `Full`
    - a drain holds more than 64 edits
    - the batches' new text exceeds half the document
  - **Versions.** The LSP version starts at 0 on `didOpen` and rises by one per `didChange`. Card 2b
    makes it monotonic across reopens.
- **`LspManager` API.** It gains `open(buffer_id, text, version, subscription, path, language)`,
  `close(buffer_id)` and `changed(buffer_id)`.
  - `didOpen` carries the §3.2 `languageId` and the LF text.
  - `didClose` is sent on `close`.
  - The server for a document is given explicitly. Card 3 adds choosing it by language and
    starting it lazily.
- **The fake's document replica** (§6.1 and §4.4). It applies `didOpen`, `didChange`, `didSave` and
  `didClose` with a reference converter written independently over `str::encode_utf16` and
  `char_indices`, never `fe-lsp`'s.
- **`scripts/soak.sh`.** It gains a separate `fe-lsp` line, filtered to this card's proptests, so it
  doesn't pick up the `#[ignore]`d `real_server_*` tests card 10 adds (§4.4).

## Size

About 550–650 non-test lines estimated, within the 1,000 cap.

## Out of scope

- **Card 2b:**
  - sessions, incarnations and the active-identity table
  - the 32-version ring
  - draining before a request, and request admission by version
  - monotonic versions across reopens
  - the 50 MB limit
- `publishDiagnostics` (card 7).
- Restart re-opens (card 4).
- Lazy start (card 3).
- UI nudging (card 5).

## Implementation notes

- **C3's property test** uses a seeded SplitMix64 generator in `tests/sync.rs`, not the
  `proptest` crate: fe-lsp doesn't depend on proptest, and C8 rules out a Cargo.toml or
  Cargo.lock change. `PROPTEST_CASES` sets the case count (24 by default), and a failure
  prints the seed and every edit.
- **The full-text thresholds** (more than 64 edits, new text over half the document) are
  counted over the whole drain. A drain split by a save sends every one of its `didChange`s
  whole.
- **`LspManager::open`** takes the server first (`open(server, buffer_id, …)`), since the
  server is given explicitly. `close` detaches the subscription on the calling thread. Events
  already queued stay with the actor's drains, in order with earlier nudges and requests, and
  go out before `didClose`. Edits made after `close` never go out. When the actor lets go of a
  document (close, crash, failed start), the subscription goes with it.
- **Backstop ticks** only mark a drain as due. The drain runs once async-lsp's queue has room
  and the previous drain's notifications have gone, so a server that doesn't read can't grow
  the outbox.
- **`textDocumentSync`** is read as LSP 3.17 says: absent means `None`. If `openClose` is
  off, nothing is sent for a document. A bare kind means open, close, that kind, and
  `didSave` without the text.
