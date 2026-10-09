---
task_uid: "01m49phfqhjsw50xg4cahz237c"
task: "T145"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::diagnostics::versioned_publications_decode_against_the_ring: a publication for a version two edits old yields LspEvent::Diagnostics with that buffer version and byte ranges selecting the intended words in that version's text, with an emoji earlier on the line, under utf-8 and utf-16 sessions; severity, message, source, code and the Unnecessary and Deprecated tags carry over; initialize advertises publishDiagnostics versionSupport, relatedInformation, tagSupport and codeDescriptionSupport"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::diagnostics::dropped_publications: out-of-order versioned publications older than the newest published are dropped; a version not in the ring is dropped; an unversioned publication from a Drop spec is dropped before and after the first edit; an unversioned publication the fake computed on the old incarnation's text and sent after the new didOpen and before any didChange is dropped; after close and reopen, a publication carrying the old incarnation's version is dropped and versions don't collide; a publication for a URI with no current incarnation is ignored"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::diagnostics::cleared_after_retirement: DiagnosticsCleared with the old (session, incarnation) is emitted after didClose and after the fake crashes, and at that point active_identity no longer returns that identity"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test tests::diagnostics::anchored_ranges_rebase: a DiagnosticSet applied with version V after two later edits holds ranges rebased by map_offset onto the current text; ranges move with further edits; an apply whose version is below the edit-log floor changes nothing; sets for two servers on one buffer are independent"
    required: true
    manual: false
  - id: "C5"
    text: "fe-editor test tests::diagnostics::identity_checks: apply with an identity other than the active one is rejected; clear(identity) empties the set only when the set came from that identity and records it as cleared; a later apply with a cleared identity is rejected and the set stays empty"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::lsp_diagnostics::stale_events_are_rejected: the fake's publication for the open file shows in the editor's set; a Diagnostics for the old identity still queued when the buffer is closed and reopened, and in a second case when the session restarts, is drained before any new-identity event and rejected; with the actor's open paused, the buffer closed and the open resumed, a publication for that URI is rejected; a Diagnostics delivered after its DiagnosticsCleared is rejected"
    required: true
    manual: false
  - id: "C7"
    text: "fe-gpui test view::diagnostics_tests::squiggles_gutter_and_hover: each diagnostic range paints a wavy underline in its severity's colour, rows with diagnostics show one gutter mark with the highest severity, and hovering a squiggle for the hover delay shows its message as plain text in the hover popover; with no diagnostics the paint is unchanged and the existing golden screenshots pass"
    required: true
    manual: false
  - id: "C8"
    text: "THIRD_PARTY.md has a row for crates/fe-editor/src/diagnostics.rs citing gpui-kit editor/diagnostics.rs at 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, Apache-2.0; fe-editor has no fe-lsp dependency; no Cargo.toml or Cargo.lock change; scripts/check-no-gpui.sh prints ok: fe-editor and ok: fe-lsp"
    required: true
    manual: false
---

# Show language server diagnostics: versioned decoding, an anchored DiagnosticSet, squiggles, gutter marks and hover messages

# Show language server diagnostics: versioned decoding, an anchored DiagnosticSet, squiggles, gutter marks and hover messages

Card 7 of `docs/decisions/lsp-client.md` §8 (GPUI for rendering). The design is:

- §4.1 "Diagnostics", "Delivery to the UI" and "The active identity", with its fake-server test list
- §4.3 (gpui-kit's wrong helpers)
- §7's Diagnostics row

It builds on card 2b's version ring and identity table, card 4's session ends, and card 5's
workbench events.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q3 (a):** decoding runs on the server thread, and the UI gets byte ranges through card 5's
  channel.

## Change

### `crates/fe-lsp/src/diagnostics.rs` (new)

- **Decoding `textDocument/publishDiagnostics`.** For the URI's current incarnation (§4.1
  "Diagnostics" 1–5):
  - a versioned *v* in the ring is decoded against the ring's `Text` for *v*, in the session's
    encoding
  - out of order (older than the newest published), not in the ring, unversioned (the spec's
    `UnversionedDiagnostics::Drop`), or no current incarnation: dropped
- **The item.** `fe_lsp::Diagnostic { range: Range<usize>, severity, message, source, code, tags }`.
- **Events.** It emits `LspEvent::Diagnostics { buffer, session, incarnation, version, server,
  items }`. It emits `LspEvent::DiagnosticsCleared { buffer, session, incarnation, server }` after
  `didClose`, and after a session ends, each once the identity is retired.
- **Capabilities.** Advertise `publishDiagnostics { versionSupport, relatedInformation,
  tagSupport [Unnecessary, Deprecated], codeDescriptionSupport }`.

### `crates/fe-editor/src/diagnostics.rs` (new)

- **`DiagnosticSet`.** Copied from gpui-kit `editor/diagnostics.rs` (the plan's reuse map; add a
  `THIRD_PARTY.md` row at commit `2c5162f8`). It holds `fe-buffer` anchors over byte ranges, never
  `lsp_types::Position`, keyed by server. Drop gpui-kit's char-column `position_to_offset` and its
  first-to-last `apply_lsp_edits` (§4.3).
- **No `fe-lsp` dependency.** fe-editor defines its own small item type, and fe-workbench converts
  to it.
- **`apply(identity, version, items, active)`:**
  - rejected unless `identity` equals the active identity and isn't recorded as cleared
  - ranges are rebased with `BufferSnapshot::map_offset` from `version`
  - a version below the edit-log floor is dropped
- **`clear(identity)`:** empties the set only if it carries that identity, and records the identity
  as cleared.

### `fe-gpui`

- **`src/view.rs` / `src/element.rs`:**
  - wavy underlines under each range, coloured by severity
  - a gutter mark on rows that have diagnostics, showing the highest severity
- **`src/hover.rs` (new): the hover popover surface.** Hovering a squiggle for the hover delay
  shows its message as plain text. Card 8 adds LSP hover content to this same surface.
- **`src/lib.rs`:** the `mod` line.

### `fe-workbench`

**`src/lsp.rs`:** applies `Diagnostics` and `DiagnosticsCleared` from the channel to each open
editor's `DiagnosticSet`, checking `LspManager::active_identity(buffer, server)` at delivery.

## Out of scope

- Pull diagnostics.
- Diagnostics for unopened files (a problems panel).
- Code actions.
- Checking which real servers version their diagnostics: card 10 runs that check, and records any
  server-specific guarantee (§10).

## Decided (lead defaults; the user may revisit)

- `publishDiagnostics` decoding lives in this card, not card 2a or 2b, to keep them under the cap. Card 2b
  supplies the ring and the identity table it reads.
- The real-server check of which servers set `version` moves to card 10, which has the server
  harness.
