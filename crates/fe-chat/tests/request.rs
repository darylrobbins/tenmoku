use std::task::{Context, Poll};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, ContextItem, ContextKind,
    Conversation, Effort, ErrorKind, ProviderError, Rows, ScriptedProvider, StopReason, TurnState,
    build_request, render_user_content,
};
use futures::StreamExt;
use futures::executor::block_on;
use serde_json::{Value, json};

fn reply(text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: format!("msg_{}", text.len()),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Thinking,
        },
        ChatEvent::ThinkingDelta {
            index: 0,
            text: "thinking".to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "thinking", "thinking": "thinking", "signature": "sig"}),
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

/// What the cache sees, in render order: the system blocks, then each
/// message, serialised.
fn prefix(request: &ChatRequest) -> String {
    let mut bytes = serde_json::to_string(&request.system).unwrap();
    for message in &request.messages {
        bytes.push_str(&serde_json::to_string(message).unwrap());
    }
    bytes
}

/// Sends one turn: builds the request, streams the reply into the chat.
fn send(
    chat: &mut Conversation,
    provider: &ScriptedProvider,
    content: Vec<Value>,
    settings: &ChatSettings,
) -> ChatRequest {
    let request = build_request(chat, content.clone(), settings);
    chat.begin_turn(content, Vec::new());
    let events: Vec<ChatEvent> = block_on(provider.stream(request.clone()).collect());
    for event in events {
        chat.apply(event);
    }
    assert_eq!(chat.turns.last().unwrap().state, TurnState::Done);
    request
}

#[test]
fn consecutive_requests_share_a_byte_identical_prefix() {
    let provider = ScriptedProvider::new([reply("One."), reply("Two."), reply("Three.")]);
    let mut settings = ChatSettings::default();
    let mut chat = Conversation::new(&settings);

    let file = ContextItem {
        kind: ContextKind::File {
            path: "src/main.rs".to_owned(),
            rows: Some(Rows { first: 1, last: 3 }),
        },
        label: "main.rs".to_owned(),
        text: "fn main() {\n    run();\n}\n".to_owned(),
    };
    let first = send(
        &mut chat,
        &provider,
        render_user_content("What does main do?", &[file]),
        &settings,
    );

    // The settings change between turns; the chat keeps its own.
    settings.model = "claude-sonnet-5-5".to_owned();
    settings.effort = Effort::Max;
    settings.show_thinking = true;
    let selection = ContextItem {
        kind: ContextKind::Selection {
            path: "src/run.rs".to_owned(),
            rows: Rows { first: 4, last: 4 },
        },
        label: "run.rs:4".to_owned(),
        text: "    serve();".to_owned(),
    };
    let second = send(
        &mut chat,
        &provider,
        render_user_content("And this line?", &[selection]),
        &settings,
    );
    let third = send(
        &mut chat,
        &provider,
        render_user_content("Thanks.", &[]),
        &settings,
    );

    let requests = [first, second, third];
    assert_eq!(provider.requests(), requests);
    for pair in requests.windows(2) {
        let (earlier, later) = (prefix(&pair[0]), prefix(&pair[1]));
        assert!(
            later.starts_with(&earlier),
            "not a prefix:\n{earlier}\n\n{later}"
        );
        assert!(later.len() > earlier.len());
    }
    for request in &requests {
        assert_eq!(request.system.len(), 1);
        let system = serde_json::to_value(&request.system[0]).unwrap();
        assert_eq!(system["type"], "text");
        assert_eq!(system["cache_control"], json!({"type": "ephemeral"}));
        assert!(request.tools.is_empty());
        assert_eq!(request.model, "claude-opus-5-5");
        assert_eq!(request.effort, Effort::Medium);
        assert!(!request.show_thinking);
        assert_eq!(request.max_tokens, 64_000);
    }
    assert_eq!(requests[2].messages.len(), 5);
    assert_eq!(
        serde_json::to_value(&requests[2].messages[1]).unwrap()["content"][0]["signature"],
        "sig"
    );
}

