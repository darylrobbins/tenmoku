use std::collections::VecDeque;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use fe_chat::{
    ApiMessage, BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, Conversation,
    ErrorKind, ProviderError, Role, StopReason, ToolSpec, TurnState, Usage, build_request,
    render_user_content,
};
use fe_chat_anthropic::{
    AnthropicProvider, ApiKey, Body, Response, Transport, TransportError, error_from_response,
};
use futures::executor::block_on;
use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use http::HeaderMap;
use serde_json::{Value, json};

const KEY: &str = "sk-ant-api03-secret-test-key";

/// One scripted answer to a `post`.
enum Reply {
    Response {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        chunks: Vec<Result<Vec<u8>, TransportError>>,
        /// Whether the body stays open after its chunks, as a live reply does.
        then_hang: bool,
    },
    Fail(TransportError),
}

impl Reply {
    /// A 200 replaying `fixture` in 7-byte chunks.
    fn fixture(fixture: &str) -> Reply {
        Reply::ok(chunks(&read_fixture(fixture), 7))
    }

    fn ok(chunks: Vec<Result<Vec<u8>, TransportError>>) -> Reply {
        Reply::Response {
            status: 200,
            headers: vec![("content-type", "text/event-stream")],
            chunks,
            then_hang: false,
        }
    }

    fn error(status: u16, error_type: &str, message: &str) -> Reply {
        let body = json!({
            "type": "error",
            "error": {"type": error_type, "message": message},
            "request_id": "req_011Test",
        });
        Reply::Response {
            status,
            headers: vec![("content-type", "application/json")],
            chunks: vec![Ok(body.to_string().into_bytes())],
            then_hang: false,
        }
    }
}

/// What a `post` was given.
struct Sent {
    url: String,
    headers: Vec<(String, String)>,
    body: Value,
}

impl Sent {
    fn header(&self, name: &str) -> Option<&str> {
        let mut values = self.headers.iter().filter(|(key, _)| key == name);
        let value = values.next().map(|(_, value)| value.as_str());
        assert!(values.next().is_none(), "one {name} header");
        value
    }
}

/// Replays scripted replies and records the requests.
#[derive(Default)]
struct FakeTransport {
    replies: Mutex<VecDeque<Reply>>,
    sent: Mutex<Vec<Sent>>,
    /// How many response bodies were dropped.
    bodies_dropped: Arc<AtomicUsize>,
}

impl FakeTransport {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Arc<FakeTransport> {
        Arc::new(FakeTransport {
            replies: Mutex::new(replies.into_iter().collect()),
            ..FakeTransport::default()
        })
    }

    fn sent(&self) -> usize {
        self.sent.lock().unwrap().len()
    }

    fn bodies_dropped(&self) -> usize {
        self.bodies_dropped.load(Ordering::SeqCst)
    }
}

impl Transport for FakeTransport {
    fn post(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        self.sent.lock().unwrap().push(Sent {
            url: url.to_owned(),
            headers,
            body: serde_json::from_slice(&body).expect("the body is JSON"),
        });
        let reply = self.replies.lock().unwrap().pop_front();
        let dropped = Arc::clone(&self.bodies_dropped);
        Box::pin(async move {
            match reply.expect("a scripted reply is left") {
                Reply::Fail(error) => Err(error),
                Reply::Response {
                    status,
                    headers,
                    chunks,
                    then_hang,
                } => {
                    let mut map = HeaderMap::new();
                    for (name, value) in headers {
                        map.insert(name, value.parse().unwrap());
                    }
                    let chunks = futures::stream::iter(chunks);
                    let body: Body = if then_hang {
                        Box::pin(chunks.chain(futures::stream::pending()))
                    } else {
                        Box::pin(chunks)
                    };
                    Ok(Response {
                        status,
                        headers: map,
                        body: Box::pin(Tracked { body, dropped }),
                    })
                }
            }
        })
    }
}

/// A body that counts its drop.
struct Tracked {
    body: Body,
    dropped: Arc<AtomicUsize>,
}

