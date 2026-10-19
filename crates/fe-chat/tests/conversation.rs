use fe_chat::{
    ApiMessage, BlockKind, ChatEvent, ChatId, ChatSettings, Conversation, ErrorKind, ProviderError,
    Role, StopReason, TurnState, Usage,
};
use serde_json::{Value, json};

fn user(text: &str) -> Vec<Value> {
    vec![json!({"type": "text", "text": text})]
}

fn text(index: usize, text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::BlockStarted {
            index,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index,
            text: text.to_owned(),
        },
        ChatEvent::BlockFinished {
            index,
            block: json!({"type": "text", "text": text}),
        },
    ]
}

fn finished(index: usize, block: Value) -> ChatEvent {
    ChatEvent::BlockFinished { index, block }
}

fn end(stop_reason: StopReason) -> ChatEvent {
    ChatEvent::Finished {
        stop_reason,
        stop_details: None,
    }
}

fn play(conversation: &mut Conversation, events: impl IntoIterator<Item = ChatEvent>) {
    for event in events {
        conversation.apply(event);
    }
}

#[test]
fn a_streamed_reply_is_rebuilt_verbatim() {
    let mut chat = Conversation::new(&ChatSettings::default());
    chat.begin_turn(user("What does main do?"), Vec::new());

    let thinking = json!({
        "type": "thinking",
        "thinking": "Look at main. It parses args.",
        "signature": "EqQBCkYIBxgCKkDsig==",
    });
    let first = json!({"type": "text", "text": "It parses the arguments, "});
    let second = json!({"type": "text", "text": "then runs the app."});
    play(
        &mut chat,
        [
            ChatEvent::Started {
                model: "claude-fallback-1".to_owned(),
                message_id: "msg_01".to_owned(),
            },
            ChatEvent::BlockStarted {
                index: 0,
                kind: BlockKind::Thinking,
            },
            ChatEvent::ThinkingDelta {
                index: 0,
                text: "Look at main. ".to_owned(),
            },
            ChatEvent::ThinkingDelta {
                index: 0,
                text: "It parses args.".to_owned(),
            },
            finished(0, thinking.clone()),
            ChatEvent::BlockStarted {
                index: 1,
                kind: BlockKind::Text,
            },
            ChatEvent::TextDelta {
                index: 1,
                text: "It parses ".to_owned(),
            },
            ChatEvent::TextDelta {
                index: 1,
                text: "the arguments, ".to_owned(),
            },
            finished(1, first.clone()),
            ChatEvent::BlockStarted {
                index: 2,
                kind: BlockKind::Text,
            },
            ChatEvent::TextDelta {
                index: 2,
                text: "then runs ".to_owned(),
            },
            ChatEvent::TextDelta {
                index: 2,
                text: "the app.".to_owned(),
            },
            finished(2, second.clone()),
            ChatEvent::Usage(Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: 600,
                cache_read_input_tokens: 0,
            }),
            end(StopReason::EndTurn),
        ],
    );

    let turn = &chat.turns[0];
    assert_eq!(turn.state, TurnState::Done);
    assert_eq!(
        turn.reply_blocks().cloned().collect::<Vec<_>>(),
        vec![thinking, first, second]
    );
    assert_eq!(turn.text(), "It parses the arguments, then runs the app.");
    assert_eq!(turn.reply[1].text, "It parses the arguments, ");
    assert_eq!(turn.reply[2].text, "then runs the app.");
    assert_eq!(turn.thinking(), "Look at main. It parses args.");
    assert_eq!(turn.model.as_deref(), Some("claude-fallback-1"));
    assert_eq!(turn.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(turn.usage.map(|usage| usage.total_input()), Some(610));
    assert_eq!(chat.title, "What does main do?");

    // Events after the turn ended change nothing.
    let before = chat.clone();
    chat.apply(ChatEvent::TextDelta {
        index: 2,
        text: "late".to_owned(),
    });
    assert_eq!(chat, before);
}

