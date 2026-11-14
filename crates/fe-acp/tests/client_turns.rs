//! Which turn a permission request belongs to, after a cancel (T618's
//! review, F2): a turn is tracked from its prompt to its response as the
//! connection receives it, not by the caller's prompt future.
//!
//! - Dropping a cancelled prompt's future before its response leaves the
//!   turn cancelled, so a late permission request is still answered
//!   `Cancelled` and never reaches the stream.
//! - A cancelled turn's response ends it at once, even while its caller
//!   hasn't resumed, so the next turn's permission requests reach the
//!   stream.
//!
//! Against the fake agent in-process over `Channel::duplex()`.

#[allow(dead_code, unreachable_pub)]
#[path = "../src/bin/fe-acp-fake-agent.rs"]
mod fake;

use std::path::{Path, PathBuf};
use std::time::Duration;

use fe_acp::schema::{ContentBlock, StopReason};
use fe_acp::{AcpClient, Channel, Incoming};
use futures::channel::mpsc;
use futures::{FutureExt as _, StreamExt as _};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(20);

fn recorded(path: &Path) -> Vec<(String, Value)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            (
                value["method"].as_str().unwrap().to_owned(),
                value["params"].clone(),
            )
        })
        .collect()
}

fn run<F>(
    record: PathBuf,
    turns: Vec<fake::Turn>,
    body: impl FnOnce(AcpClient, mpsc::UnboundedReceiver<Incoming>) -> F,
) where
    F: Future<Output = ()>,
{
    let script = fake::Script {
        record: Some(record),
        turns,
        ..fake::Script::default()
    };
    let (client_end, agent_end) = Channel::duplex();
    let (client, incoming, driver) = fe_acp::connect(client_end);
    let served = fake::serve(agent_end, script);
    let body = {
        let client = client.clone();
        async move {
            client.initialize().await.expect("initialize");
            client
                .new_session("/work/lane", None)
                .await
                .expect("session/new");
            body(client.clone(), incoming).await;
            client.shutdown();
        }
    };
    let all = async {
        let ((), driven, _served) = futures::join!(body, driver, served);
        driven.expect("the client's connection");
    };
    async_io::block_on(async {
        futures::select! {
            () = all.fuse() => {}
            _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
        }
    });
}

fn prompt(text: &str) -> Vec<ContentBlock> {
    vec![ContentBlock::from(text)]
}

#[test]
fn a_dropped_cancelled_prompt_still_refuses_late_permission_requests() {
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record.jsonl");
    let turns = vec![
        // Waits for the cancel, then for the next prompt, then asks.
        fake::Turn {
            wait_for_cancel: true,
            late_permission: Some("late".into()),
            ..fake::Turn::default()
        },
        // Waits until the first turn has answered.
        fake::Turn {
            after_previous: true,
            ..fake::Turn::default()
        },
    ];
    run(
        record.clone(),
        turns,
        async |client: AcpClient, mut incoming| {
            {
                let first = client.prompt(fake::SESSION, prompt("first"));
                futures::pin_mut!(first);
                // Sent, but not answered.
                assert!(futures::poll!(first.as_mut()).is_pending());
                client.cancel(fake::SESSION).await.expect("cancel");
                // The caller gives up on the turn before its response.
            }
            let second = client
                .prompt(fake::SESSION, prompt("second"))
                .await
                .expect("second");
            assert_eq!(second.stop_reason, StopReason::EndTurn);
            // The late request was answered without reaching the stream.
            while let Some(Some(message)) = incoming.next().now_or_never() {
                assert!(
                    !matches!(message, Incoming::Permission(_)),
                    "a cancelled turn's request reached the stream"
                );
            }
        },
    );
    let answers: Vec<Value> = recorded(&record)
        .into_iter()
        .filter(|(m, _)| m == "answer:late")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(answers, [json!({"outcome": "cancelled"})]);
}

