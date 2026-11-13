---
task_uid: "01m7aavfhh7vy9w7t1pc2mgb2k"
task: "T604"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "In crates/fe-chat/tests/chat_picker.rs: picker_empty_chat_takes_picked_model_and_effort, picker_model_switch_applies_next_turn_history_unchanged, picker_effort_change_appends_effort_message_top_level_unchanged, picker_old_chat_file_without_asked_fields_loads pass"
    required: true
    manual: false
  - id: "C2"
    text: "In crates/fe-chat-anthropic/tests/mid_chat_effort.rs: effort_message_serialises_with_empty_content, effort_message_adds_mid_conversation_beta_beside_fallback_beta, no_effort_message_no_mid_conversation_beta pass"
    required: true
    manual: false
  - id: "C3"
    text: "chat_panel::chat_picker_tests::header_shows_model_and_effort, chat_panel::chat_picker_tests::pick_on_turned_chat_applies_on_next_send and chat_panel::chat_picker_tests::typed_in_model_listed pass"
    required: true
    manual: false
  - id: "C4"
    text: "docs/decisions/5at38c8v89-ai-chat.md § 4 no longer says model and effort are fixed per conversation (show_thinking still is) and § 17 no longer lists per-message effort as out of scope"
    required: true
    manual: false
---

# In-chat model and effort picker in the chat header: switch model or effort between turns, append-only history, per-message effort through the effort-only system message (Mac T1780)

The user (2026-11-13) asked for an in-chat model and effort picker. Today the model and effort are only Settings rows (`chat.model` from `fe_config::chat_models::KNOWN`, `chat.effort`), read once by `new_chat_settings` (crates/fe-workbench/src/chat_panel.rs) and fixed per conversation (`Conversation::new`, crates/fe-chat/src/conversation.rs; record docs/decisions/5at38c8v89-ai-chat.md § 4, and § 17 lists per-message effort as out of scope).

## What to build

- **Header control.** The chat panel header (beside History and New Chat) gets a button showing the chat's current model and effort, e.g. "Opus 5.5 · Medium" (`chat-model-picker`). It opens a list in the panel body the way History does (`render_history`): the `KNOWN` models, plus the chat's model if it is not one of them (a typed-in model), and the five efforts, the current ones marked; keyboard and screen-reader operable like the History rows. Model labels come from a new `chat_models::label(id)` (product names, not translated); an unknown id shows as itself.
- **Empty chat** (no turns): a pick sets the conversation's `model` or `effort`.
- **Chat with turns**: a pick applies from the next send, and nothing earlier is edited (history stays append-only, § 4):
  - Model: the next request names the new model; earlier turns go back unchanged, thinking blocks included (other models ignore another model's thinking blocks; the prompt cache is per model, so the first request after a switch is a cache miss).
  - Effort: on models that take it (`chat_models::takes_effort_message`: claude-opus-5-5, claude-sonnet-5-5, claude-fable-5-1), the turn records its effort and the request carries an effort-only system message, `{"role": "system", "content": [], "output_config": {"effort": …}}`, before that turn's user message, with beta `mid-conversation-output-config-2026-07-01` added to `anthropic-beta` (comma-joined with the fallback beta). The top-level `output_config.effort` stays the chat's first effort, so the cached prefix holds. On other models the top-level effort changes instead (a messages-cache miss). Check the shapes against the claude-api skill before writing them.
  - Each `Turn` records what was asked (`asked_model`, `asked_effort`, `#[serde(default)]`), so a saved chat replays the same messages after a reload; chat files written before this card load unchanged.
- `show_thinking` stays fixed per chat. The Settings rows stay the defaults for new chats; the picker does not write settings. Inline edit (crates/fe-chat/src/inline.rs) is unchanged.
- Update docs/decisions/5at38c8v89-ai-chat.md: § 4's "fixed per conversation" bullet (model and effort can change between turns as above; show_thinking stays fixed), § 5 if the effort message changes the layout description, and § 17 (per-message effort is now in, through the effort message).

## Tests (new files)

- crates/fe-chat/tests/chat_picker.rs: requests built after a model switch and an effort change, against the scripted fake provider.
- crates/fe-chat-anthropic/tests/mid_chat_effort.rs: the effort message's JSON and the beta header.
- crates/fe-workbench/src/chat_picker_tests.rs, declared from chat_panel.rs as chat_panel_tests.rs is.

## Notes

The ACP agent's model picker (Claude Code) belongs to the AI agents design card, not here; keep the picker's model list and effort list behind one small function so that card can swap in an agent's options. T1588 also edits chat_panel.rs and chat.ftl (it is blocked on T1565). Read the record sections above before starting. Do not widen --scope; if a file outside it is needed, stop and say so. Ask the lead before any cargo build and before `rgt finish`.

## Implementation notes

- **The effort message on the seam** is a new `Role::System { effort }` with
  empty content (`ApiMessage::effort`, `ApiMessage::effort_change`), so the
  `ApiMessage`/`ChatRequest` literals in `inline.rs` and the fe-chat tests
  (outside the scope) stay as they are. `wire.rs` writes it as
  `{"content": [], "output_config": {"effort": …}, "role": "system"}`;
  `request_headers` now takes the request and comma-joins
  `mid-conversation-output-config-2026-07-01` after the fallback beta when one
  is present (`fe-chat-anthropic/src/provider.rs` passes it: scope rev 2).
- **Which models take it**: fe-chat doesn't depend on fe-config, so
  `ChatSettings::effort_message_models` carries the list (empty by default:
  top-level effort changes, right for any model); the panel passes
  `fe_config::chat_models::EFFORT_MESSAGE_MODELS`, and the view gets it through
  `ChatView::set_effort_message_models` (fe-config is only a dev-dependency of
  fe-chat-view, and `ChatOptions` literals are outside the scope).
- **Replay rule** (`build_request`): on a model that takes the message, the
  top-level effort is the first turn's asked effort and an effort message goes
  before each history turn, and the new one, whose asked effort differs from
  the one in effect; on any other model, no effort messages and the top-level
  effort is the chat's now. The rendering depends only on the saved turns and
  the request's model, so a reload or a switch back replays the same bytes.
  A pick on a chat saved before this card first writes the chat's model and
  effort into its turns (`Conversation::settle_asked`).
- **Header layout**: with a third button the key note ("Key from
  ANTHROPIC_API_KEY") was squeezed to a zero-width column and grew the header
  to ~480 px in the 240 px test sidebar, which moved the chat-apply tests'
  click targets. The note now has its own line under the buttons, and the
  picker button takes the room left, its label ending in an ellipsis.
- **Follow-up (outside the scope)**: `settings-chat-model` in
  `crates/fe-config/l10n/en-US/settings.ftl` still says "A chat keeps the model
  it started with."; it should say the setting is new chats' default and the
  chat header's picker changes an open chat's.
