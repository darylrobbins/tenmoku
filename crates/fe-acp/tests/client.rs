//! fe-acp's client against an in-process fake agent over `Channel::duplex()`
//! (decision record § 12): connection, initialize, new, load, prompt, cancel,
//! set config option, set mode, the permission handler, and `AcpEvent` from
//! `SessionUpdate`.
//!
//! The fake is the crate's own `Agent.v1()` with a handler per method. It
//! records what it receives as JSON and plays a behaviour chosen by the
//! prompt's text. Requests it makes of the client (permission requests, an
//! `fs/read_text_file`) run in spawned tasks, never in a handler, as the
//! crate's ordering rule requires.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{Agent, Client, UntypedMessage};
use fe_acp::schema::{
    AgentCapabilities, AvailableCommandsUpdate, CancelNotification, ContentBlock, ContentChunk,
    CurrentModeUpdate, Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, Notice, NoticeSeverity,
    PermissionOption, PermissionOptionKind, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    PromptRequest, PromptResponse, ReadTextFileRequest, RequestPermissionRequest,
    SessionConfigOption, SessionConfigSelectOption, SessionConfigValueId, SessionId,
    SessionInfoUpdate, SessionMode, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, StopReason, ToolCall, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind, UsageUpdate,
};
use fe_acp::{AcpClient, AcpEvent, Channel, ErrorCode, Incoming, PermissionRequest};
use futures::channel::{mpsc, oneshot};
use futures::{FutureExt as _, StreamExt as _};
use serde_json::{Value, json};

const SESSION: &str = "s1";
const TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// The fake agent

/// What the fake received, in order: `(method, params)` for requests and
/// notifications, and `("answer:<tool call>", outcome)` for each answer to
/// a permission request it made.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<(String, Value)>>>);

impl Log {
    fn push(&self, method: &str, params: Value) {
        self.0.lock().unwrap().push((method.to_owned(), params));
    }

    /// The params of the only entry named `method`.
    fn only(&self, method: &str) -> Value {
        let entries = self.0.lock().unwrap();
        let found: Vec<_> = entries.iter().filter(|(m, _)| m == method).collect();
        assert_eq!(
            found.len(),
            1,
            "one {method} in {:?}",
            self.methods_of(&entries)
        );
        found[0].1.clone()
    }

    fn methods_of(&self, entries: &[(String, Value)]) -> Vec<String> {
        entries.iter().map(|(m, _)| m.clone()).collect()
    }
}

fn record<T: serde::Serialize>(log: &Log, method: &str, params: &T) {
    log.push(method, serde_json::to_value(params).unwrap());
}

fn text(s: &str) -> ContentBlock {
    ContentBlock::from(s)
}

fn chunk(s: &str) -> ContentChunk {
    ContentChunk::new(text(s))
}

fn update(update: SessionUpdate) -> SessionNotification {
    SessionNotification::new(SESSION, update)
}

fn config_options(model: &str) -> Vec<SessionConfigOption> {
    vec![SessionConfigOption::select(
        "model",
        "Model",
        model.to_owned(),
        vec![
            SessionConfigSelectOption::new("sonnet", "Sonnet"),
            SessionConfigSelectOption::new("opus", "Opus"),
        ],
    )]
}

fn modes() -> SessionModeState {
    SessionModeState::new(
        "default",
        vec![
            SessionMode::new("default", "Manual"),
            SessionMode::new("acceptEdits", "Accept edits"),
            SessionMode::new("plan", "Plan"),
        ],
    )
}

fn permission_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new("allow-once", "Allow", PermissionOptionKind::AllowOnce),
        PermissionOption::new(
            "allow-with-updates",
            "Always allow",
            PermissionOptionKind::AllowAlways,
        ),
        PermissionOption::new("reject-once", "Reject", PermissionOptionKind::RejectOnce),
    ]
}

/// The tool call the fake asks about, by id.
fn asked(id: &str) -> ToolCallUpdate {
    ToolCallUpdate::new(
        id.to_owned(),
        ToolCallUpdateFields::new()
            .title(format!("Run {id}"))
            .kind(ToolKind::Execute)
            .raw_input(json!({ "command": id })),
    )
}