impl Stream for Tracked {
    type Item = Result<Vec<u8>, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.body.poll_next_unpin(cx)
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// A provider over `transport` whose waits return at once, and the waits
/// it was asked for.
fn fake_provider(transport: &Arc<FakeTransport>) -> (AnthropicProvider, Arc<Mutex<Vec<Duration>>>) {
    let waits = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&waits);
    let transport: Arc<dyn Transport> = transport.clone();
    let provider = AnthropicProvider::new(transport, ApiKey::new(KEY), "https://api.test/")
        .with_sleep(move |duration| {
            recorded.lock().unwrap().push(duration);
            Box::pin(async {})
        });
    (provider, waits)
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(path).expect("fixture")
}

fn chunks(bytes: &[u8], size: usize) -> Vec<Result<Vec<u8>, TransportError>> {
    bytes.chunks(size).map(|chunk| Ok(chunk.to_vec())).collect()
}

fn request(text: &str) -> ChatRequest {
    let chat = Conversation::new(&ChatSettings::default());
    build_request(
        &chat,
        render_user_content(text, &[]),
        &ChatSettings::default(),
    )
}

fn run(provider: &AnthropicProvider, request: ChatRequest) -> Vec<ChatEvent> {
    block_on(provider.stream(request).collect())
}

fn started(model: &str, message_id: &str) -> ChatEvent {
    ChatEvent::Started {
        model: model.to_owned(),
        message_id: message_id.to_owned(),
    }
}

fn usage(input: u64, output: u64, cache_creation: u64, cache_read: u64) -> ChatEvent {
    ChatEvent::Usage(Usage {
        input_tokens: input,
        output_tokens: output,
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
    })
}

fn block_started(index: usize, kind: BlockKind) -> ChatEvent {
    ChatEvent::BlockStarted { index, kind }
}

fn text(index: usize, text: &str) -> ChatEvent {
    ChatEvent::TextDelta {
        index,
        text: text.to_owned(),
    }
}

fn thinking(index: usize, text: &str) -> ChatEvent {
    ChatEvent::ThinkingDelta {
        index,
        text: text.to_owned(),
    }
}

fn tool_input(index: usize, partial_json: &str) -> ChatEvent {
    ChatEvent::ToolInputDelta {
        index,
        partial_json: partial_json.to_owned(),
    }
}

fn finished_block(index: usize, block: Value) -> ChatEvent {
    ChatEvent::BlockFinished { index, block }
}

fn finished(stop_reason: StopReason, stop_details: Option<Value>) -> ChatEvent {
    ChatEvent::Finished {
        stop_reason,
        stop_details,
    }
}

fn failed(kind: ErrorKind, message: &str) -> ChatEvent {
    ChatEvent::Failed(ProviderError::new(kind, message))
}

