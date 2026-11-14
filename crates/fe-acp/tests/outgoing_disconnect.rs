//! A send-side failure on the client's outgoing channel is a disconnect,
//! whatever its interleaving with the connection's teardown (T1787 F11).
//!
//! The connection driver runs on its own thread and the caller on another,
//! so a request or a `session/cancel` can fail on the outgoing channel
//! before the SDK has closed its task queue or published the termination.
//! The fake agent and transport are copied in miniature from
//! `tests/client.rs`.

use std::thread;
use std::time::{Duration, Instant};

use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{Agent, Client};
use fe_acp::schema::{
    AgentCapabilities, ContentBlock, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestPermissionRequest, StopReason, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use fe_acp::{AcpClient, Channel, Error, ErrorCode, Incoming, is_disconnect};
use futures::channel::oneshot;
use futures::{FutureExt as _, StreamExt as _};
use serde_json::json;

const SESSION: &str = "s1";
const ITERATIONS: usize = 300;
const RACE_ITERATIONS: usize = 4000;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq)]
enum Behaviour {
    /// Answers `session/new` with a JSON-RPC error.
    Refuses,
    /// Answers `session/new` with an internal error whose data is shaped
    /// like the SDK's own local channel failure.
    RefusesLikeTheSdk(&'static str),
    /// Asks a permission request on every prompt and never answers it.
    Asks,
}

/// The fake agent, served until `exit` fires or its peer goes away. Ending
/// drops its end of the transport.
async fn fake_agent(channel: Channel, behaviour: Behaviour, exit: oneshot::Receiver<()>) {
    let served = Agent
        .v1()
        .name("fake-agent")
        .on_receive_request(
            async move |request: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(request.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: NewSessionRequest, responder, _cx| {
                if let Behaviour::RefusesLikeTheSdk(data) = behaviour {
                    return responder.respond_with_error(Error::internal_error().data(data));
                }
                if behaviour == Behaviour::Refuses {
                    return responder.respond_with_error(Error::invalid_params().data("no"));
                }
                responder.respond(NewSessionResponse::new(SESSION))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: PromptRequest, responder, cx: ConnectionTo<Client>| {
                let ask_cx = cx.clone();
                cx.spawn(async move {
                    let tool = ToolCallUpdate::new(
                        "t1".to_owned(),
                        ToolCallUpdateFields::new()
                            .title("Run t1")
                            .kind(ToolKind::Execute)
                            .raw_input(json!({ "command": "t1" })),
                    );
                    let request = RequestPermissionRequest::new(
                        SESSION,
                        tool,
                        vec![PermissionOption::new(
                            "allow-once",
                            "Allow",
                            PermissionOptionKind::AllowOnce,
                        )],
                    );
                    let _ = ask_cx.send_request(request).block_task().await;
                    Ok(())
                })?;
                // The turn never ends on its own.
                futures::future::pending::<()>().await;
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(channel);
    futures::select! {
        _ = served.fuse() => {}
        _ = exit.fuse() => {}
    }
}

fn block_on_within<T>(future: impl Future<Output = T>) -> T {
    async_io::block_on(async {
        futures::select! {
            value = future.fuse() => value,
            () = futures::FutureExt::map(async_io::Timer::after(TIMEOUT), |_| ()).fuse() => panic!("timed out"),
        }
    })
}

/// A connection with its driver on a thread of its own.
struct Live {
    client: AcpClient,
    incoming: futures::channel::mpsc::UnboundedReceiver<Incoming>,
    driver: thread::JoinHandle<()>,
    agent: Option<(thread::JoinHandle<()>, oneshot::Sender<()>)>,
}

fn live(behaviour: Behaviour) -> Live {
    let (client_end, agent_end) = Channel::duplex();
    let (client, incoming, driver) = fe_acp::connect(client_end);
    let driver = thread::spawn(move || {
        let _ = async_io::block_on(driver);
    });
    let (exit_tx, exit_rx) = oneshot::channel();
    let agent = thread::spawn(move || {
        async_io::block_on(fake_agent(agent_end, behaviour, exit_rx));
    });
    Live {
        client,
        incoming,
        driver,
        agent: Some((agent, exit_tx)),
    }
}

impl Live {
    /// The agent exits: its end of the transport is dropped.
    fn agent_exits(&mut self) {
        let (agent, exit) = self.agent.take().expect("the agent is running");
        let _ = exit.send(());
        agent.join().expect("the agent thread");
    }

    fn finish(self) {
        self.client.shutdown();
        drop(self.client);
        self.driver.join().expect("the driver thread");
    }
}

#[test]
fn request_after_the_agent_end_is_closed_is_a_disconnect() {
    for i in 0..ITERATIONS {
        let (client_end, agent_end) = Channel::duplex();
        let (client, _incoming, driver) = fe_acp::connect(client_end);
        let driver = thread::spawn(move || {
            let _ = async_io::block_on(driver);
        });
        // Closed before the request, and the request on this thread while
        // the driver tears down on its own.
        drop(agent_end);
        let error = block_on_within(client.initialize()).expect_err("the agent is gone");
        assert!(is_disconnect(&error), "iteration {i}: {error:?}");
        driver.join().expect("the driver thread");
    }
}

#[test]
fn request_while_the_agent_end_is_closed_is_always_a_disconnect() {
    // The window is narrow: many rounds hit it at the base.
    for i in 0..RACE_ITERATIONS {
        let mut live = live(Behaviour::Asks);
        block_on_within(live.client.initialize()).expect("initialize");
        // Several callers at once, so one of them is sure to send while
        // the connection is between its outgoing and its incoming end.
        let callers: Vec<_> = (0..3)
            .map(|_| {
                let client = live.client.clone();
                thread::spawn(move || {
                    let deadline = Instant::now() + TIMEOUT;
                    loop {
                        match block_on_within(client.set_mode(SESSION, "plan")) {
                            Ok(()) => assert!(Instant::now() < deadline, "never failed"),
                            // The live fake has no set_mode handler: its
                            // refusal is retried until the agent goes.
                            Err(error) if error.code == ErrorCode::METHOD_NOT_FOUND => {
                                assert!(Instant::now() < deadline, "never disconnected");
                            }
                            Err(error) => return error,
                        }
                    }
                })
            })
            .collect();
        live.agent_exits();
        for caller in callers {
            let error = caller.join().expect("the caller thread");
            assert!(is_disconnect(&error), "iteration {i}: {error:?}");
        }
        live.finish();
    }
}

#[test]
fn cancel_racing_the_agent_exit_is_always_a_disconnect() {
    for i in 0..ITERATIONS {
        let mut live = live(Behaviour::Asks);
        block_on_within(live.client.initialize()).expect("initialize");
        let session = block_on_within(live.client.new_session("/work", None))
            .expect("session/new")
            .session_id;
        let client = live.client.clone();
        let prompt_session = session.clone();
        let _prompt = block_on_within(async {
            client.connected().await?;
            client.start_prompt(prompt_session, vec![ContentBlock::from("ask")])
        })
        .expect("prompt started");
        let permission = match block_on_within(live.incoming.next()).expect("incoming") {
            Incoming::Permission(request) => request,
            Incoming::Update { .. } => panic!("expected a permission request"),
        };

        let canceller = {
            let client = live.client.clone();
            let session = session.clone();
            thread::spawn(move || {
                let deadline = Instant::now() + TIMEOUT;
                loop {
                    // Each cancel is sent while the agent goes; once the
                    // connection is down one of them must fail.
                    match block_on_within(client.cancel(session.clone())) {
                        Ok(()) => assert!(Instant::now() < deadline, "never failed"),
                        Err(error) => return error,
                    }
                }
            })
        };
        live.agent_exits();
        let error = canceller.join().expect("the canceller thread");
        assert!(is_disconnect(&error), "iteration {i}: {error:?}");
        assert!(
            !permission.is_pending(),
            "iteration {i}: answered Cancelled"
        );
        drop(permission);
        live.finish();
    }
}

#[test]
fn an_error_from_a_live_agent_is_not_a_disconnect() {
    let live = live(Behaviour::Refuses);
    block_on_within(live.client.initialize()).expect("initialize");
    let error = block_on_within(live.client.new_session("/work", None))
        .expect_err("the agent refuses session/new");
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(!is_disconnect(&error), "{error:?}");
    live.finish();
}

#[test]
fn a_live_agents_internal_error_shaped_like_a_channel_failure_is_not_a_disconnect() {
    for data in [
        "send failed because receiver is gone",
        "failed to send outgoing request `backend`: permission denied",
    ] {
        let live = live(Behaviour::RefusesLikeTheSdk(data));
        block_on_within(live.client.initialize()).expect("initialize");
        let error = block_on_within(live.client.new_session("/work", None))
            .expect_err("the agent refuses session/new");
        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(error.data, Some(json!(data)), "returned unchanged");
        assert!(!is_disconnect(&error), "{error:?}");
        live.finish();
    }
}
