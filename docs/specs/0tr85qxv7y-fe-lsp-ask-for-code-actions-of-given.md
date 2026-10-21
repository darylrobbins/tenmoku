---
task_uid: "01m55v1bz5dw38fh0tr85qxv7y"
task: "T180"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp unit test code_action::tests::kinds::only_sends_and_filters_kinds passes: only [source.organizeImports] is sent with triggerKind 1, and of quickfix, source.organizeImports, source.organizeImports.ruff, sourcery and a bare command only the two organize-imports actions are kept; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test code_action::tests::kinds::automatic_queries_say_so passes: automatic sends triggerKind 2, and with only None every action is kept in today's order"
    required: true
    manual: false
  - id: "C3"
    text: "code_actions keeps its signature and behaviour (code_action_tests.rs passes, only the mod line added); CodeAction::kind_is applies the dot-prefix rule"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: ask for code actions of given kinds (context.only) or automatically (triggerKind 2), and keep only actions of the kinds asked for (Mac T936)

## Problem

LSP completeness track, item 4 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`: "no
lightbulb, `only` always None (code_action.rs:111), so no Organize Imports / Fix All / codeActionsOnSave").
`LspManager::code_actions` (`crates/fe-lsp/src/code_action.rs:87-138`) always sends `context.only: None` and
`triggerKind: Invoked`. VS Code asks with `only` for its kind-specific commands (Organize Imports sends
`["source.organizeImports"]`, Fix All `["source.fixAll"]`, codeActionsOnSave each configured kind) and with
`triggerKind: Automatic` for the lightbulb, and it filters the reply by kind itself, since servers may ignore
`only`. Every card of item 4 needs this; it is fe-lsp only.

## Change

`crates/fe-lsp/src/code_action.rs`:

1. New `#[derive(Clone, Debug, Default, PartialEq, Eq)] pub struct CodeActionQuery { pub only:
   Option<Vec<String>>, pub automatic: bool }`.
2. New `LspManager::code_actions_for(buffer, version, range, diagnostics, query: CodeActionQuery)`: as
   `code_actions`, but sends `only` (as `CodeActionKind`s) and `triggerKind` `Automatic` when `automatic`, and
   keeps only actions whose kind equals or is a dot-separated child of one in `only` (`source` keeps
   `source.organizeImports`, not `sourcery`). A bare `Command` or an action with no kind is kept only when
   `only` is `None`. Order as today. `code_actions` becomes `code_actions_for(.., CodeActionQuery::default())`,
   unchanged for its callers.
3. `CodeAction` gains `pub fn kind_is(&self, kind: &str) -> bool`, the same prefix rule, for hosts.
4. Module docs: a Kinds bullet.
5. `crates/fe-lsp/src/lib.rs` is untouched: `code_action` is a `pub mod`, so `fe_lsp::code_action::CodeActionQuery`
   is reachable.
6. Tests: new `crates/fe-lsp/src/code_action_kinds_tests.rs`, declared at the end of
   `crates/fe-lsp/src/code_action_tests.rs` as `#[path = "code_action_kinds_tests.rs"] mod kinds;`, with that
   file's fake:
   - `only_sends_and_filters_kinds`: with `only: ["source.organizeImports"]` the request's
     `context.only` is that list and `triggerKind` 1; a reply of `quickfix`, `source.organizeImports`,
     `source.organizeImports.ruff`, `sourcery` and a bare command yields the two organize-imports actions only.
   - `automatic_queries_say_so`: `automatic: true` sends `triggerKind` 2; with `only: None` every action of
     that reply is kept, in today's order.
   - The existing tests in `code_action_tests.rs` pass unchanged.

Order: T918 edits `code_action.rs`'s docs and timeout; the completion-command card factors
`execute_command` out of it. Linux-OK.