#[test]
fn the_request_matches_the_record() {
    // A chat's second turn: the first reply comes from the text fixture
    // through the provider, so the history holds it verbatim.
    let transport = FakeTransport::new([
        Reply::fixture("text.sse"),
        Reply::fixture("text.sse"),
        Reply::fixture("text.sse"),
    ]);
    let (provider, _) = fake_provider(&transport);
    let settings = ChatSettings::default();
    let mut chat = Conversation::new(&settings);
    let first = render_user_content("What is the answer?", &[]);
    let first_request = build_request(&chat, first.clone(), &settings);
    chat.begin_turn(first.clone(), Vec::new());
    for event in run(&provider, first_request) {
        chat.apply(event);
    }
    assert_eq!(chat.turns.last().unwrap().state, TurnState::Done);
    let second = render_user_content("And why?", &[]);
    let second_request = build_request(&chat, second.clone(), &settings);
    run(&provider, second_request.clone());

    let sent = transport.sent.lock().unwrap();
    let body = &sent[1].body;
    assert_eq!(sent[1].url, "https://api.test/v1/messages");
    let mut keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "cache_control",
            "fallbacks",
            "max_tokens",
            "messages",
            "model",
            "output_config",
            "stream",
            "system",
        ],
        "no thinking, sampling, tool_choice or tools"
    );
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["max_tokens"], 64_000);
    assert_eq!(body["stream"], true);
    assert_eq!(body["output_config"], json!({"effort": "medium"}));
    assert_eq!(body["fallbacks"], "default");
    assert_eq!(body["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(
        body["system"],
        json!([{
            "type": "text",
            "text": fe_chat::SYSTEM_PROMPT,
            "cache_control": {"type": "ephemeral"},
        }])
    );
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": first},
            {"role": "assistant", "content": [{"type": "text", "text": "Café — the answer is 42 ✓"}]},
            {"role": "user", "content": second},
        ]),
        "the history verbatim, ending with the user's message"
    );

    let first_headers = &sent[0];
    for sent in [first_headers, &sent[1]] {
        assert_eq!(sent.header("x-api-key"), Some(KEY));
        assert_eq!(sent.header("anthropic-version"), Some("2023-06-01"));
        assert_eq!(sent.header("content-type"), Some("application/json"));
        assert_eq!(
            sent.header("anthropic-beta"),
            Some("server-side-fallback-2026-07-01")
        );
        let agent = sent.header("user-agent").unwrap();
        assert_eq!(agent, format!("tenmoku/{}", env!("CARGO_PKG_VERSION")));
    }
    drop(sent);

    // Showing thinking asks for summarized adaptive thinking; turning
    // fallbacks off drops the parameter and its beta.
    let mut thinking = second_request;
    thinking.show_thinking = true;
    let transport = FakeTransport::new([Reply::fixture("thinking.sse")]);
    let (provider, _) = fake_provider(&transport);
    let provider = provider.with_fallbacks(false);
    run(&provider, thinking);
    let sent = transport.sent.lock().unwrap();
    assert_eq!(
        sent[0].body["thinking"],
        json!({"type": "adaptive", "display": "summarized"})
    );
    assert!(sent[0].body.get("fallbacks").is_none());
    assert_eq!(sent[0].header("anthropic-beta"), None);
    for absent in ["temperature", "top_p", "top_k", "tool_choice", "tools"] {
        assert!(sent[0].body.get(absent).is_none(), "no {absent}");
    }
    drop(sent);

    // Tools, for the agent loop, go with `strict` and eager input streaming.
    let mut with_tools = request("Read lib.rs");
    with_tools.tools = vec![ToolSpec {
        name: "read_file".to_owned(),
        description: "Reads a file of the project.".to_owned(),
        input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        strict: true,
        eager_input_streaming: true,
    }];
    let transport = FakeTransport::new([Reply::fixture("tool_use.sse")]);
    let (provider, _) = fake_provider(&transport);
    run(&provider, with_tools);
    let sent = transport.sent.lock().unwrap();
    assert_eq!(
        sent[0].body["tools"],
        json!([{
            "name": "read_file",
            "description": "Reads a file of the project.",
            "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}},
            "strict": true,
            "eager_input_streaming": true,
        }])
    );
    assert!(sent[0].body.get("tool_choice").is_none());
    drop(sent);

    // An assistant message last (a prefill, a 400) fails before sending.
    let mut prefill = request("Hi");
    prefill.messages.push(ApiMessage {
        role: Role::Assistant,
        content: vec![json!({"type": "text", "text": "Sure"})],
    });
    let transport = FakeTransport::new([]);
    let (provider, _) = fake_provider(&transport);
    let events = run(&provider, prefill);
    assert!(matches!(
        &events[..],
        [ChatEvent::Failed(ProviderError {
            kind: ErrorKind::InvalidRequest,
            ..
        })]
    ));
    assert_eq!(transport.sent(), 0);
}

