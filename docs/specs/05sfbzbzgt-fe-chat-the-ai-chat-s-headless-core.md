---
task_uid: "01m57yv3w3816b5y05sfbzbzgt"
task: "T189"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-chat test a_streamed_reply_is_rebuilt_verbatim (tests/conversation.rs): scripted events for a thinking block with its signature and two text blocks, applied with Conversation::apply, leave the turn Done with its reply blocks equal to the BlockFinished values in order, the UI text equal to the joined deltas, and the serving model from Started"
    required: true
    manual: false
  - id: "C2"
    text: "fe-chat test history_follows_the_append_only_rules (tests/conversation.rs): a done turn's user content and reply blocks (thinking included) come back unchanged; a stopped turn keeps its user content and only its finished text blocks, or leaves when it has none; refused and failed turns leave; after a mid-output fallback the thinking, redacted_thinking and tool_use blocks before the last fallback block are left out"
    required: true
    manual: false
  - id: "C3"
    text: "fe-chat test consecutive_requests_share_a_byte_identical_prefix (tests/request.rs): over three turns, each request's serialized system and messages are a byte-for-byte prefix of the next request's; the system block carries cache_control ephemeral; no tools are sent; model, effort and show_thinking come from the conversation even after the settings passed change"
    required: true
    manual: false
  - id: "C4"
    text: "fe-chat test the_scripted_provider_records_requests_and_stops_when_dropped (tests/request.rs): it plays its replies in order, requests() holds each ChatRequest, and a stream dropped mid-reply counts in dropped() and yields nothing more"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check-no-gpui.sh reports ok: fe-chat; cargo tree -p fe-chat -e normal --depth 1 lists only serde, serde_json and futures; cargo deny check passes"
    required: true
    manual: false
---

# fe-chat: the AI chat's headless core: conversation, provider trait and events, cache-stable request assembly, and a scripted provider (Mac T962)

# fe-chat: the conversation model, the provider trait and request assembly, with a scripted provider

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`, the task this depends on; the user
signed it off) puts the chat's headless core in a new crate, `crates/fe-chat`, that the host
product reuses. Nothing like it exists: the only AI driver is `fe_editor::scripted_ai`, a
script player for the editor seams. This card builds the core every later chat card uses,
without any network: the conversation, the provider seam, how a request is assembled for the
prompt cache, and a scripted provider for tests.

## Change

New crate `crates/fe-chat` ("The AI chat's headless core: conversations, the provider seam and
request assembly. Must not depend on GPUI."), registered in the workspace `Cargo.toml` as
`fe-chat = { path = "crates/fe-chat" }`. Dependencies: `serde`, `serde_json`, `futures`.
Serialisation must be deterministic whether or not another crate turns on serde_json's
`preserve_order` (features unify; see `fe_config::settings_schema`'s `sorted`): build JSON
values in a fixed order and keep received blocks as parsed. Follow the record's
§2 and §4–§5 exactly; where this card and the record differ, the record wins, and say so in the
finish notes.

- `src/provider.rs`: `ChatProvider` (`name`, `stream(ChatRequest) -> ChatStream`, a boxed
  `Stream<Item = ChatEvent> + Send`, dropping it cancels), `ChatRequest`, `ChatEvent`,
  `BlockKind`, `StopReason` (`EndTurn`, `MaxTokens`, `StopSequence`, `ToolUse`, `PauseTurn`,
  `Refusal`, `Other(String)`), `Usage` (input, output, cache creation, cache read; `total_input()`
  their sum), `ProviderError` and `ErrorKind` with `retryable()`, `Effort` (`Low`, `Medium`,
  `High`, `XHigh`, `Max`), and the tool seam the agent loop needs later: `ToolSpec { name,
  description, input_schema, strict }`, `ToolChoice { Auto, None }` (no `Any` or `Tool`: a 400
  on Claude Opus 5.5) and `ToolCall { id, name, input }`. v1 sends no tools.
- `src/conversation.rs`: `ChatId` (a random 128-bit id printed as 26 base32 chars),
  `Conversation { id, title, model, effort, show_thinking, turns }`, `Turn`, `TurnState`
  (`Streaming`, `Done`, `Stopped`, `Refused`, `Failed(ProviderError)`), `Attachment` metadata
  (kind, label, path, rows, estimate) and:
  - `Conversation::begin_turn(user_content: Vec<Value>, attachments)` and
    `Conversation::apply(event)`, the reducer that builds the streaming reply: text and
    thinking accumulate per block index for the UI; `BlockFinished` stores the verbatim block;
    `Finished` and `Failed` set the state; `Started` records the serving model.
  - `Conversation::stop()` marks the streaming turn `Stopped`.
  - `Conversation::history() -> Vec<ApiMessage>`: the record's §4 rules: done turns' user
    content and reply blocks verbatim; a stopped turn's user content plus its finished `text`
    blocks only, or nothing when it has none; refused and failed turns left out; after a
    mid-output fallback, the `thinking`, `redacted_thinking` and `tool_use` blocks before the
    last `fallback` block left out.
  - `title` is the first user line, cut to 60 chars at a char boundary.
  - `Conversation` and its parts derive `Serialize`/`Deserialize` (a later card keeps chats
    on disk); a streaming turn serialises as `Stopped`.
- `src/request.rs`: `SYSTEM_PROMPT`, a constant (the chat's role in a code editor, and the
  record §8 edit-block format: ` ```<lang> path=<relative path> `, whole-file or
  SEARCH/REPLACE sections, no path for a reply to a selection), and
  `build_request(conversation, new_user_content, settings) -> ChatRequest` placing the system
  prompt as one block with `cache_control: {"type": "ephemeral"}`, then the history, then the
  new turn. `render_user_content(question, items: &[ContextItem])` makes the new turn's blocks:
  one `text` block per item, tagged (`<file path="…" rows="…">…</file>`, `<selection …>`,
  `<diagnostics>`, `<open-editors>`), then the question. `ContextItem { kind, label, text }`
  lives here; the budgeting card adds estimates.
