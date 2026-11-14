//! The fake ACP agent for fe-acp's tests (decision record § 12).
//!
//! It is the crate's own `Agent.v1()` playing a JSON [`Script`], given as
//! its argument: inline when it starts with `{`, else the path of a file.
//! The script sets:
//!
//! - the adapter version `initialize` reports, and an `_auth/status_update`
//!   to send;
//! - an error for `session/new` or `session/load` (`authRequired`, a
//!   sandbox that can't run);
//! - one [`Turn`] per prompt: updates to send, a permission request and the
//!   option it expects back, waiting for a cancel, an error, a stop reason,
//!   or exiting mid-turn;
//! - lines to write to stderr, a child to start in its process group, and
//!   whether it lingers after its stdin closes.
//!
//! It records what it received as JSON lines in the script's `record` file:
//! `{"method", "params"}` for each request and notification,
//! `"answer:<tool call>"` for each answer to a permission request, and
//! `"started"` (its environment and arguments) and `"child"` (the child's
//! pid) for the process tests.
//!
//! In-process tests run [`serve`] over `Channel::duplex()` (they include
//! this file as a module); process tests run the binary through
//! `CARGO_BIN_EXE_fe-acp-fake-agent`.

use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{Agent, Client, ConnectTo, Error, Lines, UntypedMessage};
use fe_acp::schema::{
    AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock,
    ContentChunk, Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptRequest, PromptResponse, RequestPermissionRequest,
    ResumeSessionRequest, ResumeSessionResponse, SessionNotification, SessionUpdate, StopReason,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt as _, StreamExt as _};
use serde::Deserialize;
use serde_json::{Value, json};

/// The session id the fake gives every new session.
pub(crate) const SESSION: &str = "fake-session";

/// What the fake plays.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Script {
    /// Where it records what it received, as JSON lines.
    pub(crate) record: Option<PathBuf>,
    /// `agentInfo.version` (by default `0.106.0`); `null` sends no
    /// `agentInfo`.
    pub(crate) version: Option<String>,
    /// Sends `_auth/status_update` with this kind (and an email, which the
    /// client must not keep) before answering `session/new`.
    pub(crate) auth_status: Option<String>,
    /// `session/new` fails with this.
    pub(crate) new_error: Option<ScriptError>,
    /// `session/load` fails with this.
    pub(crate) load_error: Option<ScriptError>,
    /// `session/load` waits until this file exists before it answers.
    pub(crate) load_gate: Option<PathBuf>,
    /// `(byte, count)`: written to stderr at start, first, with no newline.
    pub(crate) stderr_fill: Option<(u8, usize)>,
    /// Raw bytes written to stderr at start, after the fill.
    pub(crate) stderr_bytes: Vec<u8>,
    /// Lines written to stderr at start, last.
    pub(crate) stderr: Vec<String>,
    /// Starts `sleep 300` in its process group, and records its pid.
    pub(crate) spawn_child: bool,
    /// Keeps running after its stdin closes, until killed.
    pub(crate) linger: bool,
    /// One per prompt, in order; prompts past the end end their turn at
    /// once.
    pub(crate) turns: Vec<Turn>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            record: None,
            version: Some("0.106.0".into()),
            auth_status: None,
            new_error: None,
            load_error: None,
            load_gate: None,
            stderr_fill: None,
            stderr_bytes: Vec::new(),
            stderr: Vec::new(),
            spawn_child: false,
            linger: false,
            turns: Vec::new(),
        }
    }
}

/// A JSON-RPC error the script answers with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScriptError {
    /// Its code: `-32000` is `authRequired`.
    pub(crate) code: i32,
    /// Its message.
    pub(crate) message: String,
}