#[test]
fn fixtures_become_chat_events() {
    let cases: Vec<(&str, Vec<ChatEvent>)> = vec![
        (
            "text.sse",
            vec![
                started("claude-opus-5-5", "msg_01TextReply"),
                usage(1200, 1, 2048, 0),
                block_started(0, BlockKind::Text),
                text(0, "Café — the "),
                text(0, "answer is 42 ✓"),
                finished_block(
                    0,
                    json!({"type": "text", "text": "Café — the answer is 42 ✓"}),
                ),
                usage(1200, 12, 2048, 0),
                finished(StopReason::EndTurn, None),
            ],
        ),
        (
            "thinking.sse",
            vec![
                started("claude-opus-5-5", "msg_01Thinking"),
                usage(900, 3, 0, 2048),
                block_started(0, BlockKind::Thinking),
                thinking(0, "The user asks why the test fails."),
                thinking(0, "\nThe fixture's path is relative to the crate."),
                finished_block(
                    0,
                    json!({
                        "type": "thinking",
                        "thinking": "The user asks why the test fails.\nThe fixture's path is relative to the crate.",
                        "signature": "EqQBCkgIBhABGAIiQL2sig",
                    }),
                ),
                block_started(1, BlockKind::Text),
                text(1, "Use `CARGO_MANIFEST_DIR`."),
                finished_block(
                    1,
                    json!({"type": "text", "text": "Use `CARGO_MANIFEST_DIR`."}),
                ),
                usage(900, 48, 0, 2048),
                finished(StopReason::EndTurn, None),
            ],
        ),
        (
            "max_tokens.sse",
            vec![
                started("claude-opus-5-5", "msg_01MaxTokens"),
                usage(40, 1, 0, 0),
                block_started(0, BlockKind::Text),
                text(0, "First, open the"),
                finished_block(0, json!({"type": "text", "text": "First, open the"})),
                usage(40, 64_000, 0, 0),
                finished(StopReason::MaxTokens, None),
            ],
        ),
        (
            "refusal_pre_output.sse",
            vec![
                started("claude-opus-5-5", "msg_01RefusedEarly"),
                usage(75, 0, 0, 0),
                usage(75, 0, 0, 0),
                finished(
                    StopReason::Refusal,
                    Some(json!({
                        "type": "refusal",
                        "category": "reasoning_extraction",
                        "explanation": "The request asks for the model's internal reasoning.",
                    })),
                ),
            ],
        ),
        (
            "refusal_mid_stream.sse",
            vec![
                started("claude-opus-5-5", "msg_01RefusedLate"),
                usage(310, 1, 0, 0),
                block_started(0, BlockKind::Text),
                text(0, "Here is how the payload"),
                finished_block(
                    0,
                    json!({"type": "text", "text": "Here is how the payload"}),
                ),
                usage(310, 7, 0, 0),
                finished(
                    StopReason::Refusal,
                    Some(json!({"type": "refusal", "category": "cyber"})),
                ),
            ],
        ),
        (
            "fallback.sse",
            vec![
                started("claude-opus-5", "msg_01FellBack"),
                usage(520, 1, 0, 0),
                block_started(
                    0,
                    BlockKind::Fallback {
                        from: "claude-opus-5-5".to_owned(),
                        to: "claude-opus-5".to_owned(),
                    },
                ),
                finished_block(
                    0,
                    json!({
                        "type": "fallback",
                        "from": {"model": "claude-opus-5-5"},
                        "to": {"model": "claude-opus-5"},
                    }),
                ),
                block_started(1, BlockKind::Text),
                text(1, "The scanner checks the header length first."),
                finished_block(
                    1,
                    json!({"type": "text", "text": "The scanner checks the header length first."}),
                ),
                usage(520, 21, 0, 0),
                finished(StopReason::EndTurn, None),
            ],
        ),
        (
            "tool_use.sse",
            vec![
                started("claude-opus-5-5", "msg_01ToolUse"),
                usage(700, 2, 0, 1024),
                block_started(0, BlockKind::Text),
                text(0, "I'll read the file."),
                finished_block(0, json!({"type": "text", "text": "I'll read the file."})),
                block_started(
                    1,
                    BlockKind::ToolUse {
                        id: "toolu_01ReadFile".to_owned(),
                        name: "read_file".to_owned(),
                    },
                ),
                tool_input(1, ""),
                tool_input(1, "{\"path\": \"src/li"),
                tool_input(1, "b.rs\", \"rows\": [1, 40]}"),
                finished_block(
                    1,
                    json!({
                        "type": "tool_use",
                        "id": "toolu_01ReadFile",
                        "name": "read_file",
                        "input": {"path": "src/lib.rs", "rows": [1, 40]},
                    }),
                ),
                usage(700, 35, 0, 1024),
                finished(StopReason::ToolUse, None),
            ],
        ),
        (
            "error_mid_stream.sse",
            vec![
                started("claude-opus-5-5", "msg_01Overloaded"),
                usage(64, 1, 0, 0),
                block_started(0, BlockKind::Text),
                text(0, "Partly"),
                failed(ErrorKind::Stream, "Overloaded"),
            ],
        ),
    ];
    for (fixture, expected) in cases {
        // In 7-byte chunks, and whole.
        let bytes = read_fixture(fixture);
        for reply in [Reply::fixture(fixture), Reply::ok(vec![Ok(bytes)])] {
            let transport = FakeTransport::new([reply]);
            let (provider, waits) = fake_provider(&transport);
            let events = run(&provider, request("Hi"));
            assert_eq!(events, expected, "{fixture}");
            assert_eq!(transport.sent(), 1, "{fixture}: sent once");
            assert!(waits.lock().unwrap().is_empty(), "{fixture}: no retry");
            assert_eq!(
                transport.bodies_dropped(),
                1,
                "{fixture}: the body is dropped"
            );
        }
    }

    // A tool's input that isn't JSON is kept as the raw string.
    let fixture = String::from_utf8(read_fixture("tool_use.sse")).unwrap();
    let broken = fixture.replace("[1, 40]}", "[1, 40");
    let transport = FakeTransport::new([Reply::ok(vec![Ok(broken.into_bytes())])]);
    let (provider, _) = fake_provider(&transport);
    let events = run(&provider, request("Hi"));
    let input = events.iter().find_map(|event| match event {
        ChatEvent::BlockFinished { index: 1, block } => Some(block["input"].clone()),
        _ => None,
    });
    assert_eq!(
        input,
        Some(json!("{\"path\": \"src/lib.rs\", \"rows\": [1, 40"))
    );

    // So is the input of deltas that join to nothing, rather than the
    // start block's `{}`.
    let mut empty = String::new();
    for line in fixture.split_inclusive("\n\n") {
        if !line.contains("src/li") && !line.contains("b.rs") {
            empty.push_str(line);
        }
    }
    let transport = FakeTransport::new([Reply::ok(vec![Ok(empty.into_bytes())])]);
    let (provider, _) = fake_provider(&transport);
    let events = run(&provider, request("Hi"));
    let input = events.iter().find_map(|event| match event {
        ChatEvent::BlockFinished { index: 1, block } => Some(block["input"].clone()),
        _ => None,
    });
    assert_eq!(input, Some(json!("")));

    // Unknown event, block and delta types are passed over or kept, and a
    // ping's or an unknown event's data isn't read.
    let unknown = "event: message_start\n\
        data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":5}}}\n\n\
        event: future_event\ndata: {\"type\":\"future_event\"}\n\n\
        event: ping\ndata:\n\n\
        event: future_event\ndata: not JSON\n\n\
        event: content_block_start\n\
        data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"future_block\",\"x\":1}}\n\n\
        event: content_block_delta\n\
        data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"future_delta\"}}\n\n\
        event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
        event: message_delta\n\
        data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n\
        event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    let transport = FakeTransport::new([Reply::ok(vec![Ok(unknown.as_bytes().to_vec())])]);
    let (provider, _) = fake_provider(&transport);
    assert_eq!(
        run(&provider, request("Hi")),
        [
            started("claude-opus-5-5", "msg_1"),
            usage(5, 0, 0, 0),
            block_started(0, BlockKind::Other("future_block".to_owned())),
            finished_block(0, json!({"type": "future_block", "x": 1})),
            usage(5, 2, 0, 0),
            finished(StopReason::EndTurn, None),
        ]
    );
}

