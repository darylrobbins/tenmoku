//! Haiku 4.5 takes no effort or thinking on the wire.

use fe_chat::{ChatSettings, Conversation, build_request, render_user_content};
use fe_chat_anthropic::request_body;
use serde_json::Value;

fn body(model: &str) -> Value {
    let settings = ChatSettings::default();
    let chat = Conversation::new(&settings);
    let mut request = build_request(&chat, render_user_content("Hi", &[]), &settings);
    request.model = model.to_owned();
    request.show_thinking = true;
    serde_json::from_slice(&request_body(&request, false).unwrap()).unwrap()
}

#[test]
fn haiku_request_has_no_effort_or_thinking() {
    let haiku = body("claude-haiku-4-5");
    assert!(haiku.get("output_config").is_none());
    assert!(haiku.get("thinking").is_none());
    let opus = body("claude-opus-5-5");
    assert!(opus["output_config"]["effort"].is_string());
    assert_eq!(opus["thinking"]["type"], "adaptive");
}
