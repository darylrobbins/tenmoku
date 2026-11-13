//! A mid-chat effort change on the wire: the effort-only system message
//! (`content: []` with `output_config.effort`) and its beta,
//! `mid-conversation-output-config-2026-07-01`, beside the fallback beta in
//! one `anthropic-beta`.

use std::sync::{Arc, Mutex};

use fe_chat::{
    ApiMessage, ChatProvider, ChatRequest, ChatSettings, Conversation, Effort, build_request,
    render_user_content,
};
use fe_chat_anthropic::{
    AnthropicProvider, ApiKey, Response, Transport, TransportError, request_body, request_headers,
};
use futures::StreamExt;
use futures::executor::block_on;
use futures::future::BoxFuture;
use serde_json::{Value, json};

const EFFORT_BETA: &str = "mid-conversation-output-config-2026-07-01";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// A request with one earlier exchange, then an effort change to `High`
/// before the new user message.
fn changed() -> ChatRequest {
    let mut request = unchanged();
    let at = request.messages.len() - 1;
    request
        .messages
        .insert(at, ApiMessage::effort(Effort::High));
    request
}

/// A request with one earlier exchange and no effort change.
fn unchanged() -> ChatRequest {
    let settings = ChatSettings::default();
    let chat = Conversation::new(&settings);
    let mut request = build_request(&chat, render_user_content("Second?", &[]), &settings);
    let earlier = [
        ApiMessage {
            role: fe_chat::Role::User,
            content: render_user_content("First?", &[]),
        },
        ApiMessage {
            role: fe_chat::Role::Assistant,
            content: vec![json!({"type": "text", "text": "One."})],
        },
    ];
    request.messages.splice(0..0, earlier);
    request
}

fn body_of(request: &ChatRequest, fallbacks: bool) -> Value {
    serde_json::from_slice(&request_body(request, fallbacks).unwrap()).unwrap()
}

fn beta(headers: &[(String, String)]) -> Vec<&str> {
    (headers.iter())
        .filter(|(name, _)| name == "anthropic-beta")
        .map(|(_, value)| value.as_str())
        .collect()
}

#[test]
fn effort_message_serialises_with_empty_content() {
    let request = changed();
    let body = body_of(&request, true);
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[2],
        json!({"role": "system", "content": [], "output_config": {"effort": "high"}})
    );
    // Its keys sorted, as the rest of the body's are.
    assert_eq!(
        serde_json::to_string(&messages[2]).unwrap(),
        r#"{"content":[],"output_config":{"effort":"high"},"role":"system"}"#
    );
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(
        messages[3]["role"], "user",
        "the request still ends with the user's"
    );
    // The top-level effort stays the chat's.
    assert_eq!(body["output_config"], json!({"effort": "medium"}));

    for (effort, name) in [
        (Effort::Low, "low"),
        (Effort::Medium, "medium"),
        (Effort::High, "high"),
        (Effort::XHigh, "xhigh"),
        (Effort::Max, "max"),
    ] {
        let mut request = unchanged();
        request.messages.insert(2, ApiMessage::effort(effort));
        assert_eq!(
            body_of(&request, false)["messages"][2]["output_config"]["effort"],
            name
        );
    }
}

#[test]
fn effort_message_adds_mid_conversation_beta_beside_fallback_beta() {
    let request = changed();
    let headers = request_headers("key", true, &request);
    assert_eq!(beta(&headers), [format!("{FALLBACK_BETA},{EFFORT_BETA}")]);
    let headers = request_headers("key", false, &request);
    assert_eq!(beta(&headers), [EFFORT_BETA]);

    // And the provider sends them.
    let transport = Arc::new(Recording::default());
    let provider = AnthropicProvider::new(transport.clone(), ApiKey::new("key"), "http://test")
        .with_sleep(|_| Box::pin(async {}));
    let _: Vec<_> = block_on(provider.stream(request).collect());
    let sent = transport.headers.lock().unwrap();
    assert_eq!(beta(&sent[0]), [format!("{FALLBACK_BETA},{EFFORT_BETA}")]);
}

#[test]
fn no_effort_message_no_mid_conversation_beta() {
    let request = unchanged();
    assert_eq!(
        beta(&request_headers("key", true, &request)),
        [FALLBACK_BETA]
    );
    assert!(beta(&request_headers("key", false, &request)).is_empty());
    let body = body_of(&request, true);
    let roles: Vec<&str> = (body["messages"].as_array().unwrap().iter())
        .map(|message| message["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "assistant", "user"]);

    let transport = Arc::new(Recording::default());
    let provider = AnthropicProvider::new(transport.clone(), ApiKey::new("key"), "http://test")
        .with_fallbacks(false)
        .with_sleep(|_| Box::pin(async {}));
    let _: Vec<_> = block_on(provider.stream(request).collect());
    assert!(beta(&transport.headers.lock().unwrap()[0]).is_empty());
}

/// Records each request's headers and answers 400, so nothing is retried.
#[derive(Default)]
struct Recording {
    headers: Mutex<Vec<Vec<(String, String)>>>,
}

impl Transport for Recording {
    fn post(
        &self,
        _url: &str,
        headers: Vec<(String, String)>,
        _body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        self.headers.lock().unwrap().push(headers);
        Box::pin(async {
            Ok(Response {
                status: 400,
                headers: http::HeaderMap::new(),
                body: futures::stream::empty().boxed(),
            })
        })
    }
}
