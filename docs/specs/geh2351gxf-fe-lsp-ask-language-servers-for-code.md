---
task_uid: "01m55v1fahttt3tkgeh2351gxf"
task: "T185"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test code_action::tests::code_lens::lenses_decode_in_bytes_of_the_version passes: two lenses past an emoji in a UTF-16 session decode to their byte ranges, sorted, with their commands; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit tests code_action::tests::code_lens::unresolved_lenses_resolve, code_action::tests::code_lens::lens_commands_run_only_when_listed and code_action::tests::code_lens::no_provider_sends_nothing pass"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test code_action::tests::code_lens::code_lens_capabilities_are_advertised passes: textDocument.codeLens, workspace.codeLens.refreshSupport and experimental.commands.commands with the three ids"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited except the one expected-list assertion in crates/fe-lsp/src/tests/completion/item_command.rs (experimental_commands_are_advertised gaining rust-analyzer.showReferences); cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: ask language servers for code lenses, resolve those without a command, and run a lens's server command, advertising rust-analyzer's showReferences client command (Mac T947)

## Problem

LSP completeness track, item 7 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`: "Code
lens ✗ | T858 (UI rows only, no LSP request)"). rust-analyzer ("▶ Run | Debug", "3 implementations",
"5 references"), gopls ("run test", "regenerate cgo") and typescript-language-server (references,
implementations) offer code lenses. VS Code asks `textDocument/codeLens` for each visible document, resolves
the lenses that come without a command (`codeLens/resolve`), and re-asks on `workspace/codeLens/refresh`.
fe-lsp advertises no `codeLens` capability and sends none of these.

rust-analyzer only sends its reference and implementation lenses (and run lenses as client commands) when the
client lists the client commands it can run in `experimental.commands.commands` (its lsp-extensions "Client
Commands": `rust-analyzer.showReferences`, `rust-analyzer.runSingle`, ...); the completion-command card adds
that list with the two editor commands.

This card is the fe-lsp half, on T850's `Admitted` helpers and `provider_capabilities`, and the completion-command
card's `execute_command`. Showing lenses is the workbench card (on T858's lens rows).

## Change

1. New `crates/fe-lsp/src/code_lens.rs`:
   - `#[non_exhaustive] pub struct CodeLens { pub range: Range<usize>, pub command: Option<lsp_types::Command>
     }` (bytes of the requested version), plus private `raw`, `server` and `from` as completion items keep them.
   - `LspManager::code_lenses(buffer, version) -> impl Future<Output = Result<Option<Vec<CodeLens>>,
     LspError>>`: `Ok(None)` and nothing sent without a `codeLensProvider`; a null reply or a timeout
     (cancelled) is `Ok(Some(vec![]))`; sorted by range start.
   - `LspManager::resolve_code_lens(lens)`: only to a server whose provider has `resolveProvider`, only while
     the session that gave it has that opening; the lens as it was on a timeout.
   - `LspManager::run_lens_command(lens)`: as `run_completion_command`: `workspace/executeCommand` through
     `execute_command` if the server's `executeCommandProvider.commands` lists it, else
     `Err(Unavailable)` with nothing sent (client commands are the host's).
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `textDocument.codeLens = {dynamicRegistration:
   false}`, `workspace.codeLens = {refreshSupport: true}`, and `experimental.commands.commands` gains
   `"rust-analyzer.showReferences"` (the references card runs it; `editor.action.showReferences` needs no
   advertising).
3. `crates/fe-lsp/src/requests.rs` `timeout`: `textDocument/codeLens` → `timeouts.hover`, `codeLens/resolve` →
   `timeouts.resolve`.
4. `crates/fe-lsp/src/lib.rs`: `mod code_lens`, export `CodeLens`.
5. Tests: new `crates/fe-lsp/src/code_lens_tests.rs`, declared at the end of
   `crates/fe-lsp/src/code_action_tests.rs` as `#[path = "code_lens_tests.rs"] mod code_lens;`, with that file's
   fake:
   - `lenses_decode_in_bytes_of_the_version`: two lenses (one with a command, one without) on rows after an
     emoji in a UTF-16 session decode to their byte ranges, sorted, with their commands.
   - `unresolved_lenses_resolve`: with `resolveProvider`, resolving the lens without a command sends its
     `data` and gives the reply's command; without `resolveProvider`, nothing is sent.
   - `lens_commands_run_only_when_listed`: a lens whose command the server lists sends
     `workspace/executeCommand` with its arguments; `rust-analyzer.showReferences` (not listed) gives
     `Err(Unavailable)` and sends nothing.
   - `no_provider_sends_nothing`; and `code_lens_capabilities_are_advertised` (including the experimental
     list's three ids).

Linux-OK.

## Implementation notes

- Advertising `rust-analyzer.showReferences` made the exact-match assertion in
  `tests/completion/item_command.rs::experimental_commands_are_advertised` stale; its expected list gained that id
  (the only existing test touched, outside the listed scope but forced by the spec's change).