/// Sends a permission request for `tool` and records the answer, or the
/// error, under `answer:<tool>`.
async fn ask(cx: &ConnectionTo<Client>, log: &Log, tool: &str) -> Result<(), fe_acp::Error> {
    let request = RequestPermissionRequest::new(SESSION, asked(tool), permission_options());
    match cx.send_request(request).block_task().await {
        Ok(response) => record(log, &format!("answer:{tool}"), &response.outcome),
        Err(err) => log.push(
            &format!("answer:{tool}"),
            json!({ "error": err.code.value() }),
        ),
    }
    Ok(())
}

/// The updates the `updates` prompt sends, in order, with the events the
/// client must hand out for them (`None` for the ones it ignores).
fn scripted_updates() -> Vec<(SessionUpdate, Option<AcpEvent>)> {
    let call = ToolCall::new("t1", "Read src/main.go")
        .kind(ToolKind::Read)
        .status(ToolCallStatus::Pending);
    let done = ToolCallUpdate::new(
        "t1",
        ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
    );
    let plan = Plan::new(vec![PlanEntry::new(
        "Read the file",
        PlanEntryPriority::High,
        PlanEntryStatus::InProgress,
    )]);
    vec![
        (
            SessionUpdate::AgentThoughtChunk(chunk("thinking")),
            Some(AcpEvent::AgentThought(text("thinking"))),
        ),
        (
            SessionUpdate::AgentMessageChunk(chunk("Hello")),
            Some(AcpEvent::AgentMessage(text("Hello"))),
        ),
        (
            SessionUpdate::ToolCall(call.clone()),
            Some(AcpEvent::ToolCall(call)),
        ),
        (
            SessionUpdate::ToolCallUpdate(done.clone()),
            Some(AcpEvent::ToolCallUpdate(done)),
        ),
        (
            SessionUpdate::Plan(plan.clone()),
            Some(AcpEvent::Plan(plan)),
        ),
        (
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![])),
            None,
        ),
        (SessionUpdate::UsageUpdate(UsageUpdate::new(10, 100)), None),
        (
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title("A title")),
            Some(AcpEvent::TitleChanged(Some("A title".into()))),
        ),
        (
            SessionUpdate::ConfigOptionUpdate(fe_acp::schema::ConfigOptionUpdate::new(
                config_options("opus"),
            )),
            Some(AcpEvent::ConfigOptions(config_options("opus"))),
        ),
        (
            SessionUpdate::AgentMessageChunk(chunk(" world")),
            Some(AcpEvent::AgentMessage(text(" world"))),
        ),
    ]
}

/// How the fake's `initialize` and `session/new` answer.
#[derive(Clone, Copy, Default)]
struct Script {
    /// `session/new` fails with `authRequired`, as the adapter does when
    /// `claude` isn't logged in.
    auth_required: bool,
}

/// A one-shot signal between the fake's handlers and its turns.
struct Gate {
    tx: Mutex<Option<oneshot::Sender<()>>>,
    rx: Mutex<Option<oneshot::Receiver<()>>>,
}

impl Default for Gate {
    fn default() -> Self {
        let (tx, rx) = oneshot::channel();
        Self {
            tx: Mutex::new(Some(tx)),
            rx: Mutex::new(Some(rx)),
        }
    }
}