/// One turn: what a prompt does, in this order.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Turn {
    /// Agent message chunks to send.
    pub(crate) updates: Vec<String>,
    /// Exits the process with this code, after the updates and a line on
    /// stderr.
    pub(crate) exit: Option<i32>,
    /// Fails the prompt with this.
    pub(crate) error: Option<ScriptError>,
    /// Closes the process's stdout (after the updates and a line on
    /// stderr) and never answers: the transport fails while the process
    /// stays alive.
    pub(crate) hang_up: bool,
    /// Asks permission for this tool call, and records the answer.
    pub(crate) permission: Option<String>,
    /// The option the permission request expects back; another answer is
    /// recorded as `"unexpected:<tool call>"`.
    pub(crate) expect: Option<String>,
    /// Before anything else, waits for the previous turn's response.
    pub(crate) after_previous: bool,
    /// Waits for `session/cancel`, then ends with the `cancelled` stop
    /// reason. The permission request, if any, stays unanswered meanwhile.
    pub(crate) wait_for_cancel: bool,
    /// After the cancel, waits for the next prompt to arrive, then asks
    /// permission for this tool call before the turn ends.
    pub(crate) late_permission: Option<String>,
    /// The stop reason: `end_turn` (the default), `max_tokens`,
    /// `refusal`, `cancelled`.
    pub(crate) stop: Option<String>,
}

/// Appends what the fake received to the record file.
#[derive(Clone)]
pub(crate) struct Recorder(Option<Arc<Mutex<File>>>);

impl Recorder {
    fn open(path: Option<&PathBuf>) -> Self {
        Self(path.map(|path| {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap_or_else(|err| panic!("opening {}: {err}", path.display()));
            Arc::new(Mutex::new(file))
        }))
    }

    pub(crate) fn push(&self, method: &str, params: Value) {
        let Some(file) = &self.0 else {
            return;
        };
        let line = json!({ "method": method, "params": params });
        let mut file = file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }

    fn record<T: serde::Serialize>(&self, method: &str, params: &T) {
        self.push(method, serde_json::to_value(params).unwrap_or(Value::Null));
    }
}

/// Counts events (cancels, prompts arrived, turns answered), and wakes
/// turns waiting for a count.
#[derive(Default)]
struct Counter {
    count: AtomicUsize,
    waiting: Mutex<Vec<(usize, oneshot::Sender<()>)>>,
}

impl Counter {
    fn get(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    /// Counts one, and returns the count before it.
    fn bump(&self) -> usize {
        let mut waiting = self.waiting.lock().unwrap();
        let before = self.count.fetch_add(1, Ordering::SeqCst);
        let now = before + 1;
        let (ready, left) = std::mem::take(&mut *waiting)
            .into_iter()
            .partition(|(at_least, _)| *at_least <= now);
        *waiting = left;
        for (_, waiter) in ready {
            let _ = waiter.send(());
        }
        before
    }

    /// Waits until the count is at least `n`.
    async fn at_least(&self, n: usize) {
        let (tx, rx) = oneshot::channel();
        {
            let mut waiting = self.waiting.lock().unwrap();
            if self.get() >= n {
                return;
            }
            waiting.push((n, tx));
        }
        let _ = rx.await;
    }
}

#[derive(Default)]
struct Counters {
    cancels: Counter,
    prompts: Counter,
    answered: Counter,
}

fn script_error(error: &ScriptError) -> Error {
    Error::new(error.code, error.message.clone())
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

/// Asks permission for `tool` and records the answer.
async fn ask(
    cx: &ConnectionTo<Client>,
    recorder: &Recorder,
    session: &str,
    tool: &str,
    expect: Option<&str>,
) {
    let call = ToolCallUpdate::new(
        tool.to_owned(),
        ToolCallUpdateFields::new()
            .title(format!("Run {tool}"))
            .kind(ToolKind::Execute)
            .raw_input(json!({ "command": tool })),
    );
    let request = RequestPermissionRequest::new(session.to_owned(), call, permission_options());
    match cx.send_request(request).block_task().await {
        Ok(response) => {
            let outcome = serde_json::to_value(&response.outcome).unwrap_or(Value::Null);
            if let Some(expect) = expect
                && outcome.get("optionId").and_then(Value::as_str) != Some(expect)
            {
                recorder.push(&format!("unexpected:{tool}"), outcome.clone());
            }
            recorder.push(&format!("answer:{tool}"), outcome);
        }
        Err(err) => recorder.push(
            &format!("answer:{tool}"),
            json!({ "error": err.code.value() }),
        ),
    }
}

fn stop_reason(stop: Option<&str>) -> StopReason {
    match stop {
        Some("max_tokens") => StopReason::MaxTokens,
        Some("refusal") => StopReason::Refusal,
        Some("cancelled") => StopReason::Cancelled,
        _ => StopReason::EndTurn,
    }
}

/// Closes stdout, so the client's side of the transport ends.
#[cfg(unix)]
fn hang_up() {
    use std::os::fd::FromRawFd as _;
    // SAFETY: fd 1 is this process's stdout, open since it started. Owning
    // and dropping it closes it; the writer thread's later writes fail,
    // which ends that thread.
    drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(1) });
}