#[test]
fn user_content_tags_each_item_then_asks() {
    let items = [
        ContextItem {
            kind: ContextKind::File {
                path: "a \"b\".rs".to_owned(),
                rows: None,
            },
            label: "a.rs".to_owned(),
            text: "x\n".to_owned(),
        },
        ContextItem {
            kind: ContextKind::Selection {
                path: "c.rs".to_owned(),
                rows: Rows { first: 2, last: 5 },
            },
            label: "c.rs:2-5".to_owned(),
            text: "y".to_owned(),
        },
        ContextItem {
            kind: ContextKind::Diagnostics,
            label: "Problems".to_owned(),
            text: "c.rs:3 error: nope".to_owned(),
        },
        ContextItem {
            kind: ContextKind::OpenEditors,
            label: "Open editors".to_owned(),
            text: "a.rs\nc.rs".to_owned(),
        },
    ];
    let content = render_user_content("Why?", &items);
    let texts: Vec<&str> = content
        .iter()
        .map(|block| {
            assert_eq!(block["type"], "text");
            block["text"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        texts,
        [
            "<file path=\"a &quot;b&quot;.rs\">\nx\n</file>",
            "<selection path=\"c.rs\" rows=\"2-5\">\ny\n</selection>",
            "<diagnostics>\nc.rs:3 error: nope\n</diagnostics>",
            "<open-editors>\na.rs\nc.rs\n</open-editors>",
            "Why?",
        ]
    );
    // Keys in sorted order, so the bytes don't depend on `preserve_order`.
    assert_eq!(
        serde_json::to_string(&content[4]).unwrap(),
        r#"{"text":"Why?","type":"text"}"#
    );
}

#[test]
fn the_scripted_provider_records_requests_and_stops_when_dropped() {
    let failure = ProviderError::new(ErrorKind::Overloaded, "busy");
    let provider = ScriptedProvider::new([
        reply("First."),
        reply("Second."),
        vec![ChatEvent::Failed(failure.clone())],
    ]);
    assert_eq!(provider.name(), "scripted");
    let chat = Conversation::new(&ChatSettings::default());
    let request = |question: &str| {
        build_request(
            &chat,
            render_user_content(question, &[]),
            &ChatSettings::default(),
        )
    };

    // The first reply plays in full, yielding between events.
    let mut stream = provider.stream(request("one"));
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    assert!(stream.poll_next_unpin(&mut cx).is_pending());
    let mut events = Vec::new();
    loop {
        match stream.poll_next_unpin(&mut cx) {
            Poll::Ready(Some(event)) => {
                events.push(event);
                assert!(stream.poll_next_unpin(&mut cx).is_pending() || events.len() == 8);
            }
            Poll::Ready(None) => break,
            Poll::Pending => {}
        }
    }
    assert_eq!(events, reply("First."));
    drop(stream);
    assert_eq!(provider.dropped(), 0);

    // The second is dropped after two events: it counts, and its rest is
    // never played.
    let mut stream = provider.stream(request("two"));
    let taken: Vec<ChatEvent> = block_on((&mut stream).take(2).collect());
    assert_eq!(taken, reply("Second.")[..2]);
    drop(stream);
    assert_eq!(provider.dropped(), 1);

    // The next stream plays the next reply, a failure, not the rest of the
    // dropped one.
    let events: Vec<ChatEvent> = block_on(provider.stream(request("three")).collect());
    assert_eq!(events, [ChatEvent::Failed(failure)]);

    // Out of replies: it fails.
    let events: Vec<ChatEvent> = block_on(provider.stream(request("four")).collect());
    assert!(
        matches!(&events[..], [ChatEvent::Failed(error)] if error.kind == ErrorKind::InvalidRequest)
    );

    let asked: Vec<Value> = provider
        .requests()
        .iter()
        .map(|request| request.messages[0].content[0]["text"].clone())
        .collect();
    assert_eq!(asked, ["one", "two", "three", "four"]);
    assert_eq!(provider.dropped(), 1);
}