#[test]
fn history_follows_the_append_only_rules() {
    let mut chat = Conversation::new(&ChatSettings::default());
    let thinking = json!({"type": "thinking", "thinking": "hm", "signature": "sig"});

    // Done: user content and reply, thinking included, unchanged.
    let asked = vec![
        json!({"type": "text", "text": "<file path=\"a.rs\">\nfn a() {}\n</file>"}),
        json!({"type": "text", "text": "Explain a"}),
    ];
    chat.begin_turn(asked.clone(), Vec::new());
    play(&mut chat, [finished(0, thinking.clone())]);
    play(&mut chat, text(1, "It does nothing."));
    play(&mut chat, [end(StopReason::EndTurn)]);
    let done = vec![
        ApiMessage {
            role: Role::User,
            content: asked,
        },
        ApiMessage {
            role: Role::Assistant,
            content: vec![
                thinking.clone(),
                json!({"type": "text", "text": "It does nothing."}),
            ],
        },
    ];
    assert_eq!(chat.history(), done);

    // Streaming: not in the history yet.
    chat.begin_turn(user("And b?"), Vec::new());
    play(&mut chat, [finished(0, thinking.clone())]);
    play(&mut chat, text(1, "b is"));
    assert_eq!(chat.history(), done);

    // Stopped: user content and the finished text blocks only; the
    // unfinished block and the thinking are left out.
    play(
        &mut chat,
        [
            ChatEvent::BlockStarted {
                index: 2,
                kind: BlockKind::Text,
            },
            ChatEvent::TextDelta {
                index: 2,
                text: "unfinished".to_owned(),
            },
        ],
    );
    chat.stop();
    assert_eq!(chat.turns[1].state, TurnState::Stopped);
    let mut expected = done.clone();
    expected.push(ApiMessage {
        role: Role::User,
        content: user("And b?"),
    });
    expected.push(ApiMessage {
        role: Role::Assistant,
        content: vec![json!({"type": "text", "text": "b is"})],
    });
    assert_eq!(chat.history(), expected);

    // Stopped with no finished text: left out.
    chat.begin_turn(user("And c?"), Vec::new());
    play(&mut chat, [finished(0, thinking.clone())]);
    chat.stop();
    assert_eq!(chat.history(), expected);

    // Refused, before or mid-stream: left out.
    chat.begin_turn(user("Something declined"), Vec::new());
    play(&mut chat, text(0, "Sure, here"));
    play(&mut chat, [end(StopReason::Refusal)]);
    assert_eq!(chat.turns[3].state, TurnState::Refused);
    chat.begin_turn(user("Declined at once"), Vec::new());
    play(&mut chat, [end(StopReason::Refusal)]);
    assert_eq!(chat.history(), expected);

    // Failed: left out.
    let error = ProviderError::new(ErrorKind::Stream, "connection reset");
    chat.begin_turn(user("Cut off"), Vec::new());
    play(&mut chat, text(0, "partial"));
    play(&mut chat, [ChatEvent::Failed(error.clone())]);
    assert_eq!(chat.turns[5].state, TurnState::Failed(error));
    assert_eq!(chat.history(), expected);

    // A max_tokens reply is kept, marked cut off.
    chat.begin_turn(user("Long one"), Vec::new());
    play(&mut chat, text(0, "very long"));
    play(&mut chat, [end(StopReason::MaxTokens)]);
    assert!(chat.turns[6].cut_off());
    expected.push(ApiMessage {
        role: Role::User,
        content: user("Long one"),
    });
    expected.push(ApiMessage {
        role: Role::Assistant,
        content: vec![json!({"type": "text", "text": "very long"})],
    });
    assert_eq!(chat.history(), expected);

    // A mid-output fallback: thinking, redacted thinking and tool calls
    // before the last fallback block are left out; text before it and
    // everything after it stays.
    let redacted = json!({"type": "redacted_thinking", "data": "opaque"});
    let tool = json!({"type": "tool_use", "id": "toolu_1", "name": "read", "input": {}});
    let fallback = |n: u32| json!({"type": "fallback", "from": "a", "to": format!("b{n}")});
    let early = json!({"type": "text", "text": "Before."});
    let late_thinking = json!({"type": "thinking", "thinking": "again", "signature": "s2"});
    let late = json!({"type": "text", "text": "After."});
    chat.begin_turn(user("Risky"), Vec::new());
    play(
        &mut chat,
        [
            finished(0, thinking.clone()),
            finished(1, redacted),
            finished(2, early.clone()),
            finished(3, tool),
            finished(4, fallback(1)),
            finished(5, thinking.clone()),
            finished(6, fallback(2)),
            finished(7, late_thinking.clone()),
            finished(8, late.clone()),
            end(StopReason::EndTurn),
        ],
    );
    let history = chat.history();
    assert_eq!(
        history.last().map(|message| &message.content),
        Some(&vec![early, fallback(1), fallback(2), late_thinking, late])
    );
    assert_eq!(history[..expected.len()], expected[..]);
}

#[test]
fn a_conversation_round_trips_and_a_streaming_turn_saves_as_stopped() {
    let mut chat = Conversation::new(&ChatSettings::default());
    chat.begin_turn(user("First"), Vec::new());
    play(&mut chat, text(0, "One"));
    play(&mut chat, [end(StopReason::EndTurn)]);
    chat.begin_turn(user("Second"), Vec::new());
    play(&mut chat, text(0, "Tw"));

    let saved = serde_json::to_string(&chat).unwrap();
    let read: Conversation = serde_json::from_str(&saved).unwrap();
    assert_eq!(read.id, chat.id);
    assert_eq!(read.turns[0], chat.turns[0]);
    assert_eq!(read.turns[1].state, TurnState::Stopped);
    chat.stop();
    assert_eq!(read, chat);
    assert_eq!(read.history(), chat.history());
}

#[test]
fn the_title_is_the_first_line_cut_at_a_char_boundary() {
    let mut chat = Conversation::new(&ChatSettings::default());
    let question = format!("\n  {}é and more\nsecond line", "a".repeat(58));
    chat.begin_turn(
        vec![
            json!({"type": "text", "text": "<file path=\"x\">\n</file>"}),
            json!({"type": "text", "text": question}),
        ],
        Vec::new(),
    );
    assert_eq!(chat.title, format!("{}é ", "a".repeat(58)));
    chat.begin_turn(user("Later"), Vec::new());
    assert_eq!(chat.title.chars().count(), 60);
}

#[test]
fn chat_ids_print_as_26_base32_chars_and_parse_back() {
    let id = ChatId::new();
    let printed = id.to_string();
    assert_eq!(printed.len(), 26);
    assert_eq!(printed.parse::<ChatId>(), Ok(id));
    assert_ne!(ChatId::new(), id);
    assert_eq!(
        ChatId::from_u128(u128::MAX).to_string(),
        "7zzzzzzzzzzzzzzzzzzzzzzzzz"
    );
    assert_eq!(ChatId::from_u128(0).to_string(), "0".repeat(26));
    assert!("8zzzzzzzzzzzzzzzzzzzzzzzzz".parse::<ChatId>().is_err());
    assert!("too short".parse::<ChatId>().is_err());
}