impl Gate {
    fn open(&self) {
        if let Some(tx) = self.tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    async fn wait(&self) {
        let rx = self.rx.lock().unwrap().take();
        if let Some(rx) = rx {
            let _ = rx.await;
        }
    }
}

#[derive(Default)]
struct Gates {
    /// Opened by the first `session/cancel`.
    cancel: Gate,
    /// Opened when the `queued` prompt arrives.
    second: Gate,
    /// Opened when the `slow-cancel` turn ends.
    first_done: Gate,
}

/// Runs the fake agent on `channel` until the client's side closes.
async fn fake_agent(channel: Channel, log: Log, script: Script) -> Result<(), fe_acp::Error> {
    let gates = Arc::new(Gates::default());
    let cancel_gates = gates.clone();

    Agent
        .v1()
        .name("fake-agent")
        .on_receive_request(
            {
                let log = log.clone();
                async move |request: InitializeRequest, responder, _cx| {
                    record(&log, "initialize", &request);
                    responder.respond(
                        InitializeResponse::new(request.protocol_version)
                            .agent_capabilities(AgentCapabilities::new().load_session(true))
                            .agent_info(Implementation::new("claude-agent-acp", "0.106.0")),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let log = log.clone();
                async move |request: NewSessionRequest, responder, _cx| {
                    record(&log, "session/new", &request);
                    if script.auth_required {
                        return responder.respond_with_error(fe_acp::Error::auth_required());
                    }
                    responder.respond(
                        NewSessionResponse::new(SESSION)
                            .modes(modes())
                            .config_options(config_options("sonnet")),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let log = log.clone();
                async move |request: LoadSessionRequest, responder, cx| {
                    record(&log, "session/load", &request);
                    // The replay, before the response.
                    cx.send_notification(update(SessionUpdate::UserMessageChunk(chunk(
                        "What is 2 + 2?",
                    ))))?;
                    cx.send_notification(update(SessionUpdate::AgentMessageChunk(chunk("4"))))?;
                    responder.respond(
                        LoadSessionResponse::new()
                            .modes(modes())
                            .config_options(config_options("sonnet")),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let log = log.clone();
                async move |request: SetSessionConfigOptionRequest, responder, _cx| {
                    record(&log, "session/set_config_option", &request);
                    let params = serde_json::to_value(&request).unwrap();
                    let model = params["value"].as_str().unwrap_or("sonnet").to_owned();
                    responder.respond(SetSessionConfigOptionResponse::new(config_options(&model)))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let log = log.clone();
                async move |request: SetSessionModeRequest, responder, cx| {
                    record(&log, "session/set_mode", &request);
                    cx.send_notification(update(SessionUpdate::CurrentModeUpdate(
                        CurrentModeUpdate::new(request.mode_id.clone()),
                    )))?;
                    responder.respond(SetSessionModeResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let log = log.clone();
                async move |notification: CancelNotification, _cx| {
                    record(&log, "session/cancel", &notification);
                    cancel_gates.cancel.open();
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let log = log.clone();
                async move |request: PromptRequest, responder, cx| {
                    record(&log, "session/prompt", &request);
                    let behaviour = match request.prompt.first() {
                        Some(ContentBlock::Text(t)) => t.text.clone(),
                        _ => String::new(),
                    };
                    let log = log.clone();
                    let gates = gates.clone();
                    let task_cx = cx.clone();
                    cx.spawn(async move {
                        let cx = task_cx;
                        let stop = match behaviour.as_str() {
                            "updates" => {
                                for (u, _) in scripted_updates() {
                                    cx.send_notification(update(u))?;
                                }
                                // A variant this schema doesn't know: the
                                // client drops it and carries on.
                                cx.send_notification(UntypedMessage::new(
                                    "session/update",
                                    json!({
                                        "sessionId": SESSION,
                                        "update": { "sessionUpdate": "brand_new_update" },
                                    }),
                                )?)?;
                                cx.send_notification(update(SessionUpdate::AgentMessageChunk(
                                    chunk("!"),
                                )))?;
                                StopReason::EndTurn
                            }
                            "permission" => {
                                cx.send_notification(update(SessionUpdate::ToolCall(
                                    ToolCall::new("bash-1", "Run bash-1").kind(ToolKind::Execute),
                                )))?;
                                ask(&cx, &log, "bash-1").await?;
                                cx.send_notification(update(SessionUpdate::AgentMessageChunk(
                                    chunk("done"),
                                )))?;
                                StopReason::EndTurn
                            }
                            "wait-for-cancel" => {
                                // One request left unanswered when the cancel
                                // comes, in a task of its own.
                                let first_cx = cx.clone();
                                let first_log = log.clone();
                                cx.spawn(async move { ask(&first_cx, &first_log, "first").await })?;
                                gates.cancel.wait().await;
                                // A request after the cancel, before the turn
                                // ends: answered without reaching the user.
                                ask(&cx, &log, "after-cancel").await?;
                                StopReason::Cancelled
                            }
                            "slow-cancel" => {
                                // The turn goes on after the cancel until a
                                // second prompt is queued, then asks once
                                // more before it ends.
                                gates.cancel.wait().await;
                                gates.second.wait().await;
                                ask(&cx, &log, "late").await?;
                                gates.first_done.open();
                                StopReason::Cancelled
                            }
                            "queued" => {
                                // Queued behind the turn running: it starts
                                // once that turn has ended.
                                gates.second.open();
                                gates.first_done.wait().await;
                                StopReason::EndTurn
                            }
                            "read-file" => {
                                let read = cx
                                    .send_request(ReadTextFileRequest::new(
                                        SESSION,
                                        "/etc/hostname",
                                    ))
                                    .block_task()
                                    .await;
                                let code = read.err().map(|e| e.code.value());
                                log.push("read-file", json!({ "error": code }));
                                StopReason::EndTurn
                            }
                            _ => StopReason::EndTurn,
                        };
                        responder.respond(PromptResponse::new(stop))
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(channel)
        .await
}

// ---------------------------------------------------------------------------
// The harness

struct Harness {
    client: AcpClient,
    incoming: mpsc::UnboundedReceiver<Incoming>,
    log: Log,
    wire: Wire,
}

/// Every frame between the client and the fake, in the order it crossed:
/// `(true, message)` from the client, `(false, message)` from the agent.
#[derive(Clone, Default)]
struct Wire(Arc<Mutex<Vec<(bool, Value)>>>);

impl Wire {
    fn frames(&self) -> Vec<(bool, Value)> {
        self.0.lock().unwrap().clone()
    }

    /// The client's `(position, outcome)` answers to the agent's permission
    /// requests, in the order the agent asked, with the tool call each was
    /// for.
    fn answers(&self) -> Vec<(String, usize, Value)> {
        let frames = self.frames();
        frames
            .iter()
            .filter(|(up, m)| !up && m["method"] == "session/request_permission")
            .map(|(_, request)| {
                let tool = request["params"]["toolCall"]["toolCallId"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let answers: Vec<_> = frames
                    .iter()
                    .enumerate()
                    .filter(|(_, (up, m))| {
                        *up && m.get("method").is_none() && m["id"] == request["id"]
                    })
                    .collect();
                assert_eq!(answers.len(), 1, "one answer to {request}");
                let (position, (_, answer)) = answers[0];
                (
                    tool,
                    position,
                    answer["result"]["outcome"]["outcome"].clone(),
                )
            })
            .collect()
    }

    /// The positions of the client's `session/cancel` notifications.
    fn cancels(&self) -> Vec<usize> {
        self.frames()
            .iter()
            .enumerate()
            .filter(|(_, (up, m))| *up && m["method"] == "session/cancel")
            .map(|(i, _)| i)
            .collect()
    }
}

/// Relays frames between the client's and the agent's channels, recording
/// them on `wire`. Each direction ends when its sender closes.
async fn tap(client: Channel, agent: Channel, wire: Wire) {
    let (mut from_client, to_client) = client.into_parts();
    let (mut from_agent, to_agent) = agent.into_parts();
    let up_wire = wire.clone();
    let up = async move {
        while let Some(frame) = from_client.next().await {
            let json: Value = serde_json::from_str(&frame.to_json().unwrap()).unwrap();
            up_wire.0.lock().unwrap().push((true, json));
            if to_agent.unbounded_send(frame).is_err() {
                break;
            }
        }
    };
    let down = async move {
        while let Some(frame) = from_agent.next().await {
            let json: Value = serde_json::from_str(&frame.to_json().unwrap()).unwrap();
            wire.0.lock().unwrap().push((false, json));
            if to_client.unbounded_send(frame).is_err() {
                break;
            }
        }
    };
    futures::join!(up, down);
}

impl Harness {
    /// The next incoming message.
    async fn next(&mut self) -> Incoming {
        self.incoming
            .next()
            .await
            .expect("the incoming stream ended")
    }

    /// Everything already on the stream, without waiting.
    fn drain(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        while let Ok(message) = self.incoming.try_recv() {
            out.push(message);
        }
        out
    }

    /// The events already on the stream, which must all be updates for the
    /// session.
    fn drain_events(&mut self) -> Vec<AcpEvent> {
        self.drain()
            .into_iter()
            .map(|message| match message {
                Incoming::Update { session_id, event } => {
                    assert_eq!(session_id, SessionId::new(SESSION));
                    event
                }
                Incoming::Permission(request) => {
                    panic!("unexpected permission request {:?}", request.request())
                }
            })
            .collect()
    }

    async fn initialized(&self) {
        self.client.initialize().await.expect("initialize");
    }

    async fn new_session(&self) -> SessionId {
        self.initialized().await;
        self.client
            .new_session("/work/lane-1", None)
            .await
            .expect("session/new")
            .session_id
    }
}

/// Runs `body` against the fake, with both connections driven alongside
/// it. Dropping the harness at the end of `body` closes the connection, and
/// the test waits for both sides to finish.
fn run<F>(script: Script, body: impl FnOnce(Harness) -> F)
where
    F: Future<Output = ()>,
{
    let (client_end, client_tap) = Channel::duplex();
    let (agent_tap, agent_end) = Channel::duplex();
    let log = Log::default();
    let wire = Wire::default();
    let relay = tap(client_tap, agent_tap, wire.clone());
    let agent = fake_agent(agent_end, log.clone(), script);
    let (client, incoming, driver) = fe_acp::connect(client_end);
    let body = body(Harness {
        client,
        incoming,
        log,
        wire,
    });
    let all = async {
        let ((), driven, served, ()) = futures::join!(body, driver, agent, relay);
        driven.expect("the client's connection");
        served.expect("the fake agent's connection");
    };
    async_io::block_on(async {
        futures::select! {
            () = all.fuse() => {}
            _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
        }
    });
}

fn expect_permission(message: Incoming) -> PermissionRequest {
    match message {
        Incoming::Permission(request) => request,
        Incoming::Update { event, .. } => panic!("expected a permission request, got {event:?}"),
    }
}

// ---------------------------------------------------------------------------
// Connection

#[test]
fn closing_the_agent_side_ends_the_stream_and_fails_requests() {
    let (client_end, agent_end) = Channel::duplex();
    let (client, mut incoming, driver) = fe_acp::connect(client_end);
    drop(agent_end);
    async_io::block_on(async {
        futures::select! {
            r = driver.fuse() => r.expect("a closed transport ends the connection cleanly"),
            _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
        }
        assert!(incoming.next().await.is_none(), "the stream ends");
        assert!(client.initialize().await.is_err(), "requests fail");
    });
}

#[test]
fn shutdown_ends_the_connection() {
    run(Script::default(), async |h: Harness| {
        h.initialized().await;
        h.client.shutdown();
        assert!(
            h.client.new_session("/work", None).await.is_err(),
            "requests fail after shutdown"
        );
    });
}

#[test]
fn dropping_every_client_handle_ends_the_connection() {
    // `run` waits for both connection futures, which end only because the
    // body drops the client.
    run(Script::default(), async |h: Harness| {
        let second = h.client.clone();
        h.initialized().await;
        drop(second);
    });
}

// ---------------------------------------------------------------------------
// Initialize

#[test]
fn initialize_is_v1_with_no_file_system_terminal_or_login() {
    run(Script::default(), async |h: Harness| {
        let response = h.client.initialize().await.expect("initialize");
        assert_eq!(
            response.agent_info.map(|i| (i.name, i.version)),
            Some(("claude-agent-acp".into(), "0.106.0".into()))
        );
        assert!(response.agent_capabilities.load_session);

        // `auth.terminal: false` and no `_meta`, so the adapter offers no
        // login method (§ 2).
        assert_eq!(
            h.log.only("initialize"),
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false,
                    "auth": { "terminal": false },
                },
                "clientInfo": { "name": "tenmoku", "version": env!("CARGO_PKG_VERSION") },
            })
        );
    });
}

#[test]
fn the_file_system_methods_are_refused() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let response = h
            .client
            .prompt(session, vec![text("read-file")])
            .await
            .expect("prompt");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(
            h.log.only("read-file"),
            json!({ "error": ErrorCode::METHOD_NOT_FOUND.value() })
        );
        assert!(h.drain().is_empty());
    });
}

// ---------------------------------------------------------------------------
// New and load

#[test]
fn new_session_sends_the_cwd_and_meta_with_no_mcp_servers() {
    run(Script::default(), async |h: Harness| {
        h.initialized().await;
        let mut meta = serde_json::Map::new();
        meta.insert(
            "claudeCode".into(),
            json!({ "options": { "strictMcpConfig": true, "tools": ["Read"] } }),
        );
        let response = h
            .client
            .new_session("/work/lane-1", Some(meta))
            .await
            .expect("session/new");
        assert_eq!(response.session_id, SessionId::new(SESSION));
        assert_eq!(
            response.modes.map(|m| m.current_mode_id.to_string()),
            Some("default".into())
        );
        assert_eq!(response.config_options, Some(config_options("sonnet")));
        assert_eq!(
            h.log.only("session/new"),
            json!({
                "cwd": "/work/lane-1",
                "mcpServers": [],
                "_meta": {
                    "claudeCode": { "options": { "strictMcpConfig": true, "tools": ["Read"] } },
                },
            })
        );
    });
}

#[test]
fn auth_required_reaches_the_caller_with_its_code() {
    let script = Script {
        auth_required: true,
    };
    run(script, async |h: Harness| {
        h.initialized().await;
        let err = h
            .client
            .new_session("/work/lane-1", None)
            .await
            .expect_err("not logged in");
        assert_eq!(err.code, ErrorCode::AUTH_REQUIRED);
    });
}

#[test]
fn load_session_replays_the_history_before_it_returns() {
    run(Script::default(), async |mut h: Harness| {
        h.initialized().await;
        let mut meta = serde_json::Map::new();
        meta.insert("claudeCode".into(), json!({ "options": {} }));
        let response = h
            .client
            .load_session("saved-7", "/work/lane-1", Some(meta))
            .await
            .expect("session/load");
        assert_eq!(response.config_options, Some(config_options("sonnet")));
        assert_eq!(
            h.log.only("session/load"),
            json!({
                "mcpServers": [],
                "cwd": "/work/lane-1",
                "sessionId": "saved-7",
                "_meta": { "claudeCode": { "options": {} } },
            })
        );
        // Already on the stream: the dispatch loop handles the replay's
        // notifications before it routes the response.
        assert_eq!(
            h.drain_events(),
            vec![
                AcpEvent::UserMessage(text("What is 2 + 2?")),
                AcpEvent::AgentMessage(text("4")),
            ]
        );
    });
}

// ---------------------------------------------------------------------------
// Prompt, and AcpEvent from SessionUpdate

#[test]
fn a_prompt_streams_its_updates_as_events_and_ends_with_the_stop_reason() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let response = h
            .client
            .prompt(session, vec![text("updates")])
            .await
            .expect("prompt");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(
            h.log.only("session/prompt"),
            json!({
                "sessionId": SESSION,
                "prompt": [{ "type": "text", "text": "updates" }],
            })
        );

        // In order, the ignored ones (commands, usage, the unknown variant)
        // dropped, and the update after the unknown variant still delivered.
        let mut expected: Vec<AcpEvent> = scripted_updates()
            .into_iter()
            .filter_map(|(_, e)| e)
            .collect();
        expected.push(AcpEvent::AgentMessage(text("!")));
        assert_eq!(h.drain_events(), expected);
    });
}

#[test]
fn acp_event_from_each_session_update() {
    for (update, expected) in scripted_updates() {
        let event = AcpEvent::from(update.clone());
        match expected {
            Some(expected) => assert_eq!(event, expected),
            None => assert!(
                matches!(event, AcpEvent::Ignored(_)),
                "{update:?} → {event:?}"
            ),
        }
    }

    let cases = [
        (
            SessionUpdate::UserMessageChunk(chunk("hi")),
            AcpEvent::UserMessage(text("hi")),
        ),
        (
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("plan")),
            AcpEvent::ModeChanged("plan".into()),
        ),
        (
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(None::<String>)),
            AcpEvent::TitleChanged(None),
        ),
        (
            SessionUpdate::SessionInfoUpdate(
                SessionInfoUpdate::new().updated_at("2026-11-13T00:00:00Z"),
            ),
            AcpEvent::Ignored("session_info_update"),
        ),
        (
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![])),
            AcpEvent::Ignored("available_commands_update"),
        ),
        (
            SessionUpdate::UsageUpdate(UsageUpdate::new(1, 2)),
            AcpEvent::Ignored("usage_update"),
        ),
        (
            SessionUpdate::Notice(Notice::new(NoticeSeverity::Info, "note")),
            AcpEvent::Ignored("notice"),
        ),
    ];
    for (update, expected) in cases {
        assert_eq!(AcpEvent::from(update), expected);
    }

    assert_eq!(AcpEvent::AgentMessage(text("x")).chunk_text(), Some("x"));
    assert_eq!(AcpEvent::ModeChanged("plan".into()).chunk_text(), None);
}

// ---------------------------------------------------------------------------
// Permission requests

#[test]
fn a_permission_request_is_handed_out_after_its_tool_call_and_answered() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let client = h.client.clone();
        let prompt = client.prompt(session, vec![text("permission")]);
        let answer = async {
            match h.next().await {
                Incoming::Update {
                    event: AcpEvent::ToolCall(call),
                    ..
                } => assert_eq!(call.tool_call_id.to_string(), "bash-1"),
                other => panic!("expected the tool call first, got {other:?}"),
            }
            let request = expect_permission(h.next().await);
            assert_eq!(request.session_id(), &SessionId::new(SESSION));
            assert_eq!(request.tool_call(), &asked("bash-1"));
            assert_eq!(request.options(), permission_options().as_slice());
            assert!(request.is_pending());
            assert!(request.select("allow-once"));
        };
        let (response, ()) = futures::join!(prompt, answer);
        assert_eq!(response.expect("prompt").stop_reason, StopReason::EndTurn);
        assert_eq!(
            h.log.only("answer:bash-1"),
            json!({ "outcome": "selected", "optionId": "allow-once" })
        );
        assert_eq!(h.drain_events(), vec![AcpEvent::AgentMessage(text("done"))]);
    });
}

#[test]
fn a_dropped_permission_request_answers_cancelled() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let client = h.client.clone();
        let prompt = client.prompt(session, vec![text("permission")]);
        let answer = async {
            let _tool_call = h.next().await;
            drop(expect_permission(h.next().await));
        };
        let (response, ()) = futures::join!(prompt, answer);
        response.expect("prompt");
        assert_eq!(
            h.log.only("answer:bash-1"),
            json!({ "outcome": "cancelled" })
        );
    });
}

