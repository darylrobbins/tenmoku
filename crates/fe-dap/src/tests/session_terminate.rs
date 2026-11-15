// SPDX-License-Identifier: Apache-2.0

//! Termination (docs/decisions/r1b9dw3g0g-debugger.md §2 item 6, D2 C7):
//! the user's Stop sends `terminate`, then `disconnect` on `terminated`
//! (or 2 s without it), and kills an adapter still running 2 s after the
//! `disconnect`; the adapter's own `terminated` gets `disconnect` at once
//! and no `terminate`; an adapter exiting by itself ends the session as
//! `AdapterExited`, with no restart. After the user's Stop no late
//! `stopped` or `continued` changes the state or enables a control (§13
//! F11).

use super::session_harness::{
    Harness, TestProcess, advance_past, continued_event, exited_process, frame,
    initialize_response, options, options_with_breakpoint, requests, start, start_on,
    start_stopped, states_from, stopped_event, wait_requests,
};
use crate::fake::{Fault, FaultKind, Script};
use crate::launcher::ExitInfo;
use crate::protocol::request;
use crate::session::{
    Controls, Disabled, EXIT_GRACE, EndReason, Resume, SessionState, TERMINATED_GRACE,
};

fn stopped(script: Script) -> Harness {
    let mut harness = start(script, options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    wait_requests(&harness.fake(0), "threads", 1);
    harness
}

fn terminates_debuggee(fake: &crate::fake::FakeAdapter) -> bool {
    let disconnect = &requests(fake, "disconnect")[0];
    disconnect
        .arguments::<request::Disconnect>()
        .unwrap()
        .terminate_debuggee
        == Some(true)
}

#[test]
fn user_stop_sends_terminate_then_disconnect_on_terminated_and_kills_a_mute_adapter() {
    let script = Script {
        lingers: true,
        exit_code: 7,
        ..Script::default()
    };
    let mut harness = stopped(script);
    let fake = harness.fake(0);
    harness.session.stop();
    wait_requests(&fake, "disconnect", 1);
    assert_eq!(
        fake.commands()[fake.commands().len() - 2..],
        ["terminate", "disconnect"]
    );
    assert!(terminates_debuggee(&fake));
    // The disconnect was answered, but the adapter keeps running: killed
    // 2 s later, not before.
    assert!(!fake.killed());
    advance_past(&harness.clock, EXIT_GRACE);
    let ended = harness.wait_ended();
    assert!(fake.killed(), "the mute adapter wasn't killed");
    assert_eq!(ended.reason, EndReason::Stopped);
    assert_eq!(ended.exit_code, Some(7));
}

#[test]
fn user_stop_without_terminated_disconnects_after_2_s() {
    let script = Script {
        faults: vec![Fault::new("terminate", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut harness = stopped(script);
    let fake = harness.fake(0);
    harness.session.stop();
    wait_requests(&fake, "terminate", 1);
    assert_eq!(harness.session.state(), SessionState::Terminating);
    advance_past(&harness.clock, TERMINATED_GRACE);
    wait_requests(&fake, "disconnect", 1);
    assert!(terminates_debuggee(&fake));
    // This adapter exits on `disconnect`.
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Stopped);
    assert!(!fake.killed());
}

#[test]
fn without_terminate_support_stop_disconnects_at_once() {
    let mut script = Script::default();
    script.capabilities.supports_terminate_request = None;
    let mut harness = stopped(script);
    let fake = harness.fake(0);
    harness.session.stop();
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Stopped);
    assert_eq!(
        fake.commands().last().map(String::as_str),
        Some("disconnect")
    );
    assert!(!fake.commands().contains(&"terminate".to_owned()));
    assert!(terminates_debuggee(&fake));
}

#[test]
fn an_adapters_own_terminated_gets_disconnect_at_once_and_no_terminate() {
    // No breakpoints: the program runs to its end, and the fake sends
    // `exited` and `terminated`.
    let script = Script {
        exit_code: 3,
        ..Script::default()
    };
    let mut harness = start(script, options());
    let fake = harness.fake(0);
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Finished);
    assert_eq!(ended.exit_code, Some(3));
    assert!(!fake.commands().contains(&"terminate".to_owned()));
    assert_eq!(
        fake.commands().last().map(String::as_str),
        Some("disconnect")
    );
    assert!(!fake.killed());
}

#[test]
fn an_adapter_exiting_on_its_own_ends_as_adapter_exited_with_no_restart() {
    let script = Script {
        faults: vec![Fault::new("setExceptionBreakpoints", FaultKind::Exit(3))],
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    let fake = harness.fake(0);
    let ended = harness.wait_ended();
    assert_eq!(
        ended.reason,
        EndReason::AdapterExited(ExitInfo {
            code: Some(3),
            signal: None
        })
    );
    assert_eq!(fake.exit().and_then(|exit| exit.code), Some(3));
    // Nothing was sent to the gone adapter, and none was started again.
    assert!(!fake.commands().contains(&"disconnect".to_owned()));
    assert_eq!(harness.launcher.launched().len(), 1);
    assert!(
        !harness.states().contains(&SessionState::Launching),
        "{:?}",
        harness.states()
    );
}

/// A stream that refuses every write.
struct Refusing;

impl futures::AsyncWrite for Refusing {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[test]
fn a_failed_input_stream_ends_the_session_and_kills_the_adapter() {
    // The adapter's input fails; its output stays open and its process
    // runs until killed.
    let (output, _keep_open) = crate::fake::pipe();
    let (process, killed) = TestProcess::new();
    let io = crate::launcher::AdapterIo {
        input: Box::pin(Refusing),
        output: Box::pin(output),
        stderr: None,
        stdout: None,
        port: None,
        process,
    };
    let mut harness = start_on(io, options());
    // A repl evaluation has no timeout of its own: the failure ends it.
    let repl = harness
        .session
        .request::<request::Evaluate>(&crate::protocol::EvaluateArguments {
            expression: "x".into(),
            context: Some("repl".into()),
            ..Default::default()
        });
    // Its output stays open: what's left is read for a while after the
    // exit, then not.
    advance_past(&harness.clock, crate::session::EXIT_DRAIN);
    let ended = harness.wait_ended();
    assert!(
        matches!(
            ended.reason,
            EndReason::Failed(crate::session::SessionError::Transport(_))
        ),
        "{ended:?}"
    );
    assert!(killed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        async_io::block_on(repl),
        Err(crate::session::RequestError::Ended {
            command: "evaluate".into()
        })
    );
}

#[test]
fn output_closing_keeps_the_kill_deadline() {
    let script = Script {
        lingers: true,
        ..Script::default()
    };
    let mut harness = stopped(script);
    let fake = harness.fake(0);
    harness.session.stop();
    wait_requests(&fake, "disconnect", 1);
    // The disconnect is answered: the kill is due 2 s on.
    harness.clock.wait_for_sleep(EXIT_GRACE);
    let almost = EXIT_GRACE - std::time::Duration::from_millis(100);
    harness.clock.advance(almost);
    // The adapter's output closes just before; it keeps running.
    fake.close_output();
    std::thread::sleep(std::time::Duration::from_millis(100));
    harness.clock.advance(EXIT_GRACE - almost);
    harness.wait_ended();
    assert!(fake.killed(), "the kill moved past its deadline");
}

#[test]
fn what_the_adapter_sent_before_exiting_is_still_read() {
    // More output than any fixed cut-off, then `exited` and `terminated`,
    // from an adapter that has already exited.
    let mut output = frame(&initialize_response(serde_json::json!({})));
    for seq in 2..302 {
        output.extend(frame(&crate::protocol::Message::Event(
            crate::protocol::EventMessage {
                seq,
                event: "output".into(),
                body: Some(serde_json::json!({ "category": "stdout", "output": "line\n" })),
                ..Default::default()
            },
        )));
    }
    output.extend(frame(&crate::protocol::Message::Event(
        crate::protocol::EventMessage::new::<crate::protocol::event::Exited>(
            302,
            &crate::protocol::ExitedBody {
                exit_code: 7,
                ..Default::default()
            },
        ),
    )));
    output.extend(frame(&crate::protocol::Message::Event(
        crate::protocol::EventMessage::new::<crate::protocol::event::Terminated>(
            303,
            &crate::protocol::TerminatedBody::default(),
        ),
    )));
    let (_sent, input) = crate::fake::pipe();
    let io = crate::launcher::AdapterIo {
        input: Box::pin(input),
        output: Box::pin(futures::io::Cursor::new(output)),
        stderr: None,
        stdout: None,
        port: None,
        process: exited_process(),
    };
    let mut harness = start_on(io, options());
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Finished);
    assert_eq!(ended.exit_code, Some(7));
}

#[test]
fn late_events_after_stop_enable_nothing() {
    // `terminate` goes unanswered: the session stays Terminating until the
    // grace runs out, for the late events to come in.
    let script = Script {
        faults: vec![Fault::new("terminate", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    assert!(harness.session.controls().continue_.is_enabled());
    harness.session.stop();
    // Off from the call on.
    let ending = Controls::none(Disabled::Ending);
    assert_eq!(harness.session.controls(), ending);
    wait_requests(&fake, "terminate", 1);
    harness.wait_state(SessionState::Terminating);

    fake.send(stopped_event("breakpoint"));
    fake.send(continued_event());
    // Both are read, and logged.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !harness
        .log_lines()
        .iter()
        .any(|line| line.contains("event=continued"))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the events never came"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let stops = harness
        .log_lines()
        .iter()
        .filter(|line| line.contains("event=stopped"))
        .count();
    assert_eq!(stops, 2);
    harness.drain();
    assert_eq!(harness.session.state(), SessionState::Terminating);
    assert_eq!(harness.session.controls(), ending);
    assert!(!harness.session.resume(Resume::Continue, None));
    assert!(!harness.session.resume(Resume::Next, None));
    assert!(!harness.session.pause(None));
    assert_eq!(harness.session.stop_data(), None);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Terminating),
        [SessionState::Terminating]
    );

    advance_past(&harness.clock, TERMINATED_GRACE);
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Stopped);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Terminating),
        [SessionState::Terminating, SessionState::Ended(ended)]
    );
    assert_eq!(harness.session.controls(), Controls::none(Disabled::Ended));
    assert!(requests(&fake, "continue").is_empty());
    assert!(requests(&fake, "pause").is_empty());
}

#[test]
fn an_adapter_exiting_while_stopped_retires_the_stop() {
    let mut harness = start_stopped(Script::default());
    let stop = harness.session.stop_data().expect("stopped");
    harness.fake(0).kill();
    let ended = harness.wait_ended();
    assert!(
        matches!(ended.reason, EndReason::AdapterExited(_)),
        "{ended:?}"
    );
    assert_eq!(harness.session.stop_data(), None);
    assert!(harness.session.epoch() > stop.epoch);
    assert_eq!(
        async_io::block_on(harness.session.variables(stop.scope("Locals").unwrap())),
        Err(crate::session::RequestError::Ended {
            command: "variables".into()
        })
    );
    assert_eq!(harness.session.controls(), Controls::none(Disabled::Ended));
}
