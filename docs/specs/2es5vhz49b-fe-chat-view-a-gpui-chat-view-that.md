---
task_uid: "01m5e17xvey5nfbx2es5vhz49b"
task: "T240"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-chat-view test a_sent_message_streams_into_the_transcript (tests/view.rs): chat.send with a scripted three-delta reply shows the joined text in chat-reply-0, clears the composer and attachments, leaves the turn Done, and the provider recorded one request ending with the composer's text; chat.send on empty text or while streaming sends nothing"
    required: true
    manual: false
  - id: "C2"
    text: "fe-chat-view test stop_cancels_the_stream (tests/view.rs): chat.stop mid-reply drops the provider's stream (dropped() is 1), marks the turn Stopped, keeps the partial text and shows Send again"
    required: true
    manual: false
  - id: "C3"
    text: "fe-chat-view test states_show_their_banner_and_action (tests/view.rs): no provider shows Add your Anthropic API key with a button emitting SetKeyRequested; a 401 banner does the same; a 429 failure shows Retry and chat.retry sends a request equal to the failed one; a refusal shows Claude declined this request with stop_details' category; a reply served by another model shows Answered by that model; a max_tokens stop shows the cut-off banner"
    required: true
    manual: false
  - id: "C4"
    text: "fe-chat-view test thinking_shows_only_when_asked (tests/view.rs): with show_thinking false no chat-thinking-0 is drawn even when the reply has a thinking block; with it true a collapsed disclosure opens to the summary text"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check-no-gpui.sh lists fe-chat-view in UI_CRATES and passes; fe-chat-view doesn't depend on fe-workbench; TurnFinished is emitted once per finished turn"
    required: true
    manual: false
---

# fe-chat-view: a GPUI chat view that streams replies, stops, retries and shows the record's error, refusal and fallback states, with plain-text replies (Mac T967)

# fe-chat-view: a chat view that streams replies, stops, retries and shows errors and refusals, with plain-text replies

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) puts the
chat's UI in `crates/fe-chat-view`, a GPUI view that knows nothing of fastedit's workbench, so
the host product can embed it (record §1). `fe-chat` has the conversation and provider seam,
and `fe-chat-anthropic` a real provider; there's no view. kiln's Kiln Chat example
(`kiln/crates/kiln/examples/host/conversation.rs`) shows the shape of a conversation item with
a field below its messages, but it has no backend, streaming or states.

This card builds the view with replies as plain text; Markdown rendering (the Markdown card, held until T846 and
T847 land) and
applying code blocks are later cards.

## Change

New crate `crates/fe-chat-view` ("The AI chat's GPUI view: transcript, composer and
attachments, over any fe-chat provider."), registered in the workspace `Cargo.toml`, and added
to `UI_CRATES` in `scripts/check-no-gpui.sh`. Dependencies: gpui, kiln (for
`#[kiln::command]`, with its own `kiln::module!` and a `pub fn init(cx)`), kiln-ui, fe-chat,
fe-gpui, fe-editor, fe-buffer, futures, log. Not fe-workbench. Dev: gpui and gpui_platform
with `test-support`.

`src/view.rs`: `pub struct ChatView`, an entity with `Render`, `Focusable` and
`EventEmitter<ChatViewEvent>`.

- `ChatView::new(provider: Arc<dyn ChatProvider>, conversation: Conversation, options:
  ChatOptions { max_tokens, fallbacks }, window, cx)`; `conversation()`;
  `set_provider(Option<Arc<dyn ChatProvider>>)` (`None` is the no-key state);
  `attach(ContextItem)` and `attachments()` for the context card; `set_notice(bool)`.
- **Transcript**, in a scroll area that follows the tail while a reply streams unless the
  user has scrolled up. Each turn: the user's text and its attachment labels; the reply as
  plain text, updated as deltas arrive; with `show_thinking`, a collapsed "Thinking"
  disclosure above it holding the summarised reasoning; a note "Answered by `<model>`" when
  the serving model isn't the conversation's; and the record §14 banner for its state, with its
  action. Debug selectors `chat-turn-<i>`, `chat-reply-<i>`, `chat-thinking-<i>`,
  `chat-banner-<i>`.
- **Composer**: an fe-gpui `EditorView` over an untitled buffer, soft-wrapped, growing from 1
  to 10 rows, in key context `chat_composer`, vim off; Send and Stop buttons beside it (Stop
  while a reply streams); the pending attachments as chips with a remove button above it.
- **Commands** (`#[kiln::command(context = "chat")]` on `ChatView`): `chat.send` (sends the
  composer's text and the attachments: `render_user_content`, `begin_turn`, `build_request`,
  then a task applying the stream's events to the conversation, notifying per event; clears
  the composer and attachments; does nothing on empty text or while streaming), `chat.stop`
  (drops the task, which cancels the stream, and `Conversation::stop`), `chat.retry` (resends
  the last failed or refused turn's user content as a new turn). Keys come in the commands card.
- **No key**: with no provider, the composer is disabled and the panel says "Add your
  Anthropic API key" with a Set API Key button emitting `ChatViewEvent::SetKeyRequested`.
  A 401 banner's button emits the same.
- **First-run notice** when `set_notice(true)`: record §10's text (what is sent, to whom,
  fallback, where chats are kept), with a Got It button emitting `NoticeDismissed`.
- A footer meter, "`<n>`k of 1M tokens", from the last reply's `Usage::total_input`; at 80 %
  it says "Long chat: start a new one for better answers".
- `ChatViewEvent::TurnFinished` after every turn ends, any way (persistence uses it).

Colours from kiln-ui's `UiTheme` (and `EditorTheme` for the composer), no literals.

Tests in `crates/fe-chat-view/tests/view.rs` (top-level fns) on GPUI's test platform with
`fe_chat::ScriptedProvider`, finding elements by debug selector.

## Implementation notes

- The commands are `SendMessage`, `StopReply` and `RetryTurn` (`chat.send`, `chat.stop`,
  `chat.retry`): a method named `send` would make a command type `Send`, shadowing the
  marker trait in the module.
- `ChatView::turn_display(i)` is what turn `i` shows (question, attachment labels, thinking,
  reply, "Answered by", banner); render draws it and the tests read it beside the debug
  selectors, which give only bounds.
- Banners follow § 14: 401 Set API Key; 404 Open Settings (`ChatViewEvent::SettingsRequested`);
  400 and refusals Edit, 413 Remove Attachments: both put the question back in the composer
  and the turn's attachments back as removable chips, read back from its user content (one
  tagged block per attachment, then the question), so a resend sends the same context unless
  chips are taken off. 429, 500, 529, network and mid-stream errors Retry. Retry, Edit and
  Remove Attachments show on the last turn only. A stream that ends without `Finished` or
  `Failed` marks the turn failed ("the reply ended early").
- "Answered by" names the last `fallback` block's model when the reply fell back mid-output
  (no new `Started` comes), else the model that started it.
- The composer's height comes from its wrapped rows. A width change rewraps the editor in
  prepaint, after the view rendered, so an empty canvas after the editor checks the row count
  then and asks for another frame when it changed.
- The composer's text is sent as typed; trimming only decides whether it's empty.
- The transcript follows the tail through the frame that draws how the reply ended, then
  stops; scrolling up mid-reply stops it at once.
- Vim off: fe-gpui had no per-editor vim switch, so the scope took in
  `crates/fe-gpui/src/view.rs` and `vim.rs` for `EditorView::set_vim_off`, which `sync_vim`
  honours whatever the global switch says. The composer sets it.
