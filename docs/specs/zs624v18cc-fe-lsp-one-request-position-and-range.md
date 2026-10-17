---
task_uid: "01m55ef5txqgbhqqzs624v18cc"
task: "T170"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::lifecycle::initialize_advertises_navigation_and_highlight_capabilities passes: the initialize params' capabilities.textDocument.declaration, .typeDefinition and .implementation each equal {dynamicRegistration: false, linkSupport: true} and .documentHighlight equals {dynamicRegistration: false}; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::lifecycle::new_requests_time_out_as_their_neighbours passes: requests::timeout with LspTimeouts::default() gives textDocument/declaration, typeDefinition and implementation the duration of textDocument/definition, and textDocument/documentHighlight that of textDocument/hover; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "Admitted::position and Admitted::offsets in crates/fe-lsp/src/document.rs replace the private position fns of hover.rs, completion.rs, navigation.rs and rename.rs (rg 'fn position\\(at: \u0026Admitted' finds only signature_help.rs's), and LspManager::provider_capabilities replaces rename_capabilities, which is gone; snippetSupport is still false"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: one request position and range helper, one provider check, and declaration, typeDefinition, implementation and documentHighlight advertised with their timeouts, so the LSP feature cards touch disjoint files (Mac T850)

## Problem

The VS Code gap audit (user-approved 2026-10-17: "a verified gap audit, then card the LSP-completeness set and
merge conflicts first"; ~/.local/share/fastedit-lead/gap-audit.md) finds go to declaration / type definition /
implementation, document highlights and LSP snippet completion missing. The cards for them would each repeat the
same three pieces of fe-lsp plumbing and collide on the same lines:

- the generic request is already there (`LspManager::document_request<R>`, `crates/fe-lsp/src/manager.rs:686`),
  but every feature module carries its own private, identical `fn position(at: &Admitted, offset: usize) ->
  TextDocumentPositionParams`: `hover.rs:81`, `completion.rs:202`, `navigation.rs:448`, `rename.rs:189` (and
  `signature_help.rs:166`, left alone here because inbox T827 owns that file);
- a reply's `lsp_types::Range` is decoded to bytes inline (`hover.rs` `convert`: `from_lsp` both ends, then
  `start.min(end)..end.max(start)`);
- "does the buffer's server provide this at all" is rename's private `rename_capabilities` (`rename.rs:156`);
- each feature adds its client capability to the one `client_capabilities()` fn (`crates/fe-lsp/src/server.rs:1999`),
  so every LSP card would conflict on `server.rs`.

## Change

A refactor plus capability advertisement, so the feature cards that depend on this one touch disjoint files.

1. `crates/fe-lsp/src/document.rs`: on `Admitted`, add
   - `pub(crate) fn position(&self, offset: usize) -> TextDocumentPositionParams` (exactly the body of today's
     copies: the document's uri, `to_lsp` of `offset` clipped `Bias::Left` in the sent text, in the session's
     encoding);
   - `pub(crate) fn offsets(&self, range: lsp_types::Range) -> Range<usize>`: both ends through `from_lsp` against
     the sent text, ordered (`start.min(end)..end.max(start)`), as hover's `convert` does.
   Delete the private `position` fns of `hover.rs`, `completion.rs`, `navigation.rs` and `rename.rs` and call
   `at.position(offset)`; use `at.offsets(..)` in `hover.rs` `convert`.
2. `crates/fe-lsp/src/manager.rs`: `pub(crate) fn provider_capabilities(&self, buffer: BufferId, feature: &str,
   provides: impl Fn(&ServerCapabilities) -> bool) -> Result<Option<ServerCapabilities>, LspError>`, generalising
   `rename_capabilities`: `Ok(None)` while the buffer's server has no capabilities yet (starting), `Err(LspError::
   Unavailable(feature.into()))` if they say it doesn't provide it, else `Ok(Some(caps))`. `rename.rs` uses it with
   `"rename"` and its `renames` predicate; `rename_capabilities` goes.
3. `crates/fe-lsp/src/server.rs` `client_capabilities()`: advertise `textDocument.declaration`, `typeDefinition` and
   `implementation` as `GotoCapability { dynamic_registration: dynamic, link_support: Some(true) }` and
   `documentHighlight` as `DynamicRegistrationClientCapabilities { dynamic_registration: dynamic }`. These are
   client capabilities only: a server answers them when asked and sends nothing because of them, so advertising
   them before the requests land (the dependent cards) changes no server's behaviour. `snippetSupport` stays
   false (the snippet card flips it, with the engine in place).
4. `crates/fe-lsp/src/requests.rs` `timeout`: today any method it doesn't list has no timeout (`_ => return None`).
   Give `textDocument/declaration`, `textDocument/typeDefinition` and `textDocument/implementation`
   `timeouts.definition`, and `textDocument/documentHighlight` `timeouts.hover`, so the feature cards' requests
   time out as their VS Code counterparts' neighbours do here.
5. New test in the existing `crates/fe-lsp/src/tests/lifecycle.rs`, next to `initialize_advertises_m4_capabilities`
   and built the same way: `initialize_advertises_navigation_and_highlight_capabilities` asserts the initialize
   params' `capabilities.textDocument.declaration`, `.typeDefinition` and `.implementation` equal
   `{"dynamicRegistration": false, "linkSupport": true}` and `.documentHighlight` equals
   `{"dynamicRegistration": false}`; and `new_requests_time_out_as_their_neighbours` asserts
   `crate::requests::timeout` with `LspTimeouts::default()` gives the three goto methods the same duration as
   `textDocument/definition` and `textDocument/documentHighlight` the same as `textDocument/hover`.

No behaviour changes beyond the four advertised capabilities and the four timeouts.
