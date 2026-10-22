---
task_uid: "01m55v1bbp6hhyn5gsgbagf6c5"
task: "T177"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::completion_tests::command::accepting_runs_the_items_command passes: editor.action.triggerSuggest asks again with Invoked, editor.action.triggerParameterHints shows signature help, another id reaches CompletionProvider::run_command once after the edit, none runs nothing; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::lsp_resolve_retry::completion_command::accepting_a_completion_runs_its_server_command passes: accepting foo sends workspace/executeCommand add.import with [foo], and the applyEdit the fake then pushes lands with the completion; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "CompletionProvider::run_command has a default that does nothing, so other providers compile unchanged; the manager's lock is not held across run_completion_command's await"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-editor -p fe-gpui -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-editor -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Run an accepted completion's command, as VS Code does: parameter hints and suggestions in the editor, server commands through the language server (Mac T927)

## Problem

LSP completeness track, item 1 (user-approved 2026-10-17). Once the card this depends on lands, fe-lsp exposes a
completion item's `command` (`fe_lsp::CompletionItem::command`) and runs a server command
(`LspManager::run_completion_command`), and advertises the client commands `editor.action.triggerParameterHints`
and `editor.action.triggerSuggest`. Accepting an item still runs nothing: fe-gpui's `completion.accept`
(`crates/fe-gpui/src/completion_menu.rs:318-325`) applies the edit and stops, and fe-editor's `CompletionItem`
(`crates/fe-editor/src/completion.rs:39-58`) can't say there is a command. VS Code runs the command after the
insert: its own client commands itself (rust-analyzer's function completions then open parameter hints) and
server commands through `workspace/executeCommand` (TypeScript's `_typescript.applyCompletionCodeAction`).

## Change

1. `crates/fe-editor/src/completion.rs`: `CompletionItem` gains `pub command: Option<String>`, the id of a
   command to run once the item is accepted (default none). Docs say the editor runs the two client ids
   itself and hands others to the source.
2. `crates/fe-gpui/src/completion_menu.rs`:
   - `CompletionProvider` gains `fn run_command(&self, item: CompletionItem) {}` (a default that does
     nothing, so other providers are unaffected): run the item's command, which isn't one of the editor's.
   - After an accept that applied (`CompletionSession::accept` gave `Ok(Some(_))`), from `completion.accept`
     and from the commit-character path alike, the accepted item's `command`:
     - `editor.action.triggerSuggest` asks for completions again (`CompletionTrigger::Invoked`);
     - `editor.action.triggerParameterHints` asks for signature help as `editor.show_signature_help` does;
     - any other id goes to the provider's `run_command`, once, after the edit;
     - none runs nothing. An accept that applied nothing runs nothing.
3. `crates/fe-workbench/src/lsp.rs`: `completion_item` copies `item.command().map(|c| c.command.clone())`;
   `LspHover`'s `run_command` takes the fe-lsp item from `data` and spawns `run_completion_command` without
   the manager's lock held across the await (as `resolve` does), logging a failure at debug.
4. Tests (new files, one mod line each in an existing test file):
   - `crates/fe-gpui/src/view/completion_command_tests.rs`, declared at the end of
     `crates/fe-gpui/src/view/completion_tests.rs` as `#[path = "completion_command_tests.rs"] mod command;`,
     with a provider of its own that records `completions` and `run_command` calls:
     `accepting_runs_the_items_command`: accepting an item with `editor.action.triggerSuggest` makes a second
     `completions` call with `Invoked` at the new caret; one with `editor.action.triggerParameterHints` shows
     the test signature-help provider's help; one with `x.cmd` calls `run_command` once with that item after
     its text is in the buffer; one with none calls nothing.
   - `crates/fe-workbench/src/tests/lsp_completion_command.rs`, declared at the end of
     `crates/fe-workbench/src/tests/lsp_resolve_retry.rs` as `#[path = "lsp_completion_command.rs"] mod
     completion_command;` (T857 holds `tests/lsp_completion.rs`, T920 `tests/mod.rs`), with the fake server
     harness of `tests/lsp.rs` as `tests/lsp_completion.rs` uses it: `accepting_a_completion_runs_its_server_command`. The fake lists
     `executeCommandProvider.commands: ["add.import"]` and offers `foo` with command `add.import` and arguments
     `["foo"]`. After accepting `foo`, the fake has received that `workspace/executeCommand` with its
     arguments; it then pushes a `workspace/applyEdit` inserting `use foo;\n` at row 0 from a thread (as
     `lsp_code_actions_tests.rs` `commands_and_disabled` does), and the buffer ends with the completion and
     the import.

Linux-OK.
