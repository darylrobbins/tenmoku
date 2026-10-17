---
task_uid: "01m55ef6ck54yj8zfnzf16zy02"
task: "T171"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-nav integration tests related_goes_to_the_first_ready_provider_that_answers and related_is_unsupported_when_no_provider_answers (crates/fe-nav/tests/related.rs, top level) pass: Navigation::related routes as definitions does, a provider keeping the default NavigationProvider::related (NavError::Unsupported) is skipped, and with only defaults the answer is Err(Unsupported); they fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::navigation::related_requests_decode_as_definitions passes: for Related::Declaration, TypeDefinition and Implementation the fake gets textDocument/declaration, typeDefinition or implementation at the caret's position, and LocationLink, Location-array and single-Location replies decode to the same paths and byte ranges LspNavigation::definitions gives for that reply; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test tests::navigation::related_without_a_provider_sends_nothing passes: a server whose capabilities lack typeDefinitionProvider answers NavError::Unsupported and the fake records no textDocument/typeDefinition request; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-nav -p fe-lsp passes with no existing test edited; cargo clippy -p fe-nav -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Answer declaration, type definition and implementation requests from language servers through fe-nav's NavigationProvider, decoded as definitions are (Mac T851)

## Problem

VS Code's Go to Declaration, Go to Type Definition and Go to Implementations (`editor.action.revealDeclaration`,
`editor.action.goToTypeDefinition`, `editor.action.goToImplementation`, cmd-F12) ask the language server
`textDocument/declaration`, `textDocument/typeDefinition` and `textDocument/implementation`. fastedit has none:
fe-nav's `NavigationProvider` (`crates/fe-nav/src/provider.rs:293`) has only `definitions`, `references` and
`workspace_symbols`, and fe-lsp's `LspNavigation` (`crates/fe-lsp/src/navigation.rs:103`) sends only
`GotoDefinition`, `References` and `WorkspaceSymbolRequest`. Gap audit 2026-10-17 (user-approved).

All three replies have definition's shape (`GotoDefinitionResponse`: a Location, Locations or LocationLinks), so
the decoding in `LspNavigation::definitions` (`navigation.rs:135`: `targets`, `Decoder`, the `NOT_FINAL` rule) serves
them unchanged. This card is the headless half; the workbench commands are a card of their own that depends on it.

## Change

1. `crates/fe-nav/src/provider.rs`:
   - `pub enum Related { Declaration, TypeDefinition, Implementation }` (Copy, Debug, PartialEq, Eq), re-exported
     from `crates/fe-nav/src/lib.rs`.
   - On `NavigationProvider`, `fn related(&self, kind: Related, req: &NavRequest, cancel: &AtomicBool) ->
     Result<Definitions, NavError>`, defaulting to `Err(NavError::Unsupported)` as `references` does, so
     `SearchNavigation` and `ScipNavigation` keep the default and are skipped.
   - `Navigation::related(&self, kind, req, cancel) -> Result<Answer<Definitions>, NavError>`, routed exactly as
     `Navigation::definitions` is (`self.route(req.language, cancel, .., || false)`).
2. `crates/fe-lsp/src/navigation.rs`: `LspNavigation` implements `related` by sending
   `lsp_types::request::GotoDeclaration`, `GotoTypeDefinition` or `GotoImplementation` and decoding as
   `definitions` does. Factor the body of `definitions` into one generic fn over a request whose params are
   `GotoDefinitionParams` and whose result is `Option<GotoDefinitionResponse>` (the three aliases are), called by
   both. Before sending, check the buffer's server with `LspManager::provider_capabilities` (from the seam card) and
   the matching provider field (`declaration_provider`, `type_definition_provider`, `implementation_provider`, any
   value but `None`/`false`): a server that doesn't provide it gets no request and the call answers
   `NavError::Unsupported`, so nothing else is asked for it either. `unambiguous` is `items.len() == 1`, as for
   definitions; an implementation request answering several locations lists them all.
3. Tests:
   - New integration test `crates/fe-nav/tests/related.rs`, top-level fns, with the in-file fake providers
     `crates/fe-nav/tests/provider.rs` uses as a model (copy what's needed; don't edit that file):
     `related_goes_to_the_first_ready_provider_that_answers` (a provider answering `related` wins over a later
     one; a provider keeping the default is skipped for the next) and
     `related_is_unsupported_when_no_provider_answers` (only the default: `Err(NavError::Unsupported)`).
   - In the existing `crates/fe-lsp/src/tests/navigation.rs`, built as its definition tests are with the fake
     server: `related_requests_decode_as_definitions` (for each of the three methods, a server whose
     `ServerCapabilities` provide it, scripted to answer a LocationLink array for one, a Location array for another
     and a single Location for the third: the request is sent with the caret's position in the buffer's version,
     and the answer's items are the same byte ranges and paths `definitions` gives for the same reply) and
     `related_without_a_provider_sends_nothing` (capabilities without `typeDefinitionProvider`: `Unsupported`, and
     the fake records no `textDocument/typeDefinition` request).
