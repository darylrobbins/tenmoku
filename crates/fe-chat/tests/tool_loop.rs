// SPDX-License-Identifier: Apache-2.0

//! The tool loop's turn model (the agents record's § 6 and § 12): rounds
//! in an append-only history, one result message per round, how a turn
//! ends (stop, round limit, `max_tokens`, refusal, failure, invalid input,
//! trust withdrawal), and the exact requests that follow.

use std::collections::HashSet;

use fe_chat::{
    ApiMessage, ChatEvent, ChatProvider, ChatRequest, ChatSettings, Conversation, Effort,
    ErrorKind, LIST_FILES, MIN_TASK_BUDGET, NotRun, PROPOSE_EDIT, ProviderError, READ_FILE,
    RUN_COMMAND, ReplyScript, Role, SEARCH, ScriptedProvider, StopReason, ToolInput, ToolResult,
    TurnState, build_request, render_user_content, round_request, tool_set,
};
use futures::StreamExt;
use futures::executor::block_on;
use serde_json::{Value, json};

const MODEL: &str = "claude-opus-5-5";

fn settings() -> ChatSettings {
    ChatSettings::default()
}

/// A chat with the five tools and a task budget.
fn chat() -> Conversation {
    let mut chat = Conversation::new(&settings());
    assert!(chat.set_tools(tool_set(), 30_000));
    chat
}

fn reply() -> ReplyScript {
    ReplyScript::new(MODEL, "msg")
}

fn read(path: &str) -> Value {
    json!({"path": path, "start_line": null, "end_line": null})
}

fn search(pattern: &str) -> Value {
    json!({"pattern": pattern, "regex": false, "case_sensitive": true, "glob": null})
}

fn list() -> Value {
    json!({"dir": null, "glob": null})
}

fn edit(path: &str) -> Value {
    json!({"path": path, "edits": [{"old_text": "a", "new_text": "b"}]})
}

fn play(chat: &mut Conversation, events: Vec<ChatEvent>) {
    for event in events {
        chat.apply(event);
    }
}

fn stream(provider: &ScriptedProvider, request: ChatRequest) -> Vec<ChatEvent> {
    block_on(provider.stream(request).collect())
}

/// Sends a new turn of `text`: its request, then the reply streamed in.
fn send(chat: &mut Conversation, provider: &ScriptedProvider, text: &str) -> ChatRequest {
    let content = render_user_content(text, &[]);
    let request = build_request(chat, content.clone(), &settings());
    chat.begin_turn_with(content, Vec::new(), &settings());
    let events = stream(provider, request.clone());
    play(chat, events);
    request
}

/// Sends the last turn's next round (or the same round again).
fn next(chat: &mut Conversation, provider: &ScriptedProvider) -> ChatRequest {
    let request = round_request(chat, &settings()).expect("a turn");
    let events = stream(provider, request.clone());
    play(chat, events);
    request
}

/// Answers every pending call with `"<name> ok"`, then starts the next
/// round.
fn answer_all(chat: &mut Conversation) {
    for call in chat.pending_calls() {
        assert!(chat.add_result(ToolResult::ok(&call.id, format!("{} ok", call.name))));
    }
    assert!(chat.next_round());
}

/// What the streaming turn has committed, as its next request sends it.
fn committed(chat: &mut Conversation) -> Vec<ApiMessage> {
    round_request(chat, &settings()).unwrap().messages
}

fn json_of<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

/// Each message serialised, as the cache compares them.
fn message_bytes(request: &ChatRequest) -> Vec<String> {
    request.messages.iter().map(json_of).collect()
}

fn user(content: Vec<Value>) -> ApiMessage {
    ApiMessage {
        role: Role::User,
        content,
    }
}

fn assistant(content: Vec<Value>) -> ApiMessage {
    ApiMessage {
        role: Role::Assistant,
        content,
    }
}

fn text(text: &str) -> Value {
    json!({"text": text, "type": "text"})
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({"id": id, "input": input, "name": name, "type": "tool_use"})
}

fn result(id: &str, content: &str, is_error: bool) -> Value {
    json!({"content": content, "is_error": is_error, "tool_use_id": id, "type": "tool_result"})
}

fn question(text: &str) -> Vec<Value> {
    render_user_content(text, &[])
}

