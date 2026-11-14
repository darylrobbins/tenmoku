// SPDX-License-Identifier: Apache-2.0

//! A `tool_use` input's explicit parse status: a finished non-object input
//! at `max_tokens` is invalid (resent once, never in history), not cut short.

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, Conversation, MIN_TASK_BUDGET,
    READ_FILE, ReplyScript, ScriptedProvider, StopReason, TurnState, build_request,
    render_user_content, round_request, tool_set,
};
use futures::StreamExt;
use futures::executor::block_on;
use serde_json::{Value, json};

fn settings() -> ChatSettings {
    ChatSettings::default()
}

fn chat() -> Conversation {
    let mut chat = Conversation::new(&settings());
    assert!(chat.set_tools(tool_set(), MIN_TASK_BUDGET.max(30_000)));
    chat
}

/// "Reading." then a finished last call at index 1 whose input is `input`
/// as a bare value, ending at `max_tokens`.
fn reply_with_call(input: Value) -> Vec<ChatEvent> {
    let mut events = ReplyScript::new("claude-opus-5-5", "msg")
        .text("Reading.")
        .stop(StopReason::MaxTokens);
    let finished = events.pop().unwrap();
    events.extend([
        ChatEvent::BlockStarted {
            index: 1,
            kind: BlockKind::ToolUse {
                id: "toolu_x".into(),
                name: READ_FILE.into(),
            },
        },
        ChatEvent::ToolInputDelta {
            index: 1,
            partial_json: input.to_string(),
        },
        ChatEvent::BlockFinished {
            index: 1,
            block: json!({"id": "toolu_x", "input": input, "name": READ_FILE, "type": "tool_use"}),
        },
        finished,
    ]);
    events
}

fn play(chat: &mut Conversation, request: &ChatRequest, provider: &ScriptedProvider) {
    for event in block_on(provider.stream(request.clone()).collect::<Vec<_>>()) {
        chat.apply(event);
    }
}

fn committed(chat: &mut Conversation) -> String {
    serde_json::to_string(&round_request(chat, &settings()).unwrap().messages).unwrap()
}

// C1
#[test]
fn finished_string_input_at_max_tokens_is_resent_once() {
    let events = reply_with_call(json!("a.rs"));
    let provider = ScriptedProvider::new([events.clone(), events]);
    let mut chat = chat();
    let content = render_user_content("Read a", &[]);
    let first = build_request(&chat, content.clone(), &settings());
    chat.begin_turn_with(content, Vec::new(), &settings());
    let before = committed(&mut chat);
    play(&mut chat, &first, &provider);
    assert!(chat.needs_resend());
    assert!(chat.pending_calls().is_empty());
    assert_eq!(committed(&mut chat), before, "never in history");
    let again = round_request(&mut chat, &settings()).unwrap();
    assert_eq!(
        serde_json::to_string(&again).unwrap(),
        serde_json::to_string(&first).unwrap(),
        "the identical request"
    );
    play(&mut chat, &again, &provider);
    assert!(!chat.needs_resend(), "resent once only");
    assert_eq!(committed(&mut chat), before, "never in history");
    assert_eq!(provider.requests().len(), 2);
}

// C2
#[test]
fn an_input_cut_short_at_max_tokens_keeps_finished_text_only() {
    let events = ReplyScript::new("claude-opus-5-5", "msg")
        .text("Reading.")
        .invalid_tool_use("toolu_x", READ_FILE, "{\"path\": \"a.r")
        .stop(StopReason::MaxTokens);
    let provider = ScriptedProvider::new([events]);
    let mut chat = chat();
    let content = render_user_content("Read a", &[]);
    let request = build_request(&chat, content.clone(), &settings());
    chat.begin_turn_with(content, Vec::new(), &settings());
    play(&mut chat, &request, &provider);
    assert!(!chat.needs_resend());
    assert!(chat.pending_calls().is_empty());
    let turn = &chat.turns[0];
    assert_eq!(turn.state, TurnState::Done);
    assert!(turn.results.is_empty());
    let history = serde_json::to_string(&chat.history()).unwrap();
    assert!(history.contains("Reading."));
    assert!(!history.contains("tool_use"), "{history}");
}
