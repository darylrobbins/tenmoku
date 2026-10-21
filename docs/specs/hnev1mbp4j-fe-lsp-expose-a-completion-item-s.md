---
task_uid: "01m55v1axk6kbd3vhnev1mbp4j"
task: "T176"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::completion::item_command::a_listed_command_runs_through_execute_command passes: an item whose command the server lists (as listed or as resolved) sends one workspace/executeCommand with its arguments and answers Ok; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::completion::item_command::client_and_unlisted_commands_send_nothing passes: editor.action.triggerParameterHints and an unlisted command answer Err(Unavailable) and send nothing; no command answers Ok and sends nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test tests::completion::item_command::experimental_commands_are_advertised passes: capabilities.experimental is {serverStatusNotification: true, commands: {commands: [editor.action.triggerParameterHints, editor.action.triggerSuggest]}}; run_code_action_command uses the shared execute_command and code_action_tests.rs passes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: expose a completion item's command and run a server one through workspace/executeCommand, and list the editor's client commands for rust-analyzer (Mac T926)

## Problem

LSP completeness track, item 1 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
completion has no "item `command`"). A completion item may carry a `command` to run after it is inserted
(LSP 3.17 `CompletionItem.command`). VS Code runs it: a server command through `workspace/executeCommand`
(typescript-language-server's `_typescript.applyCompletionCodeAction`, which adds an import that isn't a plain
text edit), or a client command of its own (`editor.action.triggerParameterHints`, which rust-analyzer attaches
to function completions; `editor.action.triggerSuggest`). fe-lsp keeps the item's `command` only inside its
private `raw` (`crates/fe-lsp/src/completion.rs:75`) and has no way to run it. rust-analyzer only attaches
`editor.action.triggerParameterHints` when the client lists it in `experimental.commands.commands` (its
lsp-extensions "Client Commands"), and fe-lsp's `experimental` is only `{serverStatusNotification: true}`
(`crates/fe-lsp/src/server.rs:2133`).

This card is the fe-lsp half: the item's command and running a server one. The editor runs it on accept in
the next card. The `workspace/executeCommand` sending lives in `run_code_action_command`
(`crates/fe-lsp/src/code_action.rs:218-258`); the code-lens card will need the same, so it is shared here.

## Change

1. `crates/fe-lsp/src/code_action.rs`: factor the request out of `run_code_action_command` into
   `pub(crate) fn execute_command(&self, server: ServerId, command: &lsp_types::Command, from: (BufferId,
   (SessionId, IncarnationId))) -> impl Future<Output = Result<(), LspError>> + Send + 'static`: sent only
   while that session has that opening of the document (as today's `has_open` check), `Unavailable` if not.
   `run_code_action_command` calls it; its behaviour and docs are unchanged.
2. `crates/fe-lsp/src/completion.rs`:
   - `CompletionItem::command(&self) -> Option<&lsp_types::Command>`: the item's command, as resolved if
     it was (the resolved reply replaces `raw` already).
   - `LspManager::run_completion_command(&self, item: &CompletionItem) -> impl Future<Output = Result<(),
     LspError>>`: `Ok(())` and nothing sent for an item without a command; `Err(Unavailable)` and nothing
     sent for one whose command isn't listed in its server's `executeCommandProvider.commands` (client
     commands such as `editor.action.triggerParameterHints` never go to the server); else `execute_command`.
     An edit the server applies while running it arrives as `LspEvent::ApplyEdit`, as for code actions.
   - Module docs: a Command bullet.
3. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `experimental` gains
   `"commands": {"commands": ["editor.action.triggerParameterHints", "editor.action.triggerSuggest"]}` beside
   `serverStatusNotification`. Both are run by the editor (next card).
4. Tests: new `crates/fe-lsp/src/tests/completion/item_command.rs`, declared by `mod item_command;` in
   `crates/fe-lsp/src/tests/completion.rs`, with a fake listing `executeCommandProvider.commands: ["fix.import"]`:
   - `a_listed_command_runs_through_execute_command`: an item whose `command` is `fix.import` with arguments
     `[1, "a"]`: `command()` is it, and `run_completion_command` sends one `workspace/executeCommand` with
     that command and those arguments and answers `Ok(())`; an item whose resolve reply adds the command runs
     it the same way.
   - `client_and_unlisted_commands_send_nothing`: an item with `editor.action.triggerParameterHints`, and one
     with `other.cmd`, each answer `Err(LspError::Unavailable(_))` and the fake records no
     `workspace/executeCommand`; an item without a command answers `Ok(())` and sends nothing.
   - `experimental_commands_are_advertised`: the initialize params' `capabilities.experimental` equals
     `{"serverStatusNotification": true, "commands": {"commands": [the two ids]}}`.
   - The code-action tests in `crates/fe-lsp/src/code_action_tests.rs` pass unchanged.

Linux-OK.