#[test]
fn a_cancelled_turns_response_ends_it_before_its_caller_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("record.jsonl");
    let turns = vec![
        fake::Turn {
            wait_for_cancel: true,
            ..fake::Turn::default()
        },
        // Queued behind the first: asks once the first has answered.
        fake::Turn {
            after_previous: true,
            permission: Some("queued".into()),
            expect: Some("allow-once".into()),
            ..fake::Turn::default()
        },
    ];
    run(
        record.clone(),
        turns,
        async |client: AcpClient, mut incoming| {
            let first = client.prompt(fake::SESSION, prompt("first"));
            futures::pin_mut!(first);
            assert!(futures::poll!(first.as_mut()).is_pending());
            client.cancel(fake::SESSION).await.expect("cancel");
            // The first turn's caller doesn't resume until the second turn
            // is done.
            let second = client.prompt(fake::SESSION, prompt("second"));
            let answer = async {
                loop {
                    match incoming.next().await.expect("the stream") {
                        Incoming::Permission(request) => {
                            assert!(request.select("allow-once"), "the answer is sent");
                            break;
                        }
                        Incoming::Update { .. } => {}
                    }
                }
            };
            let (second, ()) = futures::join!(second, answer);
            assert_eq!(second.expect("second").stop_reason, StopReason::EndTurn);
            assert_eq!(
                first.await.expect("first").stop_reason,
                StopReason::Cancelled
            );
        },
    );
    let recorded = recorded(&record);
    let answers: Vec<&Value> = recorded
        .iter()
        .filter(|(m, _)| m == "answer:queued")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(
        answers,
        [&json!({"outcome": "selected", "optionId": "allow-once"})]
    );
    assert!(!recorded.iter().any(|(m, _)| m.starts_with("unexpected:")));
}

/// The connection, the fake and the test each on a thread of their own, so
/// responses are routed while requests are being published (review round 1
/// of rev 4, F10). A cancelled turn's response ends it before the queued
/// turn's permission request is dispatched, every time.
#[test]
fn turn_bookkeeping_holds_with_the_connection_driven_concurrently() {
    for _ in 0..50 {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("record.jsonl");
        let script = fake::Script {
            record: Some(record.clone()),
            turns: vec![
                fake::Turn {
                    wait_for_cancel: true,
                    ..fake::Turn::default()
                },
                fake::Turn {
                    after_previous: true,
                    permission: Some("queued".into()),
                    expect: Some("allow-once".into()),
                    ..fake::Turn::default()
                },
            ],
            ..fake::Script::default()
        };
        let (client_end, agent_end) = Channel::duplex();
        let (client, mut incoming, driver) = fe_acp::connect(client_end);
        let driven = std::thread::spawn(move || async_io::block_on(driver));
        let served = std::thread::spawn(move || async_io::block_on(fake::serve(agent_end, script)));
        async_io::block_on(async {
            let body = async {
                client.initialize().await.expect("initialize");
                client.new_session("/work/lane", None).await.expect("new");
                let first = client.prompt(fake::SESSION, prompt("first"));
                futures::pin_mut!(first);
                assert!(futures::poll!(first.as_mut()).is_pending());
                client.cancel(fake::SESSION).await.expect("cancel");
                let second = client.prompt(fake::SESSION, prompt("second"));
                let answer = async {
                    loop {
                        if let Incoming::Permission(request) =
                            incoming.next().await.expect("the stream")
                        {
                            assert!(request.select("allow-once"));
                            break;
                        }
                    }
                };
                let (second, ()) = futures::join!(second, answer);
                assert_eq!(second.expect("second").stop_reason, StopReason::EndTurn);
                assert_eq!(
                    first.await.expect("first").stop_reason,
                    StopReason::Cancelled
                );
            };
            futures::select! {
                () = body.fuse() => {}
                _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
            }
        });
        client.shutdown();
        drop(client);
        driven.join().unwrap().expect("the connection");
        let _ = served.join().unwrap();
        assert!(
            !recorded(&record)
                .iter()
                .any(|(m, _)| m.starts_with("unexpected:")),
        );
    }
}