/// Checks `messages` is a history the API takes: user and assistant
/// alternate (a system message only after a user message, and last or
/// before an assistant one), no message is empty, and each assistant
/// message's calls are answered one to one, in order, at the head of the
/// next user message, with no other result anywhere.
fn assert_valid(messages: &[ApiMessage]) {
    let mut expect_user = true;
    let mut answered = HashSet::new();
    for (at, message) in messages.iter().enumerate() {
        match message.role {
            Role::System { .. } => {
                assert!(at > 0 && messages[at - 1].role == Role::User, "{at}");
                let next = messages.get(at + 1).map(|m| &m.role);
                assert!(matches!(next, None | Some(Role::Assistant)), "{at}");
                continue;
            }
            Role::User => assert!(expect_user, "two assistant-less user messages at {at}"),
            Role::Assistant => assert!(!expect_user, "two assistant messages at {at}"),
        }
        expect_user = !expect_user;
        assert!(!message.content.is_empty(), "empty message at {at}");
        let results: Vec<&Value> = (message.content.iter())
            .filter(|block| block["type"] == "tool_result")
            .collect();
        if message.role == Role::User {
            let before = &messages[..at];
            let calls: Vec<String> = before
                .iter()
                .rev()
                .find(|m| m.role == Role::Assistant)
                .map(ids_of)
                .unwrap_or_default();
            let ids: Vec<String> = results
                .iter()
                .map(|r| r["tool_use_id"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(ids, calls, "results at {at} answer the calls before");
            // Results come first, then any text.
            let first_other = message
                .content
                .iter()
                .position(|block| block["type"] != "tool_result")
                .unwrap_or(message.content.len());
            assert_eq!(first_other, results.len(), "results lead message {at}");
            for id in ids {
                assert!(answered.insert(id), "answered twice");
            }
        } else {
            assert!(results.is_empty());
            let next_user = messages[at + 1..].iter().find(|m| m.role == Role::User);
            if !ids_of(message).is_empty() {
                assert!(next_user.is_some(), "calls at {at} have no result message");
            }
        }
    }
}

fn ids_of(message: &ApiMessage) -> Vec<String> {
    (message.content.iter())
        .filter(|block| block["type"] == "tool_use")
        .map(|block| block["id"].as_str().unwrap().to_owned())
        .collect()
}

/// A request is valid and ends with the user's message, or the
/// `tool_removal` message after it.
fn assert_valid_request(request: &ChatRequest) {
    assert_valid(&request.messages);
    let last = request.messages.last().unwrap();
    assert!(
        last.role == Role::User || !last.removed_tools().is_empty(),
        "{last:?}"
    );
}

// C1
#[test]
fn rounds_extend_a_byte_identical_prefix() {
    let provider = ScriptedProvider::new([
        reply()
            .thinking("plan", "sig1")
            .text("Reading both.")
            .tool_use("toolu_1", READ_FILE, read("src/lib.rs"))
            .tool_use("toolu_2", SEARCH, search("fn main"))
            .stop(StopReason::ToolUse),
        reply()
            .thinking("more", "sig2")
            .tool_use("toolu_3", LIST_FILES, list())
            .stop(StopReason::ToolUse),
        reply()
            .text("Done: it parses args.")
            .stop(StopReason::EndTurn),
        reply().text("Second answer.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "What does main do?");
    answer_all(&mut chat);
    next(&mut chat, &provider);
    answer_all(&mut chat);
    next(&mut chat, &provider);
    assert_eq!(chat.turns[0].state, TurnState::Done);
    assert_eq!(chat.turns[0].rounds.len(), 2);
    send(&mut chat, &provider, "And then?");

    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    let first = &requests[0];
    assert_eq!(first.tools, tool_set());
    assert_eq!(first.task_budget, Some(30_000));
    for (n, request) in requests.iter().enumerate() {
        assert_eq!(
            json_of(&request.tools),
            json_of(&first.tools),
            "tools of {n}"
        );
        assert_eq!(
            json_of(&request.system),
            json_of(&first.system),
            "system of {n}"
        );
        assert_eq!(request.task_budget, first.task_budget);
        assert_eq!(request.model, first.model);
        assert_valid_request(request);
    }
    for pair in requests.windows(2) {
        let (before, after) = (message_bytes(&pair[0]), message_bytes(&pair[1]));
        assert!(after.len() > before.len());
        assert_eq!(after[..before.len()], before[..], "request N+1 extends N");
    }
    // Each round adds its reply, verbatim, and its one result message.
    assert_eq!(requests[1].messages.len(), 3);
    assert_eq!(
        requests[1].messages[1],
        assistant(vec![
            json!({"signature": "sig1", "thinking": "plan", "type": "thinking"}),
            text("Reading both."),
            tool_use("toolu_1", READ_FILE, read("src/lib.rs")),
            tool_use("toolu_2", SEARCH, search("fn main")),
        ])
    );
    assert_eq!(requests[2].messages.len(), 5);
    assert_eq!(requests[3].messages.len(), 7);
    assert_eq!(
        requests[3].messages[5],
        assistant(vec![text("Done: it parses args.")])
    );
    assert_eq!(chat.history().len(), 8);
    assert_valid(&chat.history());
    assert_eq!(
        chat.turns[0].text(),
        "Reading both.\n\nDone: it parses args."
    );
}

// C2
#[test]
fn each_round_has_one_result_message() {
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_a", READ_FILE, read("a.rs"))
            .tool_use(
                "toolu_b",
                RUN_COMMAND,
                json!({"program": "cargo", "args": ["test"], "cwd": null}),
            )
            .tool_use("toolu_c", SEARCH, search("x"))
            .stop(StopReason::ToolUse),
        reply().text("All three.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Check");
    let calls = chat.pending_calls();
    assert_eq!(
        calls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["toolu_a", "toolu_b", "toolu_c"]
    );
    // Results land as calls finish, in any order; the round can't move on
    // until each call has exactly one.
    assert!(chat.add_result(ToolResult::ok("toolu_c", "x: 1 match")));
    assert!(!chat.next_round());
    assert!(
        !chat.add_result(ToolResult::ok("toolu_c", "again")),
        "one result per call"
    );
    assert!(!chat.add_result(ToolResult::ok("toolu_z", "no such call")));
    assert!(chat.add_result(ToolResult::error("toolu_b", "Denied by your settings")));
    assert_eq!(chat.pending_calls().len(), 1);
    assert!(chat.add_result(ToolResult::ok("toolu_a", "fn a() {}")));
    assert!(chat.pending_calls().is_empty());
    assert!(chat.next_round());
    let request = next(&mut chat, &provider);

    // One user message after the reply, holding every result in the calls'
    // order, the denied one as an error.
    assert_eq!(request.messages.len(), 3);
    assert_eq!(
        request.messages[2],
        user(vec![
            result("toolu_a", "fn a() {}", false),
            result("toolu_b", "Denied by your settings", true),
            result("toolu_c", "x: 1 match", false),
        ])
    );
    let round = &chat.turns[0].rounds[0];
    assert_eq!(round.results.len(), 3);
    assert_valid(&chat.history());
}

// C3
#[test]
fn stop_and_the_round_limit_leave_valid_history() {
    // Stopped while two calls run, one of them finished.
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .tool_use(
                "toolu_2",
                RUN_COMMAND,
                json!({"program": "make", "args": [], "cwd": null}),
            )
            .stop(StopReason::ToolUse),
        reply()
            .tool_use("toolu_3", LIST_FILES, list())
            .stop(StopReason::ToolUse),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Build it");
    assert!(chat.add_result(ToolResult::ok("toolu_1", "text of a")));
    chat.stop();
    let turn = &chat.turns[0];
    assert_eq!(turn.state, TurnState::Stopped);
    assert!(chat.pending_calls().is_empty());
    let history = chat.history();
    assert_valid(&history);
    assert_eq!(
        history.last().unwrap(),
        &user(vec![
            result("toolu_1", "text of a", false),
            result("toolu_2", NotRun::Stopped.text(), true),
        ])
    );

    // At the round limit: the reply's calls are answered, never run, and
    // the turn ends with Continue, which sends that message.
    let mut chat = self::chat();
    send(&mut chat, &provider, "List");
    assert!(chat.end_round(NotRun::RoundLimit));
    assert_eq!(chat.turns[0].state, TurnState::Done);
    let history = chat.history();
    assert_valid(&history);
    assert_eq!(
        history.last().unwrap(),
        &user(vec![result("toolu_3", NotRun::RoundLimit.text(), true)])
    );
    assert!(chat.resume());
    assert_eq!(chat.turns[0].state, TurnState::Streaming);
    let request = round_request(&mut chat, &settings()).unwrap();
    assert_eq!(request.messages, history);
    assert_valid_request(&request);
}

// C4
#[test]
fn max_tokens_and_refusal_run_no_tools() {
    let provider = ScriptedProvider::new([
        reply()
            .text("Let me read it.")
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .stop(StopReason::MaxTokens),
        reply()
            .tool_use(
                "toolu_2",
                RUN_COMMAND,
                json!({"program": "rm", "args": ["-r"], "cwd": null}),
            )
            .stop(StopReason::Refusal),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Read a.rs");
    assert!(
        chat.pending_calls().is_empty(),
        "no call of a cut-off reply runs"
    );
    let turn = &chat.turns[0];
    assert_eq!(turn.state, TurnState::Done);
    assert!(turn.cut_off());
    // Kept, its call answered as cut off.
    let history = chat.history();
    assert_valid(&history);
    assert_eq!(
        history[1..],
        [
            assistant(vec![
                text("Let me read it."),
                tool_use("toolu_1", READ_FILE, read("a.rs"))
            ]),
            user(vec![result("toolu_1", NotRun::CutOff.text(), true)]),
        ]
    );

    let before = chat.history();
    send(&mut chat, &provider, "Clean up");
    assert!(
        chat.pending_calls().is_empty(),
        "no call of a refused reply runs"
    );
    assert_eq!(chat.turns[1].state, TurnState::Refused);
    assert_eq!(chat.history(), before, "a refused first round is left out");
}

// C5
#[test]
fn a_later_refusal_keeps_earlier_rounds_and_a_failed_round_retries_identically() {
    let provider = ScriptedProvider::new([
        reply()
            .thinking("t", "sig")
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .stop(StopReason::ToolUse),
        reply().partial_text("I won't").stop(StopReason::Refusal),
        reply().text("Fine.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Do it");
    answer_all(&mut chat);
    let refused = next(&mut chat, &provider);
    assert_eq!(chat.turns[0].state, TurnState::Refused);
    // Round 1's reply and results stay, byte for byte as sent.
    let history = chat.history();
    assert_eq!(
        message_bytes(&refused),
        history.iter().map(json_of).collect::<Vec<_>>()
    );
    assert_valid(&history);
    // The next text joins round 1's result message as a block after it.
    let request = send(&mut chat, &provider, "Something else");
    let mut expected = refused.messages.clone();
    expected
        .last_mut()
        .unwrap()
        .content
        .extend(question("Something else"));
    assert_eq!(request.messages, expected);
    assert_valid_request(&request);

    // A later round that fails: Retry sends the identical request.
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", SEARCH, search("x"))
            .stop(StopReason::ToolUse),
        reply()
            .partial_text("Half")
            .fail(ProviderError::new(ErrorKind::Overloaded, "overloaded")),
        reply().text("Whole.").stop(StopReason::EndTurn),
    ]);
    let mut chat = self::chat();
    send(&mut chat, &provider, "Find x");
    answer_all(&mut chat);
    let failed = next(&mut chat, &provider);
    assert!(matches!(chat.turns[0].state, TurnState::Failed(_)));
    assert_eq!(chat.turns[0].rounds.len(), 1, "the earlier round stays");
    assert_valid(&chat.history());
    assert!(chat.retry_round());
    let retried = next(&mut chat, &provider);
    assert_eq!(json_of(&retried), json_of(&failed));
    assert_eq!(chat.turns[0].state, TurnState::Done);
    assert_eq!(chat.turns[0].text(), "Whole.");

    // A failed first round retries as the request that began the turn.
    let provider = ScriptedProvider::new([
        reply().fail(ProviderError::new(ErrorKind::Server, "500")),
        reply().text("Ok.").stop(StopReason::EndTurn),
    ]);
    let mut chat = self::chat();
    let first = send(&mut chat, &provider, "Hi");
    assert!(chat.retry_round());
    assert_eq!(json_of(&next(&mut chat, &provider)), json_of(&first));
}

// C6
#[test]
fn the_next_user_text_goes_in_after_the_last_results() {
    let provider = ScriptedProvider::new([
        reply()
            .text("Running.")
            .tool_use(
                "toolu_1",
                RUN_COMMAND,
                json!({"program": "cargo", "args": ["build"], "cwd": null}),
            )
            .stop(StopReason::ToolUse),
        reply().text("Built.").stop(StopReason::EndTurn),
        reply().text("Ok.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    let first = send(&mut chat, &provider, "Build");
    chat.stop();
    let stopped = chat.history();

    let request = send(&mut chat, &provider, "Never mind, explain");
    let messages = &request.messages;
    assert_eq!(messages.len(), 3, "no new user message");
    assert_eq!(messages[..2], stopped[..2]);
    let mut joined = vec![result("toolu_1", NotRun::Stopped.text(), true)];
    joined.extend(question("Never mind, explain"));
    assert_eq!(messages[2], user(joined));
    // Every block sent before is unchanged.
    assert_eq!(json_of(&messages[0]), json_of(&first.messages[0]));
    assert_valid_request(&request);

    // The joined message stays as sent in later requests.
    let later = send(&mut chat, &provider, "Thanks");
    assert_eq!(message_bytes(&later)[..3], message_bytes(&request)[..]);
    assert_eq!(later.messages[3], assistant(vec![text("Built.")]));
    assert_eq!(later.messages[4], user(question("Thanks")));
    assert_valid_request(&later);
}

// C7
#[test]
fn the_tool_removal_message_comes_last_after_the_users_text() {
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", PROPOSE_EDIT, edit("a.rs"))
            .tool_use("toolu_2", READ_FILE, read("secret.rs"))
            .tool_use(
                "toolu_3",
                RUN_COMMAND,
                json!({"program": "make", "args": [], "cwd": null}),
            )
            .stop(StopReason::ToolUse),
        reply()
            .text("Without tools then.")
            .tool_use("toolu_4", READ_FILE, read("b.rs"))
            .stop(StopReason::ToolUse),
        reply().text("Ok.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Change a.rs");
    assert!(chat.add_result(ToolResult::ok(
        "toolu_1",
        "Proposed 1 change to a.rs; the user will review it."
    )));
    assert!(chat.add_result(ToolResult::ok("toolu_2", "the secret text")));
    chat.withdraw_tools();
    assert_eq!(chat.turns[0].state, TurnState::Stopped);
    assert!(!chat.has_tools());
    let history = chat.history();
    assert_valid(&history);
    // The finished propose_edit keeps its result; the finished read's
    // content is discarded, never sent.
    assert_eq!(
        history[2],
        user(vec![
            result(
                "toolu_1",
                "Proposed 1 change to a.rs; the user will review it.",
                false
            ),
            result("toolu_2", NotRun::TrustWithdrawn.text(), true),
            result("toolu_3", NotRun::TrustWithdrawn.text(), true),
        ])
    );
    assert!(!json_of(&history).contains("the secret text"));

    let request = send(&mut chat, &provider, "Go on");
    let names: Vec<String> = tool_set().into_iter().map(|tool| tool.name).collect();
    let removal = ApiMessage::tool_removal(names.iter().map(String::as_str), Effort::Medium);
    let mut joined = history[2].content.clone();
    joined.extend(question("Go on"));
    assert_eq!(
        request.messages,
        [
            history[0].clone(),
            history[1].clone(),
            user(joined),
            removal.clone()
        ]
    );
    assert_eq!(removal.removed_tools(), names);
    assert_eq!(removal.effort_change(), None, "a removal changes no effort");
    assert_eq!(request.tools, tool_set(), "the tools stay declared");
    assert_valid_request(&request);
    // A call after the withdrawal is answered, never run.
    assert!(chat.pending_calls().is_empty());
    assert_eq!(
        chat.turns[1].rounds[0].results,
        [ToolResult::not_run("toolu_4", NotRun::TrustWithdrawn)]
    );

    // Later requests keep the removal where it was, and add no other.
    let later = send(&mut chat, &provider, "Thanks");
    assert_eq!(message_bytes(&later)[..4], message_bytes(&request)[..]);
    assert_eq!(
        later
            .messages
            .iter()
            .filter(|m| !m.removed_tools().is_empty())
            .count(),
        1
    );
    assert_valid_request(&later);
}

// C8
#[test]
fn finalisation_keeps_calls_and_results_matched_one_to_one() {
    let calls = || {
        reply()
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .tool_use("toolu_2", SEARCH, search("x"))
            .tool_use("toolu_3", LIST_FILES, list())
    };
    type End = fn(&mut Conversation);
    let ends: [(&str, End); 5] = [
        ("stop", |chat| chat.stop()),
        ("withdrawal", |chat| chat.withdraw_tools()),
        ("round limit", |chat| {
            assert!(chat.end_round(NotRun::RoundLimit))
        }),
        ("time limit", |chat| {
            assert!(chat.end_round(NotRun::TimeLimit))
        }),
        ("next round", |chat| {
            for call in chat.pending_calls() {
                chat.add_result(ToolResult::ok(&call.id, "ok"));
            }
            assert!(chat.next_round());
            chat.stop();
        }),
    ];
    for (name, end) in ends {
        let provider = ScriptedProvider::new([calls().stop(StopReason::ToolUse)]);
        let mut chat = chat();
        send(&mut chat, &provider, "Go");
        assert!(chat.add_result(ToolResult::ok("toolu_2", "found")));
        end(&mut chat);
        let history = chat.history();
        assert_valid(&history);
        let round = &chat.turns[0].rounds[0];
        let ids: Vec<&str> = round
            .results
            .iter()
            .map(|r| r.tool_use_id.as_str())
            .collect();
        assert_eq!(ids, ["toolu_1", "toolu_2", "toolu_3"], "{name}");
    }

    // max_tokens: each kept call answered once.
    let provider = ScriptedProvider::new([calls().stop(StopReason::MaxTokens)]);
    let mut chat = chat();
    send(&mut chat, &provider, "Go");
    assert_valid(&chat.history());
    assert_eq!(chat.turns[0].rounds[0].results.len(), 3);

    // A call left out by a mid-output fallback isn't answered.
    let fallback = json!({"from": "a", "to": "b", "type": "fallback"});
    let mut chat = self::chat();
    chat.begin_turn_with(question("Go"), Vec::new(), &settings());
    let mut events = reply()
        .tool_use("toolu_early", READ_FILE, read("a.rs"))
        .streaming();
    events.push(ChatEvent::BlockFinished {
        index: 1,
        block: fallback,
    });
    play(&mut chat, events);
    let mut rest = reply()
        .text("t")
        .text("u")
        .tool_use("toolu_late", SEARCH, search("y"))
        .stop(StopReason::ToolUse);
    // Index the rest after the fallback block.
    for event in &mut rest {
        if let ChatEvent::BlockStarted { index, .. }
        | ChatEvent::TextDelta { index, .. }
        | ChatEvent::ToolInputDelta { index, .. }
        | ChatEvent::BlockFinished { index, .. } = event
        {
            *index += 2;
        }
    }
    play(&mut chat, rest);
    let pending: Vec<String> = chat.pending_calls().into_iter().map(|c| c.id).collect();
    assert_eq!(pending, ["toolu_late"]);
    chat.stop();
    assert_valid(&chat.history());
    assert_eq!(chat.turns[0].rounds[0].results.len(), 1);
}

// C9
#[test]
fn an_uncommitted_streaming_reply_keeps_finished_text_only() {
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .stop(StopReason::ToolUse),
        reply()
            .thinking("hmm", "sig")
            .text("First I'll check b.")
            .tool_use("toolu_2", READ_FILE, read("b.rs"))
            .partial_text("and then")
            .tool_input("toolu_3", SEARCH, "{\"pattern\": \"x")
            .streaming(),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Look");
    answer_all(&mut chat);
    let committed = committed(&mut chat);
    next(&mut chat, &provider);
    assert!(chat.pending_calls().is_empty(), "nothing runs mid-stream");
    chat.stop();
    let turn = &chat.turns[0];
    assert_eq!(turn.state, TurnState::Stopped);
    assert_eq!(turn.rounds.len(), 1);
    assert!(turn.results.is_empty());
    let history = chat.history();
    assert_valid(&history);
    assert_eq!(history[..committed.len()], committed[..]);
    assert_eq!(
        history[committed.len()..],
        [assistant(vec![text("First I'll check b.")])]
    );
}

// C10
#[test]
fn invalid_tool_input_is_resent_once_and_never_enters_history() {
    let invalid_round = || {
        reply()
            .text("Reading.")
            .invalid_tool_use("toolu_x", READ_FILE, "{\"path\": \"a.rs\", \"start")
            .stop(StopReason::ToolUse)
    };
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", LIST_FILES, list())
            .stop(StopReason::ToolUse),
        invalid_round(),
        // Valid JSON, but not read_file's input: a field is missing.
        reply()
            .tool_use("toolu_y", READ_FILE, json!({"path": "a.rs"}))
            .stop(StopReason::ToolUse),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Read a");
    answer_all(&mut chat);
    let committed = committed(&mut chat);
    let first = next(&mut chat, &provider);
    assert!(chat.needs_resend());
    assert!(chat.pending_calls().is_empty(), "nothing of it runs");
    assert_eq!(self::committed(&mut chat), committed, "never in history");
    let resent = next(&mut chat, &provider);
    assert_eq!(json_of(&resent), json_of(&first), "the identical request");
    // Invalid again: the turn fails, the earlier round in place.
    assert!(!chat.needs_resend());
    assert!(matches!(
        &chat.turns[0].state,
        TurnState::Failed(error) if error.kind == ErrorKind::Stream
    ));
    assert_eq!(chat.history(), committed);
    assert_eq!(provider.requests().len(), 3);

    // Invalid input fails the same way whatever the reply's stop reason,
    // when the reply didn't end inside it, and as arrays of the fields,
    // at the top or in an edit.
    let invalid_replies = [
        // Complete JSON, but not read_file's input, last at max_tokens: not
        // cut short, so invalid.
        reply()
            .text("Reading.")
            .tool_use("toolu_x", READ_FILE, json!({"path": "a.rs"}))
            .stop(StopReason::MaxTokens),
        reply()
            .invalid_tool_use("toolu_x", SEARCH, "{\"pattern\"")
            .text("More after it.")
            .stop(StopReason::MaxTokens),
        reply()
            .invalid_tool_use("toolu_x", SEARCH, "{\"pattern\"")
            .stop(StopReason::EndTurn),
        reply()
            .tool_use("toolu_x", READ_FILE, json!(["a.rs", null, null]))
            .stop(StopReason::ToolUse),
        reply()
            .tool_use(
                "toolu_x",
                PROPOSE_EDIT,
                json!({"path": "a.rs", "edits": [["a", "b"]]}),
            )
            .stop(StopReason::ToolUse),
    ];
    for invalid in invalid_replies {
        let provider = ScriptedProvider::new([invalid.clone(), invalid]);
        let mut chat = self::chat();
        let content = question("Go");
        chat.begin_turn_with(content, Vec::new(), &settings());
        let first = next(&mut chat, &provider);
        assert!(chat.needs_resend(), "{first:?}");
        assert!(chat.pending_calls().is_empty());
        let resent = next(&mut chat, &provider);
        assert_eq!(json_of(&resent), json_of(&first));
        assert!(matches!(chat.turns[0].state, TurnState::Failed(_)));
        assert!(chat.history().is_empty(), "never in history");
    }

    // Invalid once, then valid: only the valid reply enters.
    let provider = ScriptedProvider::new([
        invalid_round(),
        reply()
            .tool_use("toolu_ok", READ_FILE, read("a.rs"))
            .stop(StopReason::ToolUse),
    ]);
    let mut chat = self::chat();
    let first = send(&mut chat, &provider, "Read a");
    assert!(chat.needs_resend());
    let resent = next(&mut chat, &provider);
    assert_eq!(json_of(&resent), json_of(&first));
    assert_eq!(chat.pending_calls()[0].id, "toolu_ok");
    chat.stop();
    let history = json_of(&chat.history());
    assert!(!history.contains("toolu_x"));
    assert_valid(&chat.history());
}

// C11
#[test]
fn prior_committed_rounds_are_unchanged() {
    type End = fn(&mut Conversation, &ScriptedProvider);
    let ends: [(&str, Vec<ChatEvent>, End); 6] = [
        (
            "stop mid-stream",
            reply().partial_text("x").streaming(),
            |c, _| c.stop(),
        ),
        (
            "withdrawal mid-stream",
            reply().text("x").streaming(),
            |c, _| c.withdraw_tools(),
        ),
        (
            "refusal",
            reply().text("x").stop(StopReason::Refusal),
            |_, _| {},
        ),
        (
            "failure",
            reply().fail(ProviderError::new(ErrorKind::Network, "down")),
            |_, _| {},
        ),
        (
            "max_tokens",
            reply()
                .text("x")
                .tool_input("toolu_9", SEARCH, "{")
                .stop(StopReason::MaxTokens),
            |_, _| {},
        ),
        (
            "invalid input twice",
            reply()
                .invalid_tool_use("toolu_9", SEARCH, "{")
                .stop(StopReason::ToolUse),
            |c, p| {
                p.push(
                    reply()
                        .invalid_tool_use("toolu_9", SEARCH, "{")
                        .stop(StopReason::ToolUse),
                );
                next(c, p);
            },
        ),
    ];
    for (name, events, end) in ends {
        let provider = ScriptedProvider::new([
            reply()
                .thinking("a", "s1")
                .tool_use("toolu_1", READ_FILE, read("a.rs"))
                .stop(StopReason::ToolUse),
            reply()
                .tool_use("toolu_2", SEARCH, search("b"))
                .stop(StopReason::ToolUse),
            events,
        ]);
        let mut chat = chat();
        send(&mut chat, &provider, "Go");
        answer_all(&mut chat);
        next(&mut chat, &provider);
        answer_all(&mut chat);
        let committed: Vec<String> = committed(&mut chat).iter().map(json_of).collect();
        let rounds = chat.turns[0].rounds.clone();
        next(&mut chat, &provider);
        end(&mut chat, &provider);
        let after: Vec<String> = chat.history().iter().map(json_of).collect();
        assert_eq!(after[..committed.len()], committed[..], "{name}");
        assert_eq!(chat.turns[0].rounds[..2], rounds[..], "{name}");
        assert_valid(&chat.history());
    }
}

/// A turn whose round 1 ran its call, then round 2 streaming text and a
/// call's input, as the exact-request tests start.
fn mid_tool_input(provider: &ScriptedProvider, last: Vec<ChatEvent>) -> Conversation {
    provider.push(
        reply()
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .stop(StopReason::ToolUse),
    );
    provider.push(last);
    let mut chat = chat();
    send(&mut chat, provider, "Fix a.rs");
    answer_all(&mut chat);
    next(&mut chat, provider);
    chat
}

/// Round 2's events up to a `propose_edit` call whose input is streaming.
fn round_two() -> ReplyScript {
    reply()
        .thinking("fix", "sig2")
        .text("I'll change it.")
        .tool_input(
            "toolu_2",
            PROPOSE_EDIT,
            "{\"path\": \"a.rs\", \"edits\": [{\"old_",
        )
}

/// The messages of round 1, as every exact request begins.
fn round_one() -> Vec<ApiMessage> {
    vec![
        user(question("Fix a.rs")),
        assistant(vec![tool_use("toolu_1", READ_FILE, read("a.rs"))]),
        user(vec![result("toolu_1", "read_file ok", false)]),
    ]
}

// C12
#[test]
fn exact_requests_after_stop_withdrawal_and_max_tokens_mid_tool_input() {
    // Stop: round 2's finished text stays, its call and thinking don't.
    let provider = ScriptedProvider::new([]);
    let mut chat = mid_tool_input(&provider, round_two().streaming());
    chat.stop();
    provider.push(reply().text("Ok.").stop(StopReason::EndTurn));
    let request = send(&mut chat, &provider, "Stop there");
    let mut expected = round_one();
    expected.extend([
        assistant(vec![text("I'll change it.")]),
        user(question("Stop there")),
    ]);
    assert_eq!(request.messages, expected);
    assert_valid_request(&request);

    // Trust withdrawal: the same, then the removal last.
    let provider = ScriptedProvider::new([]);
    let mut chat = mid_tool_input(&provider, round_two().streaming());
    chat.withdraw_tools();
    provider.push(reply().text("Ok.").stop(StopReason::EndTurn));
    let request = send(&mut chat, &provider, "Go on");
    let names: Vec<String> = tool_set().into_iter().map(|tool| tool.name).collect();
    let mut expected = round_one();
    expected.extend([
        assistant(vec![text("I'll change it.")]),
        user(question("Go on")),
        ApiMessage::tool_removal(names.iter().map(String::as_str), Effort::Medium),
    ]);
    assert_eq!(request.messages, expected);
    assert_valid_request(&request);

    // max_tokens mid-input: the reply isn't committed. Its finished text
    // stays, cut off; no thinking, no tool_use and no results.
    let provider = ScriptedProvider::new([]);
    let cut = reply()
        .thinking("fix", "sig2")
        .text("I'll change it.")
        .invalid_tool_use(
            "toolu_2",
            PROPOSE_EDIT,
            "{\"path\": \"a.rs\", \"edits\": [{\"old_",
        )
        .stop(StopReason::MaxTokens);
    let mut chat = mid_tool_input(&provider, cut);
    assert!(chat.pending_calls().is_empty());
    assert_eq!(chat.turns[0].state, TurnState::Done);
    assert!(chat.turns[0].cut_off());
    provider.push(reply().text("Ok.").stop(StopReason::EndTurn));
    let request = send(&mut chat, &provider, "Shorter please");
    let mut expected = round_one();
    expected.extend([
        assistant(vec![text("I'll change it.")]),
        user(question("Shorter please")),
    ]);
    assert_eq!(request.messages, expected);
    assert_valid_request(&request);

    // And when the stream ends mid-input with no block stop: the same.
    let provider = ScriptedProvider::new([]);
    let mut chat = mid_tool_input(&provider, round_two().stop(StopReason::MaxTokens));
    assert!(chat.turns[0].cut_off());
    provider.push(reply().text("Ok.").stop(StopReason::EndTurn));
    let request = send(&mut chat, &provider, "Shorter please");
    assert_eq!(request.messages, expected);

    // A finished, valid call before the one cut off doesn't go back either,
    // and gets no result: the reply wasn't committed.
    let provider = ScriptedProvider::new([]);
    let cut = reply()
        .text("I'll change it.")
        .tool_use("toolu_2", READ_FILE, read("b.rs"))
        .invalid_tool_use("toolu_3", PROPOSE_EDIT, "{\"path\": \"a.rs\", \"ed")
        .stop(StopReason::MaxTokens);
    let mut chat = mid_tool_input(&provider, cut);
    assert!(chat.pending_calls().is_empty());
    assert_eq!(chat.turns[0].rounds.len(), 1, "nothing more committed");
    provider.push(reply().text("Ok.").stop(StopReason::EndTurn));
    let request = send(&mut chat, &provider, "Shorter please");
    assert_eq!(request.messages, expected);
    assert_valid_request(&request);
}

#[test]
fn the_tool_set_is_five_strict_eager_tools_sorted_by_name() {
    let tools = tool_set();
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(
        names,
        [LIST_FILES, PROPOSE_EDIT, READ_FILE, RUN_COMMAND, SEARCH]
    );
    fn strict(schema: &Value) {
        if schema["type"] == "object" {
            assert_eq!(schema["additionalProperties"], false);
            let properties = schema["properties"].as_object().unwrap();
            let mut keys: Vec<&str> = properties.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let required: Vec<&str> = (schema["required"].as_array().unwrap().iter())
                .map(|name| name.as_str().unwrap())
                .collect();
            assert_eq!(required, keys);
            properties.values().for_each(strict);
        }
        if let Some(items) = schema.get("items") {
            strict(items);
        }
    }
    for tool in &tools {
        assert!(tool.strict && tool.eager_input_streaming, "{}", tool.name);
        assert!(!tool.description.is_empty());
        strict(&tool.input_schema);
    }
    // Constant: the same bytes every time.
    assert_eq!(json_of(&tool_set()), json_of(&tools));

    // Inputs validate strictly.
    assert!(matches!(
        ToolInput::parse(READ_FILE, &read("a")),
        Ok(ToolInput::ReadFile(_))
    ));
    assert!(
        ToolInput::parse(READ_FILE, &json!({"path": "a"})).is_err(),
        "nulls must be given"
    );
    let mut extra = read("a");
    extra["mode"] = json!("raw");
    assert!(
        ToolInput::parse(READ_FILE, &extra).is_err(),
        "no unknown fields"
    );
    assert!(ToolInput::parse(PROPOSE_EDIT, &json!({"path": "a", "edits": []})).is_err());
    assert!(ToolInput::parse(PROPOSE_EDIT, &edit("a")).is_ok());
    assert!(ToolInput::parse(SEARCH, &Value::String("{\"pattern".into())).is_err());
    assert!(
        ToolInput::parse(
            RUN_COMMAND,
            &json!({"program": "ls", "args": "-l", "cwd": null})
        )
        .is_err()
    );
    assert!(ToolInput::parse(LIST_FILES, &list()).is_ok());
    // Never arrays of the fields, at the top or in an edit.
    assert!(ToolInput::parse(READ_FILE, &json!(["a.rs", null, null])).is_err());
    assert!(ToolInput::parse(LIST_FILES, &json!([null, null])).is_err());
    let nested = json!({"path": "a.rs", "edits": [["a", "b"]]});
    assert!(ToolInput::parse(PROPOSE_EDIT, &nested).is_err());
}

#[test]
fn tools_and_the_task_budget_freeze_at_the_first_send() {
    let mut chat = Conversation::new(&settings());
    assert!(chat.set_tools(tool_set(), 5_000));
    assert_eq!(
        chat.task_budget,
        Some(MIN_TASK_BUDGET),
        "raised to the minimum"
    );
    assert!(chat.set_tools(tool_set(), 0));
    assert_eq!(chat.task_budget, None);
    let provider = ScriptedProvider::new([reply().text("Hi.").stop(StopReason::EndTurn)]);
    send(&mut chat, &provider, "Hi");
    assert!(!chat.set_tools(Vec::new(), 40_000), "frozen once sent");
    assert_eq!(chat.tools, tool_set());

    // A chat without tools stays without them; a call is answered, never
    // run.
    let mut plain = Conversation::new(&settings());
    let provider = ScriptedProvider::new([
        reply().text("Hi.").stop(StopReason::EndTurn),
        reply()
            .tool_use("toolu_1", READ_FILE, read("a"))
            .stop(StopReason::ToolUse),
    ]);
    let first = send(&mut plain, &provider, "Hi");
    assert!(first.tools.is_empty() && first.task_budget.is_none());
    assert!(!plain.set_tools(tool_set(), 0));
    send(&mut plain, &provider, "Read a");
    assert!(plain.pending_calls().is_empty());
    assert_eq!(plain.turns[1].state, TurnState::Done);
    assert_valid(&plain.history());
}

#[test]
fn chats_saved_before_rounds_load_with_their_one_reply() {
    let saved = json!({
        "id": "00000000000000000000000001",
        "title": "Old",
        "model": MODEL,
        "effort": "medium",
        "show_thinking": false,
        "turns": [{
            "user_content": [{"text": "Old question", "type": "text"}],
            "attachments": [],
            "reply": [{
                "index": 0,
                "kind": "text",
                "text": "Old answer.",
                "block": {"text": "Old answer.", "type": "text"},
            }],
            "model": MODEL,
            "stop_reason": "end_turn",
            "stop_details": null,
            "usage": null,
            "state": "done",
        }],
    });
    let chat: Conversation = serde_json::from_value(saved).unwrap();
    let turn = &chat.turns[0];
    assert!(turn.rounds.is_empty() && turn.results.is_empty() && !turn.removes_tools);
    assert!(chat.tools.is_empty() && chat.task_budget.is_none() && !chat.tools_withdrawn);
    assert_eq!(
        chat.history(),
        [
            user(vec![text("Old question")]),
            assistant(vec![text("Old answer.")])
        ]
    );

    // A chat with rounds round-trips.
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", READ_FILE, read("a"))
            .stop(StopReason::ToolUse),
        reply().text("Done.").stop(StopReason::EndTurn),
    ]);
    let mut chat = self::chat();
    send(&mut chat, &provider, "Go");
    answer_all(&mut chat);
    next(&mut chat, &provider);
    let read: Conversation = serde_json::from_str(&json_of(&chat)).unwrap();
    assert_eq!(read, chat);
    assert_eq!(json_of(&read.history()), json_of(&chat.history()));
}

#[test]
fn an_effort_change_keeps_the_results_message_and_its_text_apart() {
    let mut settings = settings();
    settings.effort_message_models = &[MODEL];
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", READ_FILE, read("a"))
            .stop(StopReason::ToolUse),
        reply().text("Ok.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    let content = question("Go");
    let request = build_request(&chat, content.clone(), &settings);
    chat.begin_turn_with(content, Vec::new(), &settings);
    play(&mut chat, stream(&provider, request));
    chat.stop();
    chat.set_effort(Effort::High);
    let request = build_request(&chat, question("Harder"), &settings);
    let roles: Vec<&Role> = request.messages.iter().map(|m| &m.role).collect();
    assert_eq!(
        roles,
        [
            &Role::User,
            &Role::Assistant,
            &Role::User,
            &Role::System {
                effort: Effort::High
            },
            &Role::User
        ]
    );
    assert_eq!(request.messages[2].content.len(), 1, "the results alone");
}

#[test]
fn a_resend_or_retry_keeps_the_rounds_request_settings() {
    let mut chat = chat();
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .stop(StopReason::ToolUse),
        reply().fail(ProviderError::new(ErrorKind::Server, "500")),
        reply()
            .invalid_tool_use("toolu_2", SEARCH, "{")
            .stop(StopReason::ToolUse),
        reply().text("Done.").stop(StopReason::EndTurn),
    ]);
    let content = question("Go");
    let built = build_request(&chat, content.clone(), &settings());
    chat.begin_turn_with(content, Vec::new(), &settings());
    let first = round_request(&mut chat, &settings()).unwrap();
    assert_eq!(
        json_of(&first),
        json_of(&built),
        "round 1 is build_request's"
    );
    play(&mut chat, stream(&provider, first));
    answer_all(&mut chat);
    let failed = next(&mut chat, &provider);

    // The settings change before Retry; the retried request doesn't.
    let mut changed = settings();
    changed.max_tokens = 1_000;
    changed.effort_message_models = &[MODEL];
    assert!(chat.retry_round());
    let retried = round_request(&mut chat, &changed).unwrap();
    assert_eq!(json_of(&retried), json_of(&failed));
    play(&mut chat, stream(&provider, retried.clone()));
    assert!(chat.needs_resend());
    let resent = round_request(&mut chat, &changed).unwrap();
    assert_eq!(json_of(&resent), json_of(&failed));
    assert_eq!(resent.max_tokens, settings().max_tokens);
    play(&mut chat, stream(&provider, resent));
    assert_eq!(chat.turns[0].state, TurnState::Done);
    assert_eq!(provider.requests().len(), 4);

    // A first round sent from build_request: the settings change right
    // after it fails, and its Retry is still the same request.
    let provider = ScriptedProvider::new([
        reply().fail(ProviderError::new(ErrorKind::Server, "500")),
        reply().text("Ok.").stop(StopReason::EndTurn),
    ]);
    let mut chat = self::chat();
    let first = send(&mut chat, &provider, "Go");
    assert!(matches!(chat.turns[0].state, TurnState::Failed(_)));
    assert!(chat.retry_round());
    let retried = round_request(&mut chat, &changed).unwrap();
    assert_eq!(json_of(&retried), json_of(&first));

    // And before a first round's resend for invalid input.
    let provider = ScriptedProvider::new([reply()
        .invalid_tool_use("toolu_1", SEARCH, "{")
        .stop(StopReason::ToolUse)]);
    let mut chat = self::chat();
    let first = send(&mut chat, &provider, "Go");
    assert!(chat.needs_resend());
    let resent = round_request(&mut chat, &changed).unwrap();
    assert_eq!(json_of(&resent), json_of(&first));
    assert_eq!(resent.max_tokens, settings().max_tokens);
}

#[test]
fn a_round_from_before_a_withdrawal_isnt_retried() {
    let provider = ScriptedProvider::new([
        reply()
            .tool_use("toolu_1", READ_FILE, read("a.rs"))
            .stop(StopReason::ToolUse),
        reply().fail(ProviderError::new(ErrorKind::Server, "500")),
        reply().fail(ProviderError::new(ErrorKind::Server, "500")),
        reply().text("Ok.").stop(StopReason::EndTurn),
    ]);
    let mut chat = chat();
    send(&mut chat, &provider, "Go");
    answer_all(&mut chat);
    next(&mut chat, &provider);
    assert!(matches!(chat.turns[0].state, TurnState::Failed(_)));
    chat.withdraw_tools();
    // Its request had no tool_removal: the next send carries it instead.
    assert!(!chat.retry_round());
    assert!(chat.removal_pending());
    let content = question("Again");
    let request = build_request(&chat, content.clone(), &settings());
    chat.begin_turn_with(content, Vec::new(), &settings());
    play(&mut chat, stream(&provider, request.clone()));
    assert!(!request.messages.last().unwrap().removed_tools().is_empty());
    assert!(matches!(chat.turns[1].state, TurnState::Failed(_)));

    // That turn's request carried the removal, so its Retry is allowed
    // and identical.
    assert!(chat.retry_round());
    let retried = next(&mut chat, &provider);
    assert_eq!(json_of(&retried), json_of(&request));
    assert_eq!(chat.turns[1].state, TurnState::Done);
    assert_valid(&chat.history());
}
