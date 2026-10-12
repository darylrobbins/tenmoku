---
task_uid: "01m4r8hdp0wjwc1g5h8jq74qjh"
task: "T82"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test signature_help::tests::labels_and_parameters: parameters given as label offsets (UTF-16, with a non-ASCII label) and as strings both become byte ranges of the label; active signature and parameter out of range are clamped; no signatures is None"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test signature_help::tests::context_and_triggers: the request's context carries the trigger kind, character, retrigger flag and the active help; signature_help_triggers gives the server's trigger and retrigger characters once it's ready"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-lsp and cargo clippy -p fe-lsp --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Ask language servers for signature help in fe-lsp: signatures with parameter ranges in their labels, the active ones, and the trigger characters (Mac T471)

## Problem

The user (2026-10-12) asked for LSP signature help. T465 declares the capability; nothing sends
`textDocument/signatureHelp` or exposes the server's trigger characters.

## Change

In `crates/fe-lsp/src/signature_help.rs` (the stub module T465 adds), an `impl LspManager` block:

- `signature_help(buffer, version, offset, context: SignatureContext) -> impl Future<Output =
  Result<Option<SignatureHelp>, LspError>>`. `SignatureContext { trigger: Invoked | Character(char) | ContentChange,
  is_retrigger: bool, active: Option<SignatureHelp> }` becomes the request's `SignatureHelpContext`.
- `SignatureHelp { signatures: Vec<Signature>, active_signature: usize, active_parameter: Option<usize> }`;
  `Signature { label: String, documentation: Option<String>, parameters: Vec<Range<usize>>, active_parameter:
  Option<usize> }`, where each parameter is the byte range of its text in `label`: from `labelOffsets` (UTF-16
  offsets in the label, converted) or by finding a string label in `label` from left to right. Documentation is
  markdown or plain text as hover keeps it. Out-of-range active indexes are clamped; no signatures is `None`.
- `signature_help_triggers(buffer) -> Option<(Vec<char>, Vec<char>)>`: the server's trigger and retrigger
  characters for the buffer's server, once it's ready.
- Timeout as hover's; a timeout is `Ok(None)`.

## Tests

New file `crates/fe-lsp/src/signature_help_tests.rs`, declared from signature_help.rs with `#[path]`, on the fake
server.
