---
task_uid: "01m7avvhcm8m297mwjyyzpfj6k"
task: "T613"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo test -p fe-config --test chat_known_models_exist known_models_are_real_ids passes: KNOWN is a subset of the four real ids with opus first"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-chat-anthropic --test haiku_request haiku_request_has_no_effort_or_thinking passes: request_body for claude-haiku-4-5 with show_thinking true has no output_config and no thinking key; claude-opus-5-5 still carries output_config.effort and adaptive thinking"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench chat_picker passes, with a new test in chat_panel_tests.rs that the effort rows are disabled for claude-haiku-4-5; cargo test -p fe-chat -p fe-chat-view -p fe-config passes"
    required: true
    manual: false
  - id: "C4"
    text: "rg 'haiku-5-5|Haiku 5.5' crates finds nothing, and settings-chat-model no longer says a chat keeps its model"
    required: true
    manual: false
---

# Fix the chat model list: claude-haiku-5-5 does not exist; use claude-haiku-4-5 and send it no effort or thinking (Mac T1781)

`crates/fe-config/src/chat_models.rs` lists `claude-haiku-5-5` ("Haiku 5.5"), which does not exist; choosing it in Settings or the chat model picker sends every request to a nonexistent model. The current Haiku is `claude-haiku-4-5` (exact id, no date suffix, 200K context).

Haiku 4.5 also differs on the wire: `output_config.effort` errors on it, and its thinking is `{type: "enabled", budget_tokens: N}` (min 1024, below max_tokens) or none; adaptive thinking is for the 4.6+ models. `crates/fe-chat-anthropic/src/wire.rs::request_body` always sends `output_config.effort` and, with show_thinking, `thinking: {type: adaptive}`.

Fix:
- KNOWN and label(): `claude-haiku-4-5` / "Haiku 4.5" in the same slot (third). Opus 5.5 stays the default; sonnet-5-5 and fable-5-1 unchanged.
- Add `chat_models::takes_effort(id) -> bool` (false only for `claude-haiku-4-5`).
- request_body: for a model that does not take effort, omit `output_config` and omit `thinking` entirely (no adaptive, no budget; keep it minimal). Other models' bodies are byte-identical to today.
- Chat picker (chat_panel.rs render_picker): the effort rows are disabled (not clickable, not marked) while the chat's model does not take effort.
- settings.ftl `settings-chat-model`: drop "A chat keeps the model it started with." (the picker now switches model between turns); reword to "The Claude model new chats start with."
- Update the existing tests that name `claude-haiku-5-5` (they use it as an arbitrary model id) to `claude-haiku-4-5`.

Conflicts: none found. No running task touches fe-chat, fe-chat-anthropic or chat_models; T1779 (AI agents design, blocked) only writes docs/decisions; T1589/T1590/T1592/T1597 (ready) call into the chat provider but do not edit these files.

Where the model check lives: fe-chat-anthropic does not depend on fe-config, and this card must not add that dependency. Give wire.rs its own private check on `request.model` (the one id, `claude-haiku-4-5`, that takes no effort), with a comment pointing at `fe_config::chat_models::takes_effort`, which the picker uses. A model id the app doesn't know (typed via Other…) keeps today's body.