#[test]
fn errors_are_typed_and_only_transient_ones_retry() {
    let jittered = |wait: Duration, base: u64| {
        let base = Duration::from_secs(base);
        wait >= base && wait < base + Duration::from_millis(250)
    };

    // A 429 waits for its retry-after, then succeeds.
    let mut rate_limited = Reply::error(429, "rate_limit_error", "Rate limited");
    if let Reply::Response { headers, .. } = &mut rate_limited {
        headers.push(("retry-after", "3"));
    }
    let transport = FakeTransport::new([rate_limited, Reply::fixture("text.sse")]);
    let (provider, waits) = fake_provider(&transport);
    let events = run(&provider, request("Hi"));
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(3)]);
    assert_eq!(transport.sent(), 2);
    assert_eq!(events.last(), Some(&finished(StopReason::EndTurn, None)));

    // A retry-after over a minute is capped at one.
    let mut slow = Reply::error(429, "rate_limit_error", "Rate limited");
    if let Reply::Response { headers, .. } = &mut slow {
        headers.push(("retry-after", "600"));
    }
    let transport = FakeTransport::new([slow, Reply::fixture("text.sse")]);
    let (provider, waits) = fake_provider(&transport);
    run(&provider, request("Hi"));
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(60)]);
    // ...as is one too long for a Duration, without a panic.
    let mut endless = Reply::error(429, "rate_limit_error", "Rate limited");
    if let Reply::Response { headers, .. } = &mut endless {
        headers.push(("retry-after", "100000000000000000000"));
    }
    let transport = FakeTransport::new([endless, Reply::fixture("text.sse")]);
    let (provider, waits) = fake_provider(&transport);
    run(&provider, request("Hi"));
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(60)]);

    // 500 and 529 retry after 1 s then 2 s, plus jitter, then fail.
    for (status, error_type, message, kind) in [
        (500, "api_error", "Internal server error", ErrorKind::Server),
        (529, "overloaded_error", "Overloaded", ErrorKind::Overloaded),
    ] {
        let transport = FakeTransport::new([
            Reply::error(status, error_type, message),
            Reply::error(status, error_type, message),
            Reply::error(status, error_type, message),
        ]);
        let (provider, waits) = fake_provider(&transport);
        let events = run(&provider, request("Hi"));
        assert_eq!(events, [failed(kind, message)], "{status}");
        assert_eq!(transport.sent(), 3, "{status}: tried three times");
        let waits = waits.lock().unwrap();
        assert_eq!(waits.len(), 2, "{status}");
        assert!(jittered(waits[0], 1), "{status}: first wait {:?}", waits[0]);
        assert!(
            jittered(waits[1], 2),
            "{status}: second wait {:?}",
            waits[1]
        );
    }

    // A connection that fails, or breaks before any event, retries.
    let transport = FakeTransport::new([
        Reply::Fail(TransportError::new("connection refused")),
        Reply::ok(vec![
            Ok(b"event: ping\n".to_vec()),
            Err(TransportError::new("reset")),
        ]),
        Reply::fixture("text.sse"),
    ]);
    let (provider, waits) = fake_provider(&transport);
    let events = run(&provider, request("Hi"));
    assert_eq!(transport.sent(), 3);
    assert_eq!(waits.lock().unwrap().len(), 2);
    assert_eq!(events[0], started("claude-opus-5-5", "msg_01TextReply"));
    assert_eq!(events.last(), Some(&finished(StopReason::EndTurn, None)));
    assert_eq!(transport.bodies_dropped(), 2, "the broken body was dropped");

    // ...and fails as Network once the retries are spent.
    let transport = FakeTransport::new([
        Reply::Fail(TransportError::new("connection refused")),
        Reply::Fail(TransportError::new("connection refused")),
        Reply::ok(Vec::new()),
    ]);
    let (provider, _) = fake_provider(&transport);
    assert_eq!(
        run(&provider, request("Hi")),
        [failed(
            ErrorKind::Network,
            "the connection closed before the reply started"
        )]
    );
    assert_eq!(transport.sent(), 3);

    // The rest fail at once, with their kind and the API's message.
    for (status, error_type, kind) in [
        (400, "invalid_request_error", ErrorKind::InvalidRequest),
        (401, "authentication_error", ErrorKind::Auth),
        (402, "billing_error", ErrorKind::Billing),
        (403, "permission_error", ErrorKind::Permission),
        (404, "not_found_error", ErrorKind::NotFound),
        (413, "request_too_large", ErrorKind::TooLarge),
    ] {
        let message = format!("The API says {status}");
        let transport = FakeTransport::new([Reply::error(status, error_type, &message)]);
        let (provider, waits) = fake_provider(&transport);
        let events = run(&provider, request("Hi"));
        assert_eq!(events, [failed(kind, &message)], "{status}");
        assert!(!kind.retryable());
        assert_eq!(transport.sent(), 1, "{status}: not retried");
        assert!(waits.lock().unwrap().is_empty(), "{status}");
    }

    // Other 5xx statuses are Server errors, but only 500 and 529 retry.
    for status in [501, 502, 503, 504, 505] {
        let transport = FakeTransport::new([Reply::error(status, "api_error", "Bad gateway")]);
        let (provider, waits) = fake_provider(&transport);
        let events = run(&provider, request("Hi"));
        assert_eq!(
            events,
            [failed(ErrorKind::Server, "Bad gateway")],
            "{status}"
        );
        assert_eq!(transport.sent(), 1, "{status}: not retried");
        assert!(waits.lock().unwrap().is_empty(), "{status}");
    }

    // A body that isn't the API's JSON still gives its status's kind.
    let error = error_from_response(502, &HeaderMap::new(), b"<html>Bad gateway</html>");
    assert_eq!(error.kind, ErrorKind::Server);
    assert_eq!(error.message, "HTTP 502: <html>Bad gateway</html>");
    let error = error_from_response(418, &HeaderMap::new(), b"");
    assert_eq!(
        (error.kind, error.message.as_str()),
        (ErrorKind::InvalidRequest, "HTTP 418")
    );

    // A connection cut after the first event fails as Stream, unretried.
    let text = read_fixture("text.sse");
    let cut = text.len() / 2;
    let transport = FakeTransport::new([Reply::ok(vec![
        Ok(text[..cut].to_vec()),
        Err(TransportError::new("connection reset")),
    ])]);
    let (provider, waits) = fake_provider(&transport);
    let events = run(&provider, request("Hi"));
    assert_eq!(events[0], started("claude-opus-5-5", "msg_01TextReply"));
    assert_eq!(
        events.last(),
        Some(&failed(
            ErrorKind::Stream,
            "the connection broke: connection reset"
        ))
    );
    assert_eq!(transport.sent(), 1);
    assert!(waits.lock().unwrap().is_empty());
    // So does one that just closes.
    let transport = FakeTransport::new([Reply::ok(vec![Ok(text[..cut].to_vec())])]);
    let (provider, _) = fake_provider(&transport);
    let events = run(&provider, request("Hi"));
    assert_eq!(
        events.last(),
        Some(&failed(
            ErrorKind::Stream,
            "the connection closed before the reply ended"
        ))
    );
    assert_eq!(transport.sent(), 1);

    // A whole ping is an event too: a cut or a close after it isn't retried.
    let ping = b"event: ping\ndata: {\"type\":\"ping\"}\n\n".to_vec();
    for (after_ping, message) in [
        (
            vec![Err(TransportError::new("reset"))],
            "the connection broke: reset",
        ),
        (Vec::new(), "the connection closed before the reply ended"),
    ] {
        let mut chunks = vec![Ok(ping.clone())];
        chunks.extend(after_ping);
        let transport = FakeTransport::new([Reply::ok(chunks)]);
        let (provider, waits) = fake_provider(&transport);
        let events = run(&provider, request("Hi"));
        assert_eq!(events, [failed(ErrorKind::Stream, message)]);
        assert_eq!(transport.sent(), 1, "{message}: not retried");
        assert!(waits.lock().unwrap().is_empty());
    }

    // Dropping the stream drops the transport's body, closing it.
    let transport = FakeTransport::new([Reply::Response {
        status: 200,
        headers: Vec::new(),
        chunks: chunks(&text[..cut], 7),
        then_hang: true,
    }]);
    let (provider, _) = fake_provider(&transport);
    let mut stream = provider.stream(request("Hi"));
    let first = block_on(stream.next());
    assert_eq!(first, Some(started("claude-opus-5-5", "msg_01TextReply")));
    assert_eq!(transport.bodies_dropped(), 0, "the body is open");
    drop(stream);
    assert_eq!(
        transport.bodies_dropped(),
        1,
        "dropping the stream dropped it"
    );
}

#[test]
fn an_api_key_never_shows() {
    let key = ApiKey::new(KEY);
    assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
    assert!(!format!("{key:#?}").contains("secret"));
    let transport: Arc<dyn Transport> = FakeTransport::new([]);
    let provider = AnthropicProvider::new(transport, key, fe_chat_anthropic::DEFAULT_BASE_URL);
    let shown = format!("{provider:?}");
    assert!(!shown.contains(KEY), "{shown}");
    assert!(
        shown.contains("https://api.anthropic.com/v1/messages"),
        "{shown}"
    );
    assert_eq!(provider.name(), "anthropic");
}