- `src/fake.rs`: `ScriptedProvider`: built from a list of scripted replies (each a list of
  `ChatEvent`s, optionally ending in a `Failed`), it plays the next one per `stream` call,
  yielding between events, and records every `ChatRequest` it was given (`requests()`). It
  stops yielding when its stream is dropped (`dropped()` counts drops), so cancellation is
  testable. Public, not behind a feature: the host product's tests use it too.
- `src/lib.rs`: the crate docs (what it is, the record's link) and re-exports.

Tests in `crates/fe-chat/tests/conversation.rs` and `crates/fe-chat/tests/request.rs`
(top-level `#[test]` fns). No network, no GPUI.

`scripts/check-no-gpui.sh` already treats every crate not in `UI_CRATES` as core, so
`fe-chat` is checked without editing it.

## Implementation notes

- `build_request`'s settings are `ChatSettings { model, effort, show_thinking, max_tokens }`
  (`Default` is the record's § 12 defaults). `Conversation::new(&settings)` fixes the chat's
  model, effort and thinking; `build_request` sends those, and only `max_tokens` from the
  settings passed.
- `history()` also leaves out a done turn with no finished blocks: an empty assistant message
  is a 400, and the record doesn't say otherwise.
- The title is the first non-empty line of the user content's last `text` block (the
  question, which `render_user_content` puts last), so attached context never titles a chat.
- Built JSON (`render_user_content`'s blocks) inserts its keys in sorted order, so its bytes
  are the same with or without serde_json's `preserve_order`; received blocks are kept as
  parsed. `SystemBlock` serialises through a hand-written impl in a fixed field order.
- `BlockKind::Fallback { from, to }` holds strings read from the block's `from`/`to`; the
  history filter goes by each finished block's `type`, so the Anthropic card can widen them.
- `ScriptedProvider`: `dropped()` counts streams dropped before their reply ended; once the
  replies run out a stream yields one `Failed` (`InvalidRequest`).
- No differences from the record.
