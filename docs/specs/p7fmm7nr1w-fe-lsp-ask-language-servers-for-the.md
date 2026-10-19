---
task_uid: "01m55v37xnwntp60p7fmm7nr1w"
task: "T170"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::hover::inlay_hints::hints_decode_in_bytes_of_the_version passes for UTF-8 and UTF-16: a type hint and a two-part parameter hint decode to their byte offsets, labels, kinds and padding, the request's range being the asked one encoded; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit tests tests::hover::inlay_hints::no_provider_sends_nothing and tests::hover::inlay_hints::resolve_fills_tooltip_and_edits pass: no inlayHintProvider is Ok(None) with no request; resolving sends data back and fills tooltip and edits in bytes of the version; past the resolve timeout the hint is as it was and $/cancelRequest is sent"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test tests::hover::inlay_hints::inlay_hint_capabilities_are_advertised passes: textDocument.inlayHint {dynamicRegistration: false, resolveSupport: {properties: [tooltip, textEdits, label.tooltip]}} and workspace.inlayHint {refreshSupport: true}"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: ask language servers for the inlay hints in a range and resolve a hint's tooltip and edits, in bytes of the buffer's version (Mac T933)

Filed on argon from Mac T933 (card ~/coordination/argon-cards/zzr/T933.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txs3a28jc7gdkvq72jh3w. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T850, Mac T918 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T933)".

## Problem

LSP completeness track, item 3b (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
"Inlay hints ✗"). fe-lsp neither advertises `textDocument.inlayHint` (`client_capabilities`,
`crates/fe-lsp/src/server.rs:1999`) nor sends `textDocument/inlayHint` or `inlayHint/resolve`. VS Code asks for
the hints of the rows in view, and resolves a hint for its tooltip and edits when the user hovers or
double-clicks it.

This card is the fe-lsp request and resolve, built on T850's `Admitted::position`/`offsets` and
`LspManager::provider_capabilities`, as T853 (document highlights) is. Showing them is the workbench card.

## Change

1. New `crates/fe-lsp/src/inlay_hint.rs`, modelled on `hover.rs` and T853's `document_highlight.rs`:
   - `#[non_exhaustive] pub struct InlayHint { pub offset: usize, pub label: String, pub kind:
     Option<InlayHintKind>, pub padding_left: bool, pub padding_right: bool, pub tooltip: Option<String>,
     pub edits: Vec<(Range<usize>, String)> }`, plus private `raw`, `server` and `from`
     (buffer, session and incarnation) as `CompletionItem` keeps them; `label` joins a label-part list's
     values; `offset` and `edits` are bytes of the requested version.
   - `LspManager::inlay_hints(buffer, version, range: Range<usize>) -> impl Future<Output =
     Result<Option<Vec<InlayHint>>, LspError>>`: `Ok(None)` and nothing sent if the server has no
     `inlayHintProvider` (through `provider_capabilities`), `Ok(Some(vec![]))` for a null reply or a timeout
     (which cancels), hints sorted by offset; a hint whose position is outside `range` is dropped.
   - `LspManager::resolve_inlay_hint(hint) -> impl Future<Output = Result<InlayHint, LspError>>`: as
     completion's `resolve`: only to a server whose provider has `resolveProvider`, only while the session
     that gave it has that opening; fills `tooltip` and `edits`, the edits decoded against the request
     version's text, which the hint keeps privately (as T913 has completion items keep theirs); the hint as it
     was on a timeout.
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `textDocument.inlayHint = {dynamicRegistration:
   false, resolveSupport: {properties: ["tooltip", "textEdits", "label.tooltip"]}}` and
   `workspace.inlayHint = {refreshSupport: true}` (the refresh request is reported as `LspEvent::Refresh` by
   the refresh-events card).
3. `crates/fe-lsp/src/requests.rs` `timeout`: `textDocument/inlayHint` → `timeouts.hover`, `inlayHint/resolve`
   → `timeouts.resolve`.
4. `crates/fe-lsp/src/lib.rs`: `mod inlay_hint`, export `InlayHint` and `InlayHintKind` (fe-lsp's own, Type and
   Parameter, so fe-workbench needs no lsp_types).
5. Tests: new `crates/fe-lsp/src/tests/hover/inlay_hints.rs`, declared by `mod inlay_hints;` in
   `crates/fe-lsp/src/tests/hover.rs` (its only change), for UTF-8 and UTF-16 sessions with an emoji before
   the hints:
   - `hints_decode_in_bytes_of_the_version`: a reply with a type hint (string label, paddingLeft) and a
     parameter hint (two label parts, paddingRight) gives two `InlayHint`s at the byte offsets of their
     positions in the requested version, labels `: u8` and `name:`, kinds and padding as sent; the request's
     range is the asked range, encoded.
   - `no_provider_sends_nothing`: without `inlayHintProvider`, `Ok(None)` and no request recorded.
   - `resolve_fills_tooltip_and_edits`: with `resolveProvider`, resolving the type hint sends its `data` back
     and gives the reply's markdown tooltip and its `textEdits` in bytes of the version; a delay past the
     resolve timeout gives the hint as it was and the fake receives `$/cancelRequest`.
   - `inlay_hint_capabilities_are_advertised`: the initialize params carry the two capabilities above.

Linux-OK.
