// SPDX-License-Identifier: Apache-2.0

//! Requests (docs/decisions/r1b9dw3g0g-debugger.md §2 "Requests" and
//! "Reverse requests"): per-command timeouts on the injected clock (10 s,
//! 30 s for `launch`, none for a `repl` evaluation), `cancel` for a dropped
//! request future only with `supportsCancelRequest`, `runInTerminal` as an
//! event whose reply carries the process id, and `startDebugging` refused.

use std::time::Duration;

use futures::FutureExt as _;
use kiln_l10n::tr;
use serde_json::json;

use super::session_harness::{
    Harness, advance_past, object, options, options_with_breakpoint, start, wait_client_response,
    wait_requests,
};
use crate::fake::{Fault, FaultKind, Script, Startup};
use crate::protocol::{Empty, EvaluateArguments, RunInTerminalBody, request};
use crate::session::{
    DapEvent, EndReason, LAUNCH_TIMEOUT, PendingRequest, REQUEST_TIMEOUT, RequestError,
    SessionError, SessionState, TerminalStarted,
};

fn stopped(script: Script) -> Harness {
    let mut harness = start(script, options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    wait_requests(&harness.fake(0), "threads", 1);
    harness
}

fn evaluate(harness: &Harness, context: &str) -> PendingRequest<crate::protocol::EvaluateBody> {
    harness
        .session
        .request::<request::Evaluate>(&EvaluateArguments {
            expression: "x".into(),
            context: Some(context.into()),
            ..Default::default()
        })
}

/// A request the fake answers, awaited: every command sent before it has
/// been handled once it returns.
fn round_trip(harness: &Harness) {
    let threads = harness
        .session
        .request::<request::Threads>(&Empty::default());
    async_io::block_on(threads).expect("threads is answered");
}

#[test]
fn requests_time_out_per_command() {
    let script = Script {
        faults: vec![
            Fault::new("evaluate", FaultKind::Unanswered),
            Fault::new("evaluate", FaultKind::Unanswered).nth(2),
        ],
        ..Script::default()
    };
    let harness = stopped(script);
    let fake = harness.fake(0);

    // 10 s by default.
    let mut watch = evaluate(&harness, "watch");
    wait_requests(&fake, "evaluate", 1);
    advance_past(&harness.clock, REQUEST_TIMEOUT);
    assert_eq!(
        async_io::block_on(&mut watch),
        Err(RequestError::Timeout {
            command: "evaluate".into(),
            after: Duration::from_secs(10)
        })
    );

    // None in the repl: the user cancels it.
    let mut repl = evaluate(&harness, "repl");
    wait_requests(&fake, "evaluate", 2);
    round_trip(&harness);
    harness.clock.advance(Duration::from_secs(3600));
    // Timers are handled before later commands, so a timeout would have
    // landed before this round trip's reply.
    round_trip(&harness);
    assert!(
        (&mut repl).now_or_never().is_none(),
        "the repl evaluation timed out"
    );
    drop(repl);
}

#[test]
fn launch_times_out_after_30_s() {
    let script = Script {
        startup: Startup::LaunchResponseFirst,
        faults: vec![Fault::new("launch", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut harness = start(script, options());
    let fake = harness.fake(0);
    wait_requests(&fake, "launch", 1);
    advance_past(&harness.clock, LAUNCH_TIMEOUT);
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Failed(SessionError::StartTimeout));
    assert!(fake.killed());
    assert_eq!(LAUNCH_TIMEOUT, Duration::from_secs(30));
}

#[test]
fn a_dropped_request_sends_cancel_only_with_cancel_support() {
    let unanswered = vec![Fault::new("evaluate", FaultKind::Unanswered)];

    // With `supportsCancelRequest`: `cancel`, naming the request.
    let harness = stopped(Script {
        faults: unanswered.clone(),
        ..Script::default()
    });
    let fake = harness.fake(0);
    let pending = evaluate(&harness, "watch");
    let seq = wait_requests(&fake, "evaluate", 1)[0].seq;
    drop(pending);
    let cancels = wait_requests(&fake, "cancel", 1);
    let arguments = cancels[0].arguments::<request::Cancel>().unwrap();
    assert_eq!(arguments.request_id, Some(seq));

    // Without it: nothing.
    let mut script = Script {
        faults: unanswered,
        ..Script::default()
    };
    script.capabilities.supports_cancel_request = None;
    let harness = stopped(script);
    let fake = harness.fake(0);
    let pending = evaluate(&harness, "watch");
    wait_requests(&fake, "evaluate", 1);
    drop(pending);
    round_trip(&harness);
    wait_requests(&fake, "threads", 2);
    assert!(
        !fake.commands().contains(&"cancel".to_owned()),
        "{:?}",
        fake.commands()
    );
}

#[test]
fn run_in_terminal_reaches_the_sink_and_its_reply_carries_the_process_id() {
    let script = Script {
        run_in_terminal: true,
        ..Script::default()
    };
    let mut launch = options_with_breakpoint();
    launch.arguments = object(json!({ "args": ["--flag"], "cwd": "/work" }));
    let mut harness = start(script, launch);
    harness.wait("runInTerminal", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::RunInTerminal { .. }))
    });
    let at = harness
        .events
        .iter()
        .position(|event| matches!(event, DapEvent::RunInTerminal { .. }))
        .unwrap();
    let DapEvent::RunInTerminal {
        args,
        cwd,
        kind,
        title,
        reply,
        ..
    } = harness.events.remove(at)
    else {
        unreachable!()
    };
    assert_eq!(args, ["/fake/src/main.rs", "--flag"]);
    assert_eq!(cwd, "/work");
    assert_eq!(kind.as_deref(), Some("integrated"));
    assert_eq!(title.as_deref(), Some("fake program"));
    reply.started(TerminalStarted {
        process_id: Some(4242),
        shell_process_id: None,
    });
    let fake = harness.fake(0);
    let response = wait_client_response(&fake, "runInTerminal");
    assert!(response.success);
    let body = response.body::<crate::protocol::reverse::RunInTerminal>();
    assert_eq!(
        body.map(|body: RunInTerminalBody| body.process_id),
        Ok(Some(4242))
    );
    // The fake then goes on with the launch.
    harness.wait_state(SessionState::Stopped);
}