#[test]
fn an_option_the_request_did_not_offer_answers_cancelled() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let client = h.client.clone();
        let prompt = client.prompt(session, vec![text("permission")]);
        let answer = async {
            let _tool_call = h.next().await;
            let request = expect_permission(h.next().await);
            assert!(!request.select("allow-everything"));
        };
        let (response, ()) = futures::join!(prompt, answer);
        response.expect("prompt");
        assert_eq!(
            h.log.only("answer:bash-1"),
            json!({ "outcome": "cancelled" })
        );
    });
}

// ---------------------------------------------------------------------------
// Cancel

#[test]
fn cancel_sends_session_cancel_then_answers_pending_requests_cancelled() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let client = h.client.clone();
        let prompt = client.prompt(session.clone(), vec![text("wait-for-cancel")]);
        let stop = async {
            let request = expect_permission(h.next().await);
            assert_eq!(request.tool_call(), &asked("first"));
            h.client.cancel(session.clone()).await.expect("cancel");
            assert!(!request.is_pending(), "answered by the cancel");
            assert!(!request.select("allow-once"), "too late to allow");
        };
        let (response, ()) = futures::join!(prompt, stop);
        assert_eq!(response.expect("prompt").stop_reason, StopReason::Cancelled);

        assert_eq!(
            h.log.only("session/cancel"),
            json!({ "sessionId": SESSION })
        );
        assert_eq!(
            h.log.only("answer:first"),
            json!({ "outcome": "cancelled" })
        );
        // The request made after the cancel never reached the stream.
        assert_eq!(
            h.log.only("answer:after-cancel"),
            json!({ "outcome": "cancelled" })
        );
        assert!(h.drain().is_empty());

        // On the wire, both answers follow the cancel notification.
        let cancels = h.wire.cancels();
        assert_eq!(cancels.len(), 1);
        let answers = h.wire.answers();
        assert_eq!(answers.len(), 2);
        for (tool, at, outcome) in &answers {
            assert!(*at > cancels[0], "{tool} answered before the cancel");
            assert_eq!(outcome, "cancelled");
        }

        // Once the cancelled turn ended, the next prompt's requests reach
        // the stream.
        let client = h.client.clone();
        let prompt = client.prompt(session, vec![text("permission")]);
        let answer = async {
            let _tool_call = h.next().await;
            assert!(expect_permission(h.next().await).select("reject-once"));
        };
        let (response, ()) = futures::join!(prompt, answer);
        response.expect("prompt");
        assert_eq!(
            h.log.only("answer:bash-1"),
            json!({ "outcome": "selected", "optionId": "reject-once" })
        );
    });
}

