---
task_uid: "01m55v39h42axqzx4pf5dyt09y"
task: "T174"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::encoding::semantic_tokens::full_tokens_decode_through_the_legend passes for UTF-8 and UTF-16: three tokens on two lines decode to their bytes, type names and modifiers; an out-of-legend type is skipped; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::encoding::semantic_tokens::delta_edits_apply_to_the_previous_data passes: the second request is full/delta with the first resultId, and a delta reply gives the tokens a full reply of the edited stream gives; a provider without delta gets full"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit tests tests::encoding::semantic_tokens::no_provider_sends_nothing and tests::encoding::semantic_tokens::semantic_token_capabilities_are_advertised pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: ask language servers for a document's semantic tokens, full then delta, decoded through the server's legend into byte ranges with type and modifier names (Mac T945)

Filed on argon from Mac T945 (card ~/coordination/argon-cards/zzr/T945.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txzb49crw28xchx1cwjc6. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T850, Mac T918 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T945)".

## Problem

LSP completeness track, item 6 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
"Semantic tokens ✗ (refresh answered null)"). fe-lsp neither advertises `textDocument.semanticTokens`
(`client_capabilities`, `crates/fe-lsp/src/server.rs:1999`) nor asks for them. VS Code asks
`textDocument/semanticTokens/full`, then `full/delta` with the previous `resultId` after edits, and decodes the
integer stream through the server's legend (`semanticTokensProvider.legend`: token types and modifiers by
index). Range requests are only an optimisation for huge files; this card leaves them out.

This is the fe-lsp half, built on T850's `Admitted` helpers and `provider_capabilities`. Colouring is the
workbench card.

## Change

1. New `crates/fe-lsp/src/semantic_tokens.rs`:
   - `#[non_exhaustive] pub struct SemanticToken { pub range: Range<usize>, pub token_type: Arc<str>, pub
     modifiers: Vec<Arc<str>> }`: bytes of the requested version, names from the legend.
   - `#[non_exhaustive] pub struct SemanticTokens { pub tokens: Vec<SemanticToken>, pub result_id:
     Option<String>, data: Vec<u32> }`: `data` is the raw stream, kept to apply a delta to.
   - `LspManager::semantic_tokens(buffer, version, previous: Option<&SemanticTokens>) -> impl Future<Output =
     Result<Option<SemanticTokens>, LspError>>`: `Ok(None)` and nothing sent without a
     `semanticTokensProvider` with `full`; `full/delta` with `previous.result_id` when the provider supports
     delta and there is one, else `full`. A delta reply's edits are applied to `previous.data` (sorted by
     start, as the spec requires) to make the new stream; a full reply replaces it. The stream is decoded
     (relative line, relative start, length, type index, modifier bits) against the requested version's text
     in the session's encoding; a token with an out-of-legend type is skipped, unknown modifier bits are
     ignored, and a token running past its line's end is cut there. A timeout cancels and is `Ok(None)`.
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `textDocument.semanticTokens = {dynamicRegistration:
   false, requests: {full: {delta: true}}, tokenTypes: <the 23 standard types>, tokenModifiers: <the 10
   standard modifiers>, formats: ["relative"], overlappingTokenSupport: false, multilineTokenSupport: false,
   augmentsSyntaxTokens: true}` and `workspace.semanticTokens = {refreshSupport: true}` (refresh is reported by
   the refresh-events card).
3. `crates/fe-lsp/src/requests.rs` `timeout`: the two methods → `timeouts.hover`.
4. `crates/fe-lsp/src/lib.rs`: `mod semantic_tokens`, export `SemanticToken` and `SemanticTokens`.
5. Tests: new `crates/fe-lsp/src/tests/encoding/semantic_tokens.rs`, declared by `mod semantic_tokens;` in
   `crates/fe-lsp/src/tests/encoding.rs` (its only change), for UTF-8 and UTF-16 sessions with an emoji before
   a token:
   - `full_tokens_decode_through_the_legend`: with legend types `[namespace, function, parameter]` and
     modifiers `[declaration, defaultLibrary]`, a full reply of three tokens on two lines gives three
     `SemanticToken`s at the right bytes, with their names and modifiers; an out-of-legend type index is
     skipped.
   - `delta_edits_apply_to_the_previous_data`: a second request sends `full/delta` with the first reply's
     `resultId`; a delta reply replacing the second token's five integers gives the tokens a full reply of the
     edited stream would; a provider without delta gets `full` again.
   - `no_provider_sends_nothing`: without `semanticTokensProvider`, `Ok(None)` and no request recorded.
   - `semantic_token_capabilities_are_advertised`: the initialize params carry both capabilities.

Linux-OK.

## Implementation notes

- The two requests are local `Request` types (`FullRequest`, `DeltaRequest`) under the standard method names,
  whose results keep the stream and each edit's `data` as plain integers: lsp-types groups them into
  five-integer tokens and rejects an edit of one integer, which the LSP spec allows (review F1).
- `manager.rs` is outside the scope, so the request goes through `document_request_if_provided` (for the
  configured `lsp.timeouts.hover`), with `full` or `full/delta` picked when the call is made. The
  capabilities are read between two reads of the document's opening (`active_identity`), which a new
  session changes, and the legend is tagged with that opening's session (review F2):
  - A reply admitted in another session than the legend's (a restart, or a document not yet open when
    asked) is `Err(LspError::Retry)`: asked again, it is decoded by its own legend.
  - A delta is asked only while the opening is the one the previous answer came from, and a delta reply is
    applied only if the request was admitted in that opening; otherwise `Err(Retry)`, and the next request
    is `full`. (Which method goes can't be decided at admission without changing `manager.rs`; a request
    whose opening changes between the call and its admission may still name the old `resultId`, but its
    delta is never applied.)
  - While the server is initializing (no capabilities yet) nothing is sent: the call waits for them through
    `document_request_if_provided` with a check that refuses, then answers `Err(Retry)`.
- A null reply is no tokens, and a token that comes to an empty range (past the text's end, say) is skipped.
- The standard type and modifier lists are `semantic_tokens::TOKEN_TYPES` and `TOKEN_MODIFIERS`, so
  `server.rs` only gains the two capability fields.
- Beyond the four named tests: `delta_edits_apply_to_the_previous_data` also applies single-integer edits
  given out of order, `a_restart_asks_again_in_full` covers asking during a restart (nothing sent, `Retry`)
  and a previous answer from the old session (`full`), and `another_opening_is_asked_in_full` covers a
  reopen at admission (`Retry`) and the next request (`full`, then `full/delta` again).
