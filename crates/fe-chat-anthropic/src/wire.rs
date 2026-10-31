//! The request as the Messages API takes it (the record's § 3): the JSON
//! body and the headers.
//!
//! JSON built here is built with its keys in sorted order, as `fe-chat`'s
//! is, so it serialises the same whether or not another crate turns on
//! serde_json's `preserve_order`. Message content is sent as it was kept.

use fe_chat::{ApiMessage, ChatRequest, Effort, ErrorKind, ProviderError, Role, SystemBlock};
use serde_json::{Map, Value};

/// The `anthropic-version` header's value.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The beta that `fallbacks: "default"` takes, exactly. (The array form
/// takes `server-side-fallback-2026-06-01`; mixing them is a 400.)
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// The body of a streamed request for `request`, with `fallbacks: "default"`
/// when `fallbacks` is on.
///
/// Never sends sampling parameters, `tool_choice` or an assistant prefill:
/// all are 400s on Claude Opus 5.5. A request whose last message isn't the
/// user's fails here, before anything is sent.
pub fn request_body(request: &ChatRequest, fallbacks: bool) -> Result<Vec<u8>, ProviderError> {
    match request.messages.last() {
        Some(message) if message.role == Role::User => {}
        _ => {
            return Err(ProviderError::new(
                ErrorKind::InvalidRequest,
                "a request must end with the user's message",
            ));
        }
    }
    let mut body = Map::new();
    body.insert(
        "cache_control".to_owned(),
        object([("type", "ephemeral".into())]),
    );
    if fallbacks {
        body.insert("fallbacks".to_owned(), "default".into());
    }
    body.insert("max_tokens".to_owned(), request.max_tokens.into());
    body.insert(
        "messages".to_owned(),
        request.messages.iter().map(message).collect(),
    );
    body.insert("model".to_owned(), request.model.clone().into());
    body.insert(
        "output_config".to_owned(),
        object([("effort", effort(request.effort).into())]),
    );
    body.insert("stream".to_owned(), true.into());
    body.insert(
        "system".to_owned(),
        request.system.iter().map(system_block).collect(),
    );
    if request.show_thinking {
        body.insert(
            "thinking".to_owned(),
            object([
                ("display", "summarized".into()),
                ("type", "adaptive".into()),
            ]),
        );
    }
    if !request.tools.is_empty() {
        let tools = request.tools.iter().map(|tool| {
            object([
                ("description", tool.description.clone().into()),
                ("eager_input_streaming", true.into()),
                ("input_schema", tool.input_schema.clone()),
                ("name", tool.name.clone().into()),
                ("strict", tool.strict.into()),
            ])
        });
        body.insert("tools".to_owned(), tools.collect());
    }
    serde_json::to_vec(&Value::Object(body))
        .map_err(|error| ProviderError::new(ErrorKind::InvalidRequest, error.to_string()))
}

/// The request's headers, the key among them. The fallback beta goes only
/// with `fallbacks: "default"`.
pub fn request_headers(api_key: &str, fallbacks: bool) -> Vec<(String, String)> {
    let mut headers = vec![
        ("x-api-key".to_owned(), api_key.to_owned()),
        ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
        (
            "user-agent".to_owned(),
            format!("tenmoku/{}", env!("CARGO_PKG_VERSION")),
        ),
    ];
    if fallbacks {
        headers.push(("anthropic-beta".to_owned(), FALLBACK_BETA.to_owned()));
    }
    headers
}

fn message(message: &ApiMessage) -> Value {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    object([
        ("content", Value::Array(message.content.clone())),
        ("role", role.into()),
    ])
}

fn system_block(block: &SystemBlock) -> Value {
    let mut fields = Map::new();
    if block.cache_control.is_some() {
        fields.insert(
            "cache_control".to_owned(),
            object([("type", "ephemeral".into())]),
        );
    }
    fields.insert("text".to_owned(), block.text.clone().into());
    fields.insert("type".to_owned(), "text".into());
    Value::Object(fields)
}

fn effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
    }
}

/// An object of `fields`, given in sorted order.
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}