/// Once the connection's driver has ended, every request fails as a
/// disconnect, at once (review round 1 of rev 4, F11).
#[test]
fn requests_after_the_driver_has_ended_are_disconnects() {
    let (client_end, agent_end) = Channel::duplex();
    let (client, _incoming, driver) = fe_acp::connect(client_end);
    let driven = std::thread::spawn(move || async_io::block_on(driver));
    let served = std::thread::spawn(move || {
        async_io::block_on(fake::serve(agent_end, fake::Script::default()))
    });
    async_io::block_on(async {
        client.initialize().await.expect("initialize");
        client.new_session("/work/lane", None).await.expect("new");
    });
    // The agent's side goes away; the driver ends with it.
    client.shutdown();
    let _ = driven.join().unwrap();
    let _ = served.join().unwrap();
    async_io::block_on(async {
        let errors = [
            client.initialize().await.err(),
            client.prompt(fake::SESSION, prompt("hi")).await.err(),
            client.start_prompt(fake::SESSION, prompt("hi")).err(),
            client
                .load_session(fake::SESSION, "/work/lane", None)
                .await
                .err(),
            client
                .start_load_session(fake::SESSION, "/work/lane", None, |_| {})
                .err(),
            client.cancel(fake::SESSION).await.err(),
        ];
        for (i, error) in errors.into_iter().enumerate() {
            let error = error.unwrap_or_else(|| panic!("request {i} succeeded"));
            assert!(fe_acp::is_disconnect(&error), "request {i}: {error:?}");
            assert_eq!(
                fe_acp::AgentState::from_error(&error),
                fe_acp::AgentState::Disconnected { stderr: Vec::new() },
                "request {i}"
            );
        }
        assert!(!client.turn_running(&fake::SESSION.into()));
    });
}

/// The client's outgoing side, which stops taking lines (and never wakes)
/// once `held` is set: the SDK's output draining then stays open, so its
/// teardown never publishes the termination.
struct HeldSink {
    tx: mpsc::UnboundedSender<String>,
    held: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl HeldSink {
    fn poll_open(&self) -> std::task::Poll<std::io::Result<()>> {
        if self.held.load(std::sync::atomic::Ordering::SeqCst) {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok(()))
        }
    }
}

impl futures::Sink<String> for HeldSink {
    type Error = std::io::Error;

    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_open()
    }

    fn start_send(self: std::pin::Pin<&mut Self>, line: String) -> std::io::Result<()> {
        self.tx
            .unbounded_send(line)
            .map_err(|_| std::io::ErrorKind::BrokenPipe.into())
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_open()
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_open()
    }
}

/// The connection starts tearing down (its incoming side ends) while its
/// output draining is held open, so the termination isn't published yet:
/// callback registration, ordinary requests and cancel all fail as
/// disconnects, never as untagged internal errors (rev 4 round 2, F11).
#[test]
fn requests_while_the_connection_tears_down_are_disconnects() {
    let (client_out, agent_in) = mpsc::unbounded::<String>();
    let (agent_out, client_in) = mpsc::unbounded::<String>();
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (cut_tx, cut_rx) = futures::channel::oneshot::channel::<()>();
    let client_transport = agent_client_protocol::Lines::new(
        HeldSink {
            tx: client_out,
            held: held.clone(),
        },
        client_in.map(Ok).take_until(cut_rx),
    );
    let agent_transport = agent_client_protocol::Lines::new(
        futures::SinkExt::sink_map_err(agent_out, |_| {
            std::io::Error::from(std::io::ErrorKind::BrokenPipe)
        }),
        agent_in.map(Ok),
    );
    let (client, _incoming, driver) = fe_acp::connect(client_transport);
    let _driven = std::thread::spawn(move || async_io::block_on(driver));
    let _served = std::thread::spawn(move || {
        async_io::block_on(fake::serve(agent_transport, fake::Script::default()))
    });
    async_io::block_on(async {
        let body = async {
            client.initialize().await.expect("initialize");
            client.new_session("/work/lane", None).await.expect("new");
            // Output held, then the incoming side ends: teardown starts and
            // stalls draining.
            held.store(true, std::sync::atomic::Ordering::SeqCst);
            cut_tx.send(()).unwrap();
            async_io::Timer::after(Duration::from_millis(300)).await;

            let registered = client.start_prompt(fake::SESSION, prompt("hi")).err();
            let requested = client.initialize().await.err();
            let cancelled = client.cancel(fake::SESSION).await.err();
            let loaded = client
                .load_session(fake::SESSION, "/work/lane", None)
                .await
                .err();
            for (what, error) in [
                ("start_prompt", registered),
                ("initialize", requested),
                ("cancel", cancelled),
                ("load_session", loaded),
            ] {
                let error = error.unwrap_or_else(|| panic!("{what} succeeded"));
                assert!(fe_acp::is_disconnect(&error), "{what}: {error:?}");
            }
        };
        futures::select! {
            () = body.fuse() => {}
            _ = futures::FutureExt::fuse(async_io::Timer::after(TIMEOUT)) => panic!("timed out"),
        }
    });
    // The driver stays stuck draining; the threads are left to the process.
}
