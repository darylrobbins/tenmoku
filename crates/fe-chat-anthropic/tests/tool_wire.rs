//! The tool wire: tools with `eager_input_streaming`, `tool_result` blocks,
//! `output_config.task_budget`, the `tool_removal` system message, and the
//! betas that go with them, joined with the existing ones.
//!
//! The expected bodies are fixtures under `tests/fixtures/tool_wire/`, compared
//! as values; set `UPDATE_FIXTURES=1` to rewrite them.

use fe_chat::{
    ApiMessage, ChatRequest, ChatSettings, Conversation, Effort, Role, ToolResult, build_request,
    render_user_content, tool_set,
};
use fe_chat_anthropic::{request_body, request_headers};
use serde_json::{Value, json};

const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const EFFORT_BETA: &str = "mid-conversation-output-config-2026-07-01";
const BUDGET_BETA: &str = "task-budgets-2026-03-13";
const TOOLS_BETA: &str = "mid-conversation-tool-changes-2026-07-01";

fn with_tools(budget: u32) -> ChatRequest {
    let settings = ChatSettings::default();
    let mut chat = Conversation::new(&settings);
    assert!(chat.set_tools(tool_set(), budget));
    build_request(&chat, render_user_content("Look.", &[]), &settings)
}

/// A request whose history holds a tool round: the call, then its results.
fn after_a_round(budget: u32) -> ChatRequest {
    let mut request = with_tools(budget);
    let user = request.messages.pop().unwrap();
    request.messages.extend([
        user,
        ApiMessage {
            role: Role::Assistant,
            content: vec![
                json!({"type": "tool_use", "id": "toolu_1", "name": "read_file",
                       "input": {"path": "a.rs"}}),
            ],
        },
        ApiMessage {
            role: Role::User,
            content: vec![
                ToolResult::ok("toolu_1", "fn a() {}").block(),
                ToolResult::error("toolu_2", "Denied by your settings").block(),
            ],
        },
    ]);
    request
}

fn body(request: &ChatRequest) -> String {
    let bytes = request_body(request, true).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    serde_json::to_string_pretty(&value).unwrap() + "\n"
}

fn fixture(name: &str, actual: &str) {
    let path = format!(
        "{}/tests/fixtures/tool_wire/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    // Compared as values: tool schemas' key order depends on whether another
    // crate turns on serde_json's `preserve_order`.
    let expected = std::fs::read_to_string(&path).unwrap();
    let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap();
    assert_eq!(parse(actual), parse(&expected), "{name}");
}

fn betas(request: &ChatRequest, fallbacks: bool) -> Vec<String> {
    request_headers("key", fallbacks, request)
        .into_iter()
        .filter(|(name, _)| name == "anthropic-beta")
        .map(|(_, value)| value)
        .collect()
}

#[test]
fn request_fixtures() {
    fixture("tools.json", &body(&with_tools(0)));
    fixture("tools_budget.json", &body(&after_a_round(30_000)));
}

#[test]
fn every_tool_streams_eagerly_and_no_tool_choice_is_sent() {
    let value: Value =
        serde_json::from_slice(&request_body(&with_tools(0), true).unwrap()).unwrap();
    let tools = value["tools"].as_array().unwrap();
    assert_eq!(tools.len(), tool_set().len());
    for tool in tools {
        assert_eq!(tool["eager_input_streaming"], true);
    }
    assert!(value.get("tool_choice").is_none());
    assert_eq!(value["output_config"], json!({"effort": "medium"}));
}

#[test]
fn tool_results_go_back_in_one_user_message_with_sorted_keys() {
    let value: Value =
        serde_json::from_slice(&request_body(&after_a_round(0), true).unwrap()).unwrap();
    let last = value["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["role"], "user");
    assert_eq!(
        serde_json::to_string(&last["content"][1]).unwrap(),
        r#"{"content":"Denied by your settings","is_error":true,"tool_use_id":"toolu_2","type":"tool_result"}"#
    );
}

#[test]
fn a_task_budget_is_in_output_config_beside_effort() {
    let value: Value =
        serde_json::from_slice(&request_body(&with_tools(30_000), true).unwrap()).unwrap();
    assert_eq!(
        value["output_config"],
        json!({"effort": "medium", "task_budget": {"total": 30000, "type": "tokens"}})
    );
    let value: Value =
        serde_json::from_slice(&request_body(&with_tools(0), true).unwrap()).unwrap();
    assert!(value["output_config"].get("task_budget").is_none());
}

#[test]
fn the_task_budget_survives_a_model_without_effort() {
    let mut request = with_tools(30_000);
    request.model = "claude-haiku-4-5".to_owned();
    let value: Value = serde_json::from_slice(&request_body(&request, true).unwrap()).unwrap();
    assert_eq!(
        value["output_config"],
        json!({"task_budget": {"total": 30000, "type": "tokens"}})
    );
}

#[test]
fn the_budget_beta_joins_the_existing_ones() {
    let plain = with_tools(0);
    assert_eq!(betas(&plain, true), [FALLBACK_BETA]);
    assert!(betas(&plain, false).is_empty());

    let budget = with_tools(30_000);
    assert_eq!(
        betas(&budget, true),
        [format!("{FALLBACK_BETA},{BUDGET_BETA}")]
    );
    assert_eq!(betas(&budget, false), [BUDGET_BETA]);

    let mut both = with_tools(30_000);
    let at = both.messages.len() - 1;
    both.messages.insert(at, ApiMessage::effort(Effort::High));
    assert_eq!(
        betas(&both, true),
        [format!("{FALLBACK_BETA},{EFFORT_BETA},{BUDGET_BETA}")]
    );
}

#[test]
fn a_withdrawal_serialises_with_the_removal_last_and_its_beta() {
    let settings = ChatSettings::default();
    let mut chat = Conversation::new(&settings);
    assert!(chat.set_tools(tool_set(), 30_000));
    chat.withdraw_tools();
    let request = build_request(&chat, render_user_content("Go on", &[]), &settings);
    let last = request.messages.last().unwrap();
    assert!(!last.removed_tools().is_empty(), "the removal is last");

    let bytes = request_body(&request, true).expect("a terminal tool_removal is valid");
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let messages = value["messages"].as_array().unwrap();
    assert_eq!(messages[messages.len() - 2]["role"], "user");
    let removal = messages.last().unwrap();
    assert_eq!(removal["role"], "system");
    assert!(removal.get("output_config").is_none(), "no effort is sent");
    assert_eq!(removal["content"][0]["type"], "tool_removal");
    assert_eq!(
        removal["content"][0]["tool"]["type"], "tool_reference",
        "{removal}"
    );
    assert_eq!(
        betas(&request, true),
        [format!("{FALLBACK_BETA},{BUDGET_BETA},{TOOLS_BETA}")]
    );
}

#[test]
fn an_assistant_prefill_or_a_bare_system_message_still_fails() {
    let mut request = with_tools(0);
    request.messages.push(ApiMessage {
        role: Role::Assistant,
        content: vec![json!({"type": "text", "text": "Sure"})],
    });
    assert!(request_body(&request, true).is_err());

    let mut request = with_tools(0);
    request.messages.push(ApiMessage::effort(Effort::High));
    assert!(request_body(&request, true).is_err());

    let mut request = with_tools(0);
    request.messages.pop();
    request
        .messages
        .push(ApiMessage::tool_removal(["search"], Effort::Medium));
    assert!(request_body(&request, true).is_err(), "no user before it");
}