/// An allow racing a cancel on two threads, many times over (review F1).
/// On the wire an allow is always before the `session/cancel`, and a
/// `Cancelled` from the cancel is always after it.
#[test]
fn an_allow_racing_a_cancel_never_lands_after_it() {
    const ROUNDS: usize = 40;
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let mut sent = Vec::new();
        for _ in 0..ROUNDS {
            let client = h.client.clone();
            let prompt = client.prompt(session.clone(), vec![text("permission")]);
            let race = async {
                let _tool_call = h.next().await;
                let request = expect_permission(h.next().await);
                let barrier = Arc::new(std::sync::Barrier::new(2));
                let canceller = {
                    let client = h.client.clone();
                    let session = session.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        async_io::block_on(client.cancel(session)).expect("cancel");
                    })
                };
                let allower = std::thread::spawn(move || {
                    barrier.wait();
                    request.select("allow-once")
                });
                // Neither thread needs the connection driven: it is up, and
                // both only queue frames.
                canceller.join().unwrap();
                sent.push(allower.join().unwrap());
            };
            let (response, ()) = futures::join!(prompt, race);
            response.expect("prompt");
            h.drain();
        }

        let answers = h.wire.answers();
        let cancels = h.wire.cancels();
        assert_eq!(answers.len(), ROUNDS);
        assert_eq!(cancels.len(), ROUNDS);
        for (round, ((_, at, outcome), cancel)) in answers.iter().zip(&cancels).enumerate() {
            if outcome == "selected" {
                assert!(at < cancel, "round {round}: an allow after the cancel");
                assert!(sent[round], "round {round}: select said it wasn't sent");
            } else {
                assert_eq!(outcome, "cancelled", "round {round}");
                assert!(at > cancel, "round {round}: Cancelled before the cancel");
                assert!(!sent[round], "round {round}: select said it was sent");
            }
        }
    });
}