#[cfg(not(unix))]
fn hang_up() {
    std::process::exit(9);
}

/// Plays `script` on `transport` until the client's side closes.
pub(crate) async fn serve(
    transport: impl ConnectTo<Agent> + 'static,
    script: Script,
) -> Result<(), Error> {
    let recorder = Recorder::open(script.record.as_ref());
    let script = Arc::new(script);
    let counters = Arc::new(Counters::default());

    Agent
        .v1()
        .name("fe-acp-fake-agent")
        .on_receive_request(
            {
                let recorder = recorder.clone();
                let script = script.clone();
                async move |request: InitializeRequest, responder, _cx| {
                    recorder.record("initialize", &request);
                    let mut response = InitializeResponse::new(request.protocol_version)
                        .agent_capabilities(AgentCapabilities::new().load_session(true));
                    if let Some(version) = &script.version {
                        response = response
                            .agent_info(Implementation::new("claude-agent-acp", version.clone()));
                    }
                    responder.respond(response)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let recorder = recorder.clone();
                let script = script.clone();
                async move |request: NewSessionRequest, responder, cx| {
                    recorder.record("session/new", &request);
                    if let Some(kind) = &script.auth_status {
                        cx.send_notification(UntypedMessage::new(
                            "_auth/status_update",
                            json!({
                                "kind": kind,
                                "email": "someone@example.com",
                                "organization": "Example",
                            }),
                        )?)?;
                    }
                    if let Some(error) = &script.new_error {
                        return responder.respond_with_error(script_error(error));
                    }
                    responder.respond(NewSessionResponse::new(SESSION))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let recorder = recorder.clone();
                let script = script.clone();
                async move |request: LoadSessionRequest, responder, cx| {
                    recorder.record("session/load", &request);
                    if let Some(error) = &script.load_error {
                        return responder.respond_with_error(script_error(error));
                    }
                    let gate = script.load_gate.clone();
                    let task_cx = cx.clone();
                    cx.spawn(async move {
                        if let Some(gate) = gate {
                            while !gate.exists() {
                                async_io::Timer::after(std::time::Duration::from_millis(10)).await;
                            }
                        }
                        // A one-line replay, before the response.
                        task_cx.send_notification(SessionNotification::new(
                            request.session_id.clone(),
                            SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::from(
                                "earlier",
                            ))),
                        ))?;
                        responder.respond(LoadSessionResponse::new())
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let recorder = recorder.clone();
                async move |request: ResumeSessionRequest, responder, _cx| {
                    recorder.record("session/resume", &request);
                    responder.respond(ResumeSessionResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let recorder = recorder.clone();
                async move |request: CloseSessionRequest, responder, _cx| {
                    recorder.record("session/close", &request);
                    responder.respond(CloseSessionResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let recorder = recorder.clone();
                let counters = counters.clone();
                async move |notification: CancelNotification, _cx| {
                    recorder.record("session/cancel", &notification);
                    counters.cancels.bump();
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let recorder = recorder.clone();
                async move |request: PromptRequest, responder, cx| {
                    recorder.record("session/prompt", &request);
                    let index = counters.prompts.bump();
                    let turn = script.turns.get(index).cloned().unwrap_or_default();
                    let session = request.session_id.0.to_string();
                    let seen = counters.cancels.get();
                    let recorder = recorder.clone();
                    let counters = counters.clone();
                    let task_cx = cx.clone();
                    cx.spawn(async move {
                        let cx = task_cx;
                        if turn.after_previous {
                            counters.answered.at_least(index).await;
                        }
                        for text in &turn.updates {
                            cx.send_notification(SessionNotification::new(
                                session.clone(),
                                SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                    ContentBlock::from(text.as_str()),
                                )),
                            ))?;
                        }
                        if let Some(code) = turn.exit {
                            eprintln!("fake agent: exiting mid-turn with {code}");
                            let _ = std::io::stderr().flush();
                            std::process::exit(code);
                        }
                        if turn.hang_up {
                            eprintln!("fake agent: hanging up");
                            let _ = std::io::stderr().flush();
                            hang_up();
                            futures::future::pending::<()>().await;
                        }
                        if let Some(error) = &turn.error {
                            counters.answered.bump();
                            return responder.respond_with_error(script_error(error));
                        }
                        if let Some(tool) = turn.permission.clone() {
                            if turn.wait_for_cancel {
                                let (ask_cx, ask_recorder) = (cx.clone(), recorder.clone());
                                let (session, expect) = (session.clone(), turn.expect.clone());
                                cx.spawn(async move {
                                    ask(&ask_cx, &ask_recorder, &session, &tool, expect.as_deref())
                                        .await;
                                    Ok(())
                                })?;
                            } else {
                                ask(&cx, &recorder, &session, &tool, turn.expect.as_deref()).await;
                            }
                        }
                        let stop = if turn.wait_for_cancel {
                            counters.cancels.at_least(seen + 1).await;
                            StopReason::Cancelled
                        } else {
                            stop_reason(turn.stop.as_deref())
                        };
                        if let Some(tool) = &turn.late_permission {
                            counters.prompts.at_least(index + 2).await;
                            ask(&cx, &recorder, &session, tool, None).await;
                        }
                        let responded = responder.respond(PromptResponse::new(stop));
                        counters.answered.bump();
                        responded
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(transport)
        .await
}

/// Records what the process tests read back: its environment, arguments,
/// working directory and pid.
fn record_start(recorder: &Recorder, args: &[String]) {
    let variable = |name: &str| std::env::var(name).ok();
    recorder.push(
        "started",
        json!({
            "env": {
                "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS": variable("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS"),
                "CLAUDE_CODE_EXECUTABLE": variable("CLAUDE_CODE_EXECUTABLE"),
            },
            "args": args,
            "cwd": std::env::current_dir().ok(),
            "pid": std::process::id(),
        }),
    );
}

/// The process's stdin and stdout as the crate's line transport: a thread
/// reads stdin's lines, and another writes the outgoing ones to stdout.
fn stdio() -> Lines<
    impl futures::Sink<String, Error = std::io::Error> + Send + 'static,
    impl futures::Stream<Item = std::io::Result<String>> + Send + 'static,
> {
    let (incoming_tx, incoming_rx) = mpsc::unbounded();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let end = line.is_err();
            if incoming_tx.unbounded_send(line).is_err() || end {
                break;
            }
        }
    });
    let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded::<String>();
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        while let Some(line) = async_io::block_on(outgoing_rx.next()) {
            if writeln!(stdout, "{line}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let outgoing =
        outgoing_tx.sink_map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe));
    Lines::new(outgoing, incoming_rx)
}

#[allow(dead_code)] // Tests include this file as a module, without `main`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let script = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(|arg| {
            let text = if arg.starts_with('{') {
                arg.clone()
            } else {
                std::fs::read_to_string(arg).unwrap_or_else(|err| panic!("reading {arg}: {err}"))
            };
            serde_json::from_str::<Script>(&text).unwrap_or_else(|err| panic!("the script: {err}"))
        })
        .unwrap_or_default();
    let recorder = Recorder::open(script.record.as_ref());
    record_start(&recorder, &args);
    {
        let mut stderr = std::io::stderr().lock();
        if let Some((byte, count)) = script.stderr_fill {
            let _ = stderr.write_all(&vec![byte; count]);
        }
        let _ = stderr.write_all(&script.stderr_bytes);
        for line in &script.stderr {
            let _ = writeln!(stderr, "{line}");
        }
        let _ = stderr.flush();
    }
    if script.spawn_child {
        let child = std::process::Command::new("sleep")
            .arg("300")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("starting the child");
        recorder.push("child", json!({ "pid": child.id() }));
        // Never waited for: it lives until its process group is killed.
        std::mem::forget(child);
    }
    let linger = script.linger;
    let served = async_io::block_on(serve(stdio(), script));
    if let Err(err) = served {
        eprintln!("fake agent: {err}");
    }
    if linger {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
}
