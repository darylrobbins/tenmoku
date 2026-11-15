// SPDX-License-Identifier: Apache-2.0

//! Restart (docs/decisions/r1b9dw3g0g-debugger.md §2 item 7, D2 C6): with
//! `supportsRestartRequest` a successful `restart` reaches Running with a
//! fresh thread baseline and configures again on the adapter's new
//! `initialized`; a failed or unanswered one falls back to stopping the
//! adapter and starting another; and a completion arriving after Stop
//! changes nothing.

use super::session_harness::{
    Harness, advance_past, commands_after, options_with_breakpoint, start, wait_requests,
};
use crate::fake::{Fault, FaultKind, Script, Trigger};
use crate::session::{DapEvent, EndReason, REQUEST_TIMEOUT, SessionState};

/// A session stopped at the breakpoint, on `script`.
fn stopped(script: Script) -> Harness {
    let mut harness = start(script, options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    harness.wait_fetched(1);
    harness
}

/// The states from the first Restarting on.
fn states_from_restart(harness: &Harness) -> Vec<SessionState> {
    let states = harness.states();
    let at = states
        .iter()
        .position(|state| *state == SessionState::Restarting)
        .expect("never Restarting");
    states[at..].to_vec()
}

#[test]
fn a_successful_restart_reaches_running_with_a_fresh_baseline_and_resends_breakpoints() {
    let mut harness = stopped(Script::default());
    let fake = harness.fake(0);
    assert_eq!(harness.session.threads().len(), 1);
    harness.session.restart();
    // The fake answers, then sends `initialized` again; the program then
    // starts over and stops at the breakpoint again.
    harness.wait_state_times("the second stop", 2, |state| {
        *state == SessionState::Stopped
    });
    wait_requests(&fake, "configurationDone", 2);
    harness.wait_fetched(2);
    assert_eq!(
        states_from_restart(&harness),
        [
            SessionState::Restarting,
            SessionState::Running,
            SessionState::Stopped
        ]
    );
    // The fresh baseline, configuration again, and the new stop's fetch.
    assert_eq!(
        commands_after(&fake, "restart"),
        [
            "threads",
            "setBreakpoints",
            "setExceptionBreakpoints",
            "configurationDone",
            "threads",
            "stackTrace",
            "scopes",
            "variables"
        ]
    );
    let restart = &wait_requests(&fake, "restart", 1)[0];
    let arguments = restart
        .arguments::<crate::protocol::request::Restart>()
        .unwrap();
    assert_eq!(
        arguments.arguments.as_ref(),
        Some(&options_with_breakpoint().arguments)
    );
    // The baseline was cleared, then fetched fresh.
    let restarting = harness
        .events
        .iter()
        .position(|event| matches!(event, DapEvent::State(SessionState::Restarting)))
        .unwrap();
    assert!(
        harness.events[restarting..]
            .iter()
            .any(|event| matches!(event, DapEvent::Threads(threads) if threads.is_empty())),
        "the threads weren't cleared"
    );
    // The same adapter throughout.
    assert_eq!(harness.launcher.launched().len(), 1);
    harness.session.stop();
    harness.wait_ended();
}

/// After a restart that fell back: the old adapter was stopped and a new
/// one started, which stops at the breakpoint again.
fn assert_fell_back(harness: &mut Harness) {
    let first = harness.fake(0);
    let second = harness.fake(1);
    harness.wait_state_times("the second stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    let after = commands_after(&first, "restart");
    assert_eq!(after, ["terminate", "disconnect"], "{after:?}");
    first.wait_exit();
    // The baseline, then the stop's fetch.
    assert_eq!(
        second.commands(),
        [
            "initialize",
            "launch",
            "setBreakpoints",
            "setExceptionBreakpoints",
            "configurationDone",
            "threads",
            "threads",
            "stackTrace",
            "scopes",
            "variables"
        ]
    );
    let states = states_from_restart(harness);
    assert_eq!(
        states,
        [
            SessionState::Restarting,
            SessionState::Terminating,
            SessionState::Launching,
            SessionState::Initializing,
            SessionState::Configuring,
            SessionState::Running,
            SessionState::Stopped
        ]
    );
    assert!(harness.events.iter().any(
        |event| matches!(event, DapEvent::Failed(err) if err.to_string().contains("restart"))
    ));
    harness.session.stop();
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Stopped);
}

#[test]
fn a_failed_restart_falls_back_to_stop_and_start() {
    let script = Script {
        faults: vec![Fault::new("restart", FaultKind::Fail("no can do".into()))],
        ..Script::default()
    };
    let mut harness = stopped(script.clone());
    // The new adapter's script restarts fine; it isn't asked to.
    harness.launcher.set_script(Script::default());
    harness.session.restart();
    assert_fell_back(&mut harness);
}

#[test]
fn an_unanswered_restart_falls_back_after_its_timeout() {
    let script = Script {
        faults: vec![Fault::new("restart", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut harness = stopped(script);
    harness.launcher.set_script(Script::default());
    harness.session.restart();
    wait_requests(&harness.fake(0), "restart", 1);
    assert_eq!(harness.session.state(), SessionState::Restarting);
    advance_past(&harness.clock, REQUEST_TIMEOUT);
    assert_fell_back(&mut harness);
}

#[test]
fn without_restart_support_restart_is_stop_and_start() {
    let mut script = Script::default();
    script.capabilities.supports_restart_request = None;
    let mut harness = stopped(script.clone());
    harness.session.restart();
    let second = harness.fake(1);
    harness.wait_state_times("the second stop", 2, |state| {
        *state == SessionState::Stopped
    });
    wait_requests(&second, "threads", 1);
    let first = harness.fake(0);
    assert!(!first.commands().contains(&"restart".to_owned()));
    assert_eq!(
        commands_after(&first, "variables"),
        ["terminate", "disconnect"]
    );
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_restart_completion_after_stop_changes_nothing() {
    for kind in [
        // A success, with the `initialized` that follows it.
        FaultKind::ReplyAfter(Trigger::Terminate),
        // A failure, which would otherwise start a new adapter.
        FaultKind::FailAfter {
            trigger: Trigger::Terminate,
            message: "too late".into(),
        },
    ] {
        let script = Script {
            faults: vec![Fault::new("restart", kind.clone())],
            ..Script::default()
        };
        let mut harness = stopped(script);
        let fake = harness.fake(0);
        harness.session.restart();
        wait_requests(&fake, "restart", 1);
        harness.session.stop();
        let ended = harness.wait_ended();
        assert_eq!(ended.reason, EndReason::Stopped, "{kind:?}");
        fake.wait_exit();
        // The completion came after `terminate`, and was ignored: no
        // configuration, no baseline, no new adapter, no failure shown.
        assert!(
            fake.sent().iter().any(|message| matches!(
                message,
                crate::protocol::Message::Response(response) if response.command == "restart"
            )),
            "{kind:?}: the fake never answered the restart"
        );
        assert_eq!(
            commands_after(&fake, "restart"),
            ["terminate", "disconnect"],
            "{kind:?}"
        );
        assert_eq!(harness.launcher.launched().len(), 1, "{kind:?}");
        assert_eq!(
            states_from_restart(&harness),
            [
                SessionState::Restarting,
                SessionState::Terminating,
                SessionState::Ended(ended.clone())
            ],
            "{kind:?}"
        );
        assert!(
            !harness
                .events
                .iter()
                .any(|event| matches!(event, DapEvent::Failed(_))),
            "{kind:?}"
        );
    }
}

#[test]
fn an_earlier_baseline_reply_never_replaces_the_restarts() {
    // No `thread` events: the threads come from baselines only. The
    // startup baseline's reply (one thread) is held past the restart.
    let script = Script {
        thread_events: false,
        faults: vec![Fault::new(
            "threads",
            FaultKind::ReplyAfter(Trigger::Manual),
        )],
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    let fake = harness.fake(0);
    harness.wait_fetched(1);
    harness.session.restart();
    // The restart's baseline is answered at once, before the program
    // starts again: no threads. Then the second stop's fetch lists one.
    harness.wait_state_times("the second stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    let listed = |events: &[DapEvent]| {
        events
            .iter()
            .filter(|event| matches!(event, DapEvent::Threads(_)))
            .count()
    };
    let before = listed(&harness.events);
    let held = wait_requests(&fake, "threads", 1)[0].seq;
    fake.release();
    fake.wait_until("the held reply", |_, sent| {
        sent.iter().any(|message| {
            matches!(
                message,
                crate::protocol::Message::Response(response) if response.request_seq == held
            )
        })
    });
    // Everything the fake sent before this has been handled once it is
    // answered.
    round_trip(&harness);
    harness.drain();
    // The held reply changed nothing: the threads are the second stop's.
    assert_eq!(listed(&harness.events), before);
    assert_eq!(
        harness
            .session
            .threads()
            .iter()
            .map(|thread| thread.id)
            .collect::<Vec<_>>(),
        [crate::fake::THREAD_ID]
    );
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_restart_retires_the_last_runs_exit_code() {
    let script = Script {
        faults: vec![Fault::new("configurationDone", FaultKind::Exit(3)).nth(2)],
        ..Script::default()
    };
    let mut harness = stopped(script);
    let fake = harness.fake(0);
    // The old run reports its exit code, without ending the session.
    fake.send(crate::protocol::Message::Event(
        crate::protocol::EventMessage::new::<crate::protocol::event::Exited>(
            0,
            &crate::protocol::ExitedBody {
                exit_code: 5,
                ..Default::default()
            },
        ),
    ));
    harness.wait("the exit code", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::Exited(5)))
    });
    harness.session.restart();
    // The restarted run's adapter goes with no `exited` of its own.
    let ended = harness.wait_ended();
    assert!(
        matches!(ended.reason, EndReason::AdapterExited(_)),
        "{ended:?}"
    );
    assert_eq!(ended.exit_code, None);
}

/// A request the fake answers, awaited.
fn round_trip(harness: &Harness) {
    let source = harness
        .session
        .request::<crate::protocol::request::SourceRequest>(&crate::protocol::SourceArguments {
            source_reference: 1,
            ..Default::default()
        });
    async_io::block_on(source).expect("source is answered");
}

fn exited_event(code: i64) -> crate::protocol::Message {
    crate::protocol::Message::Event(crate::protocol::EventMessage::new::<
        crate::protocol::event::Exited,
    >(
        0,
        &crate::protocol::ExitedBody {
            exit_code: code,
            ..Default::default()
        },
    ))
}

/// A session stopped at the breakpoint, whose run reported exit code 5;
/// its `restart` reply is held, and its adapter exits by itself on the
/// restarted run's `configurationDone`.
fn restarting_after_an_exit_code() -> Harness {
    let script = Script {
        faults: vec![
            Fault::new("restart", FaultKind::ReplyAfter(Trigger::Manual)),
            Fault::new("configurationDone", FaultKind::Exit(3)).nth(2),
        ],
        ..Script::default()
    };
    let mut harness = stopped(script);
    let fake = harness.fake(0);
    fake.send(exited_event(5));
    harness.wait("the exit code", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::Exited(5)))
    });
    harness.session.restart();
    wait_requests(&fake, "restart", 1);
    harness
}

#[test]
fn an_exit_code_reported_before_the_restart_reply_is_the_new_runs() {
    let mut harness = restarting_after_an_exit_code();
    let fake = harness.fake(0);
    fake.send(exited_event(9));
    harness.wait("the new exit code", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::Exited(9)))
    });
    fake.release();
    let ended = harness.wait_ended();
    assert!(
        matches!(ended.reason, EndReason::AdapterExited(_)),
        "{ended:?}"
    );
    assert_eq!(ended.exit_code, Some(9));
}

#[test]
fn a_restart_retires_the_exit_code_even_when_its_reply_is_ignored() {
    let mut harness = restarting_after_an_exit_code();
    let fake = harness.fake(0);
    // The restarted run stops before the reply, so the guard ignores it.
    fake.send(crate::protocol::Message::Event(
        crate::protocol::EventMessage::new::<crate::protocol::event::Stopped>(
            0,
            &crate::protocol::StoppedBody {
                reason: "entry".into(),
                thread_id: Some(crate::fake::THREAD_ID),
                ..Default::default()
            },
        ),
    ));
    harness.wait_state_times("the stop before the reply", 2, |state| {
        *state == SessionState::Stopped
    });
    fake.release();
    let ended = harness.wait_ended();
    assert!(
        matches!(ended.reason, EndReason::AdapterExited(_)),
        "{ended:?}"
    );
    assert_eq!(ended.exit_code, None);
}
