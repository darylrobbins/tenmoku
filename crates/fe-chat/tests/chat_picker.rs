//! The chat's model and effort picks (record § 4): an empty chat takes them
//! as its own; a chat with turns asks for them from the next send, its
//! earlier turns going back unchanged, and an effort change on a model that
//! takes one goes through an effort-only system message, the top-level
//! effort staying the chat's first.

use fe_chat::{
    ApiMessage, BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, Conversation,
    Effort, Role, ScriptedProvider, StopReason, TurnState, build_request, render_user_content,
};
use futures::StreamExt;
use futures::executor::block_on;
use serde_json::{Value, json};

/// The models that take an effort message here, as the app's list does.
const EFFORT_MESSAGE_MODELS: &[&str] = &["claude-opus-5-5", "claude-sonnet-5-5"];

fn settings() -> ChatSettings {
    ChatSettings {
        effort_message_models: EFFORT_MESSAGE_MODELS,
        ..ChatSettings::default()
    }
}

/// A finished reply of `model`'s: a thinking block, then `text`.
fn reply(model: &str, text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: model.to_owned(),
            message_id: format!("msg_{text}"),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Thinking,
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "thinking", "thinking": "", "signature": format!("sig-{model}")}),
        },
        ChatEvent::BlockStarted {
            index: 1,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index: 1,
            text: text.to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 1,
            block: json!({"type": "text", "text": text}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

/// Sends `question` as the view does: builds the request, begins the turn
/// and streams the reply into it. Returns the request sent.
fn send(
    chat: &mut Conversation,
    provider: &ScriptedProvider,
    question: &str,
    settings: &ChatSettings,
) -> ChatRequest {
    let content = render_user_content(question, &[]);
    let request = build_request(chat, content.clone(), settings);
    chat.begin_turn(content, Vec::new());
    let events: Vec<ChatEvent> = block_on(provider.stream(request.clone()).collect());
    for event in events {
        chat.apply(event);
    }
    assert_eq!(chat.turns.last().unwrap().state, TurnState::Done);
    request
}

fn user(question: &str) -> ApiMessage {
    ApiMessage {
        role: Role::User,
        content: render_user_content(question, &[]),
    }
}

/// The messages of `request`, serialised, in order: what the cache sees.
fn serialised(messages: &[ApiMessage]) -> Vec<String> {
    (messages.iter())
        .map(|message| serde_json::to_string(message).unwrap())
        .collect()
}

/// Whether `later`'s messages begin with all of `earlier`'s, byte for byte.
fn extends(later: &ChatRequest, earlier: &ChatRequest) -> bool {
    let (later, earlier) = (serialised(&later.messages), serialised(&earlier.messages));
    later.len() > earlier.len() && later[..earlier.len()] == earlier[..]
}

#[test]
fn picker_empty_chat_takes_picked_model_and_effort() {
    let settings = settings();
    let mut chat = Conversation::new(&settings);
    assert_eq!(
        (chat.model.as_str(), chat.effort),
        ("claude-opus-5-5", Effort::Medium)
    );

    chat.set_model("claude-sonnet-5-5");
    chat.set_effort(Effort::High);
    assert_eq!(
        (chat.model.as_str(), chat.effort),
        ("claude-sonnet-5-5", Effort::High)
    );

    let provider = ScriptedProvider::new([reply("claude-sonnet-5-5", "Hi.")]);
    let request = send(&mut chat, &provider, "Hello?", &settings);
    assert_eq!(request.model, "claude-sonnet-5-5");
    assert_eq!(
        request.effort,
        Effort::High,
        "the first effort is top-level"
    );
    assert_eq!(request.messages, [user("Hello?")], "no effort message");
    let turn = &chat.turns[0];
    assert_eq!(turn.asked_model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(turn.asked_effort, Some(Effort::High));
    assert_eq!(chat.first_effort(), Effort::High);
}

#[test]
fn picker_model_switch_applies_next_turn_history_unchanged() {
    let settings = settings();
    let mut chat = Conversation::new(&settings);
    let provider = ScriptedProvider::new([
        reply("claude-opus-5-5", "One."),
        reply("claude-opus-5-5", "Two."),
        reply("claude-haiku-4-5", "Three."),
        reply("claude-opus-5-5", "Four."),
    ]);
    send(&mut chat, &provider, "First?", &settings);
    let second = send(&mut chat, &provider, "Second?", &settings);
    let history = chat.history();

    // Picked between turns: nothing earlier changes.
    chat.set_model("claude-haiku-4-5");
    assert_eq!(chat.history(), history, "the history is as it was");
    assert_eq!(
        chat.turns[1].asked_model.as_deref(),
        Some("claude-opus-5-5")
    );

    let third = send(&mut chat, &provider, "Third?", &settings);
    assert_eq!(third.model, "claude-haiku-4-5", "the next request names it");
    assert_eq!(third.effort, Effort::Medium);
    // Earlier turns go back verbatim, the other model's thinking included.
    assert_eq!(third.messages[..history.len()], history[..]);
    let thinking: Vec<&Value> = (third.messages.iter())
        .flat_map(|message| &message.content)
        .filter(|block| block["type"] == "thinking")
        .collect();
    assert_eq!(thinking.len(), 2, "{thinking:?}");
    assert!(
        thinking
            .iter()
            .all(|block| block["signature"] == "sig-claude-opus-5-5")
    );
    assert_eq!(third.messages.last(), Some(&user("Third?")));
    assert!(extends(&third, &second));
    assert_eq!(
        chat.turns[2].asked_model.as_deref(),
        Some("claude-haiku-4-5")
    );
    assert_eq!(chat.turns[2].model.as_deref(), Some("claude-haiku-4-5"));

    // And back: the same history again, the haiku turn included.
    chat.set_model("claude-opus-5-5");
    let fourth = send(&mut chat, &provider, "Fourth?", &settings);
    assert_eq!(fourth.model, "claude-opus-5-5");
    assert!(extends(&fourth, &third));
    assert!(fourth.messages.iter().all(|m| m.effort_change().is_none()));
}

#[test]
fn picker_effort_change_appends_effort_message_top_level_unchanged() {
    let settings = settings();
    let mut chat = Conversation::new(&settings);
    let provider = ScriptedProvider::new([
        reply("claude-opus-5-5", "One."),
        reply("claude-opus-5-5", "Two."),
        reply("claude-opus-5-5", "Three."),
        reply("claude-opus-5-5", "Four."),
        reply("claude-haiku-4-5", "Five."),
    ]);
    let first = send(&mut chat, &provider, "First?", &settings);
    assert_eq!(first.effort, Effort::Medium);

    // Raised for the next turn: an effort message before it, the top-level
    // effort the chat's first, so the prefix holds.
    chat.set_effort(Effort::High);
    let second = send(&mut chat, &provider, "Second?", &settings);
    assert_eq!(second.effort, Effort::Medium, "top-level unchanged");
    assert_eq!(second.model, "claude-opus-5-5");
    let effort_at = second.messages.len() - 2;
    assert_eq!(second.messages[effort_at], ApiMessage::effort(Effort::High));
    assert_eq!(second.messages[effort_at].content, Vec::<Value>::new());
    assert_eq!(second.messages.last(), Some(&user("Second?")));
    assert!(extends(&second, &first));
    assert_eq!(chat.turns[1].asked_effort, Some(Effort::High));
    assert_eq!(chat.turns[0].asked_effort, Some(Effort::Medium));

    // Kept: the next request replays the message where it was, and adds
    // none.
    let third = send(&mut chat, &provider, "Third?", &settings);
    assert_eq!(third.effort, Effort::Medium);
    assert!(extends(&third, &second));
    let changes: Vec<Effort> = third
        .messages
        .iter()
        .filter_map(ApiMessage::effort_change)
        .collect();
    assert_eq!(changes, [Effort::High]);

    // Back to the first: another message, still appended.
    chat.set_effort(Effort::Medium);
    let fourth = send(&mut chat, &provider, "Fourth?", &settings);
    assert_eq!(fourth.effort, Effort::Medium);
    assert!(extends(&fourth, &third));
    let changes: Vec<Effort> = fourth
        .messages
        .iter()
        .filter_map(ApiMessage::effort_change)
        .collect();
    assert_eq!(changes, [Effort::High, Effort::Medium]);

    // A model that doesn't take the message: the top-level effort changes
    // instead, and no effort message is sent.
    chat.set_model("claude-haiku-4-5");
    chat.set_effort(Effort::Low);
    let fifth = send(&mut chat, &provider, "Fifth?", &settings);
    assert_eq!(fifth.effort, Effort::Low);
    assert!(fifth.messages.iter().all(|m| m.effort_change().is_none()));
    assert_eq!(fifth.messages, {
        let mut plain = chat.history()[..8].to_vec();
        plain.push(user("Fifth?"));
        plain
    });
}

#[test]
fn picker_old_chat_file_without_asked_fields_loads() {
    let settings = settings();
    let mut chat = Conversation::new(&settings);
    chat.set_effort(Effort::Low);
    let provider = ScriptedProvider::new([
        reply("claude-opus-5-5", "One."),
        reply("claude-opus-5-5", "Two."),
        reply("claude-opus-5-5", "Three."),
        reply("claude-opus-5-5", "Four."),
    ]);
    send(&mut chat, &provider, "First?", &settings);
    send(&mut chat, &provider, "Second?", &settings);

    // A file written before the picker: its turns say nothing of what was
    // asked.
    let mut saved = serde_json::to_value(&chat).unwrap();
    for turn in saved["turns"].as_array_mut().unwrap() {
        let turn = turn.as_object_mut().unwrap();
        assert!(turn.remove("asked_model").is_some());
        assert!(turn.remove("asked_effort").is_some());
    }
    let mut old: Conversation = serde_json::from_value(saved).unwrap();
    assert!(old.turns.iter().all(|turn| turn.asked_model.is_none()));
    assert!(old.turns.iter().all(|turn| turn.asked_effort.is_none()));
    assert_eq!(old.history(), chat.history());

    // It replays the same messages as the chat it was saved from.
    let next = render_user_content("Third?", &[]);
    assert_eq!(
        build_request(&old, next.clone(), &settings),
        build_request(&chat, next.clone(), &settings)
    );

    // A pick then keeps what its turns asked: the chat's own then.
    old.set_model("claude-sonnet-5-5");
    old.set_effort(Effort::Max);
    assert!(old.turns.iter().all(|turn| {
        turn.asked_model.as_deref() == Some("claude-opus-5-5")
            && turn.asked_effort == Some(Effort::Low)
    }));
    let request = build_request(&old, next, &settings);
    assert_eq!(request.model, "claude-sonnet-5-5");
    assert_eq!(request.effort, Effort::Low, "the chat's first effort");
    let changes: Vec<Effort> = request
        .messages
        .iter()
        .filter_map(ApiMessage::effort_change)
        .collect();
    assert_eq!(changes, [Effort::Max]);
    assert_eq!(request.messages[..4], old.history()[..]);
}