/// A prompt sent after a cancel while the cancelled turn still runs (review
/// F2): the agent queues it, and a late request from the cancelled turn is
/// still answered `Cancelled` without reaching the stream. Once both turns
/// end, requests reach the stream again.
#[test]
fn a_prompt_queued_behind_a_cancelled_turn_does_not_reopen_its_requests() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        let client = h.client.clone();
        let first = client.prompt(session.clone(), vec![text("slow-cancel")]);
        let then = async {
            client.cancel(session.clone()).await.expect("cancel");
            client.prompt(session.clone(), vec![text("queued")]).await
        };
        let (first, second) = futures::join!(first, then);
        assert_eq!(
            first.expect("first prompt").stop_reason,
            StopReason::Cancelled
        );
        assert_eq!(
            second.expect("second prompt").stop_reason,
            StopReason::EndTurn
        );
        assert_eq!(h.log.only("answer:late"), json!({ "outcome": "cancelled" }));
        assert!(
            h.drain().is_empty(),
            "the late request never reached the stream"
        );
        let cancels = h.wire.cancels();
        let answers = h.wire.answers();
        assert_eq!((cancels.len(), answers.len()), (1, 1));
        assert!(answers[0].1 > cancels[0]);

        let prompt = client.prompt(session, vec![text("permission")]);
        let answer = async {
            let _tool_call = h.next().await;
            assert!(expect_permission(h.next().await).select("allow-once"));
        };
        let (response, ()) = futures::join!(prompt, answer);
        response.expect("prompt");
        assert_eq!(
            h.log.only("answer:bash-1"),
            json!({ "outcome": "selected", "optionId": "allow-once" })
        );
    });
}

// ---------------------------------------------------------------------------
// Config options and modes

#[test]
fn set_config_option_sends_the_value_and_returns_the_options() {
    run(Script::default(), async |h: Harness| {
        let session = h.new_session().await;
        let options = h
            .client
            .set_config_option(session, "model", SessionConfigValueId::new("opus"))
            .await
            .expect("set_config_option");
        assert_eq!(options, config_options("opus"));
        assert_eq!(
            h.log.only("session/set_config_option"),
            json!({ "sessionId": SESSION, "configId": "model", "value": "opus" })
        );
    });
}

#[test]
fn set_mode_sends_the_mode_and_its_update_arrives() {
    run(Script::default(), async |mut h: Harness| {
        let session = h.new_session().await;
        h.client.set_mode(session, "plan").await.expect("set_mode");
        assert_eq!(
            h.log.only("session/set_mode"),
            json!({ "sessionId": SESSION, "modeId": "plan" })
        );
        assert_eq!(h.drain_events(), vec![AcpEvent::ModeChanged("plan".into())]);
    });
}
