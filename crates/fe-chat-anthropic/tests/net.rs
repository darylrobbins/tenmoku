use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use fe_chat::{
    ChatEvent, ChatProvider, ChatRequest, ChatSettings, Conversation, build_request,
    render_user_content,
};
use fe_chat_anthropic::{AnthropicProvider, ApiKey, Response, Transport, TransportError};
use futures::StreamExt;
use futures::executor::block_on;
use futures::future::BoxFuture;
use http::HeaderMap;
use kiln_command::{Capability, Grants};
use serde_json::Value;

const KEY: &str = "sk-ant-api03-secret-test-key";

const FIXTURES: [&str; 8] = [
    "text.sse",
    "thinking.sse",
    "tool_use.sse",
    "max_tokens.sse",
    "fallback.sse",
    "refusal_pre_output.sse",
    "refusal_mid_stream.sse",
    "error_mid_stream.sse",
];

fn read_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(path).expect("fixture")
}

/// Replays a fixture as one 200 in 7-byte chunks, as `tests/provider.rs`'s
/// fake does.
struct FakeTransport(Vec<u8>);

impl Transport for FakeTransport {
    fn post(
        &self,
        _url: &str,
        _headers: Vec<(String, String)>,
        _body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        let chunks: Vec<_> = self.0.chunks(7).map(|chunk| Ok(chunk.to_vec())).collect();
        Box::pin(async move {
            Ok(Response {
                status: 200,
                headers: HeaderMap::new(),
                body: futures::stream::iter(chunks).boxed(),
            })
        })
    }
}

/// A loopback server answering one request with `fixture` as a chunked
/// `text/event-stream`, a few hundred bytes a chunk, and giving back the
/// request it read.
fn serve(fixture: Vec<u8>) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                  Transfer-Encoding: chunked\r\nRequest-Id: req_011Loopback\r\n\r\n",
            )
            .unwrap();
        for chunk in fixture.chunks(300) {
            write!(stream, "{:x}\r\n", chunk.len()).unwrap();
            stream.write_all(chunk).unwrap();
            stream.write_all(b"\r\n").unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(2));
        }
        // The client may have gone at message_stop.
        let _ = stream.write_all(b"0\r\n\r\n");
        request
    });
    (base_url, thread)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        assert_eq!(
            stream.read(&mut byte).unwrap(),
            1,
            "the request's head ends"
        );
        bytes.push(byte[0]);
    }
    let head = String::from_utf8(bytes).unwrap();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let line = line.to_ascii_lowercase();
            Some(
                line.strip_prefix("content-length:")?
                    .trim()
                    .parse()
                    .unwrap(),
            )
        })
        .expect("a content-length");
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    head + &String::from_utf8(body).unwrap()
}

fn grants(host: &str) -> Grants {
    [Capability::parse(&format!("net:{host}")).unwrap()]
        .into_iter()
        .collect()
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

#[test]
fn the_anthropic_transport_streams_a_fixture() {
    for fixture in FIXTURES {
        let bytes = read_fixture(fixture);
        let faked = AnthropicProvider::new(
            Arc::new(FakeTransport(bytes.clone())),
            ApiKey::new(KEY),
            "https://api.anthropic.com",
        );
        let expected = run(&faked, request("Hello"));

        let (base_url, server) = serve(bytes);
        let client = fe_net::Client::new(&grants("127.0.0.1"));
        let provider = AnthropicProvider::new(Arc::new(client), ApiKey::new(KEY), base_url);
        let events = run(&provider, request("Hello"));
        assert_eq!(events, expected, "{fixture}");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ChatEvent::Started { .. })),
            "{fixture}: {events:?}"
        );

        let sent = server.join().unwrap();
        assert!(sent.starts_with("POST /v1/messages HTTP/1.1\r\n"), "{sent}");
        let lower = sent.to_ascii_lowercase();
        assert!(lower.contains(&format!("x-api-key: {KEY}\r\n")), "{sent}");
        assert!(
            lower.contains("anthropic-version: 2023-06-01\r\n"),
            "{sent}"
        );
        let (_, body) = sent.split_once("\r\n\r\n").unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["stream"], true);
    }
}

#[test]
fn a_host_outside_the_grants_fails_the_reply_without_a_connection() {
    let provider = AnthropicProvider::with_client(&grants("example.com"), ApiKey::new(KEY))
        .with_sleep(|_| Box::pin(async {}));
    let events = run(&provider, request("Hello"));
    let [ChatEvent::Failed(error)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(error.message.contains("api.anthropic.com"), "{error:?}");
}

/// For a person to run, with `ANTHROPIC_API_KEY` set: a one-line reply
/// streams from the API, and the same request again reads its prompt from
/// the cache. The question is padded past the 512-token minimum a prompt
/// must reach to be cached.
#[test]
#[ignore = "reaches api.anthropic.com, with ANTHROPIC_API_KEY"]
fn live_text_reply() {
    let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY is set");
    let provider = AnthropicProvider::with_client(&grants("api.anthropic.com"), ApiKey::new(key));
    let padding: String = (1..=150)
        .map(|n| format!("Reference line {n}: this line is padding and needs no reply.\n"))
        .collect();
    let question = format!("{padding}\nReply with exactly one line: the word ready.");

    let first = run(&provider, request(&question));
    let text: String = first
        .iter()
        .filter_map(|event| match event {
            ChatEvent::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        first
            .iter()
            .any(|event| matches!(event, ChatEvent::Finished { .. })),
        "{first:?}"
    );
    assert!(text.to_ascii_lowercase().contains("ready"), "{text:?}");
    assert_eq!(text.trim().lines().count(), 1, "{text:?}");

    let second = run(&provider, request(&question));
    let cache_read: u64 = second
        .iter()
        .filter_map(|event| match event {
            ChatEvent::Usage(usage) => Some(usage.cache_read_input_tokens),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    assert!(cache_read > 0, "{second:?}");
}