#[test]
fn a_dropped_terminal_reply_answers_a_failure() {
    let script = Script {
        run_in_terminal: true,
        ..Script::default()
    };
    let mut harness = start(script, options());
    harness.wait("runInTerminal", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::RunInTerminal { .. }))
    });
    harness.events.clear();
    let response = wait_client_response(&harness.fake(0), "runInTerminal");
    assert!(!response.success);
    assert_eq!(
        response.message.as_deref(),
        Some(tr!("dap-run-in-terminal-dropped").as_ref())
    );
}

#[test]
fn start_debugging_is_answered_with_an_error() {
    let script = Script {
        start_debugging: true,
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    let fake = harness.fake(0);
    let response = wait_client_response(&fake, "startDebugging");
    assert!(!response.success);
    assert_eq!(
        response.message.as_deref(),
        Some(tr!("dap-start-debugging-refused").as_ref())
    );
    assert_eq!(
        tr!("dap-start-debugging-refused").as_ref(),
        "child sessions are not supported"
    );
    // The session itself carries on.
    harness.wait_state(SessionState::Stopped);
}

#[test]
fn requests_fail_once_the_session_ends() {
    let mut harness = stopped(Script {
        faults: vec![Fault::new("evaluate", FaultKind::Unanswered)],
        ..Script::default()
    });
    let pending = evaluate(&harness, "watch");
    wait_requests(&harness.fake(0), "evaluate", 1);
    harness.session.stop();
    harness.wait_ended();
    let late = evaluate(&harness, "watch");
    for result in [async_io::block_on(pending), async_io::block_on(late)] {
        assert_eq!(
            result,
            Err(RequestError::Ended {
                command: "evaluate".into()
            })
        );
    }
}

#[test]
fn the_sessions_own_commands_are_refused_as_raw_requests() {
    let harness = super::session_harness::start_stopped(Script::default());
    let fake = harness.fake(0);
    let before = fake.commands().len();
    for command in [
        "initialize",
        "launch",
        "attach",
        "configurationDone",
        "restart",
        "terminate",
        "disconnect",
    ] {
        let refused = harness
            .session
            .raw_request::<serde_json::Value>(command, json!({}));
        assert_eq!(
            async_io::block_on(refused),
            Err(RequestError::SessionOwned {
                command: command.into()
            })
        );
    }
    round_trip(&harness);
    assert_eq!(fake.commands()[before..], ["threads"]);
    assert_eq!(harness.session.state(), SessionState::Stopped);
}
