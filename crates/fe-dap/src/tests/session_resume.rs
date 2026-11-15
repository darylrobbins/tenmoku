// SPDX-License-Identifier: Apache-2.0

//! Resumes and stop epochs (docs/decisions/r1b9dw3g0g-debugger.md §2
//! items 4-5; §11 D2 C2, and C3 as amended by §13 F2): a step moves to
//! Running on its response with no `continued` needed, stepping is off
//! from the send on, the old stop's ids answer `Stale` locally; a stop
//! after the step's response, or before it, ends Stopped at a new epoch;
//! and a reply from before a step never fills the new stop's data.

use serde_json::Value;

use super::session_harness::{
    Harness, breakpoint_line, locals, options_with_breakpoint, requests, round_trip, start,
    start_stopped, states_from, wait_requests,
};
use crate::fake::{Fault, FaultKind, Script, THREAD_ID, Trigger};
use crate::protocol::{Message, request};
use crate::session::{Control, DapEvent, Disabled, RequestError, Resume, SessionState, StopData};

fn stop_data(harness: &Harness) -> StopData {
    harness.session.stop_data().expect("the session is stopped")
}

fn stale(command: &str) -> RequestError {
    RequestError::Stale {
        command: command.into(),
    }
}

/// The `variables` requests `fake` got for `reference`.
fn variables_for(fake: &crate::fake::FakeAdapter, reference: i64) -> usize {
    requests(fake, "variables")
        .iter()
        .filter(|request| {
            request
                .arguments::<request::Variables>()
                .is_ok_and(|args| args.variables_reference == reference)
        })
        .count()
}

/// The `Variables` events so far.
fn variables_events(harness: &Harness) -> Vec<(crate::session::StopRef, Vec<String>)> {
    harness
        .events
        .iter()
        .filter_map(|event| match event {
            DapEvent::Variables {
                reference,
                variables,
            } => Some((
                *reference,
                variables
                    .iter()
                    .map(|variable| variable.name.clone())
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_step_without_continued_runs_on_its_response() {
    let script = Script {
        continued_events: false,
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    let before = stop_data(&harness);
    let old_locals = before.scope("Locals").unwrap();
    assert_eq!(variables_for(&fake, old_locals.id), 1);
    assert!(harness.session.controls().step.is_enabled());

    assert!(harness.session.resume(Resume::Next, None));
    // From the send on: stepping and Continue are off, and a second step
    // is refused.
    let controls = harness.session.controls();
    assert_eq!(controls.step, Control::Disabled(Disabled::Resuming));
    assert_eq!(controls.continue_, Control::Disabled(Disabled::Resuming));
    assert!(controls.stop.is_enabled());
    assert!(!harness.session.resume(Resume::StepIn, None));
    // The old reference answers Stale, with no request.
    assert_eq!(
        async_io::block_on(harness.session.variables(old_locals)),
        Err(stale("variables"))
    );

    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    // Running on the step's response: the fake sent no `continued`.
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Running,
            SessionState::Stopped
        ]
    );
    assert!(
        !fake
            .sent()
            .iter()
            .any(|message| matches!(message, Message::Event(event) if event.event == "continued"))
    );
    assert!(
        !harness
            .events
            .iter()
            .any(|event| matches!(event, DapEvent::Continued(_)))
    );
    // One `next`, on the stopped thread; the old reference was never asked
    // for again.
    let next = &requests(&fake, "next");
    assert_eq!(next.len(), 1);
    assert_eq!(
        next[0].arguments::<request::Next>().unwrap().thread_id,
        THREAD_ID
    );
    assert_eq!(variables_for(&fake, old_locals.id), 1);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_steps_response_then_stopped_ends_stopped_at_a_new_epoch() {
    // The conformant order, `continued` events included.
    let mut harness = start_stopped(Script::default());
    let fake = harness.fake(0);
    let first = stop_data(&harness);
    assert_eq!(first.epoch, harness.session.epoch());
    assert!(harness.session.resume(Resume::Next, None));
    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);

    // The response went before the `stopped`.
    let sent: Vec<String> = fake
        .sent()
        .iter()
        .filter_map(|message| match message {
            Message::Response(response) if response.command == "next" => Some("next".into()),
            Message::Event(event) if event.event == "stopped" => Some("stopped".into()),
            _ => None,
        })
        .collect();
    assert_eq!(sent, ["stopped", "next", "stopped"]);

    let second = stop_data(&harness);
    assert!(second.epoch > first.epoch, "{second:?}");
    assert_eq!(second.epoch, harness.session.epoch());
    assert_eq!(
        second.stopped.as_ref().map(|body| body.reason.as_str()),
        Some("step")
    );
    assert_eq!(second.thread_id, Some(THREAD_ID));
    assert_eq!(second.frames[0].line, breakpoint_line() + 1);
    assert_eq!(locals(&second), ["a", "b"]);
    assert_eq!(harness.session.state(), SessionState::Stopped);
    // The first stop's ids are over.
    assert_eq!(
        async_io::block_on(harness.session.scopes(first.top_frame().unwrap())),
        Err(stale("scopes"))
    );
    // The new stop's are good.
    let children = async_io::block_on(harness.session.variables(second.scope("Locals").unwrap()))
        .expect("the new stop's locals");
    assert_eq!(children.variables.len(), 2);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_stopped_before_the_steps_response_ends_stopped_and_the_late_response_changes_nothing() {
    let script = Script {
        stopped_before_step_response: true,
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    let first = stop_data(&harness);
    assert!(harness.session.resume(Resume::Next, None));
    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    // The fake sent the `stopped` first, then the response, then nothing
    // before the fetch's replies: once the fetch is in, so is the response.
    harness.wait_fetched(2);
    let sent: Vec<String> = fake
        .sent()
        .iter()
        .filter_map(|message| match message {
            Message::Response(response) if response.command == "next" => Some("next".into()),
            Message::Event(event) if event.event == "stopped" => Some("stopped".into()),
            _ => None,
        })
        .collect();
    assert_eq!(sent, ["stopped", "stopped", "next"]);
    harness.drain();
    // Never Running: the late response failed the guard.
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Stopped
        ]
    );
    assert_eq!(harness.session.state(), SessionState::Stopped);
    let second = stop_data(&harness);
    assert!(second.epoch > first.epoch);
    assert_eq!(second.epoch, harness.session.epoch());
    assert_eq!(locals(&second), ["a", "b"]);
    assert!(harness.session.controls().step.is_enabled());
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_variables_reply_delayed_past_a_step_is_dropped() {
    // The first stop's `Locals` reply is held until the step is answered.
    let script = Script {
        faults: vec![Fault::new(
            "variables",
            FaultKind::ReplyAfter(Trigger::NextResume),
        )],
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    let fake = harness.fake(0);
    wait_requests(&fake, "variables", 1);
    let first = stop_data(&harness);
    assert!(first.variables.is_empty());

    assert!(harness.session.resume(Resume::Next, None));
    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    // The held reply went out after the step's events, before the new
    // fetch's: once the new stop's variables are in, it has been handled.
    harness.wait_fetched(1);
    assert_eq!(requests(&fake, "variables").len(), 2);
    let second = stop_data(&harness);
    // Only the new stop's reply filled anything: `add`'s locals, not the
    // held `main` ones.
    assert_eq!(
        variables_events(&harness),
        [(
            second.scope("Locals").unwrap(),
            vec!["a".to_owned(), "b".to_owned()]
        )]
    );
    assert_eq!(second.variables.len(), 1);
    assert_eq!(locals(&second), ["a", "b"]);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_hosts_variables_request_in_flight_answers_stale_when_a_step_is_sent() {
    // The host's own `variables` (the second) is held until the test lets
    // it go.
    let script = Script {
        faults: vec![Fault::new("variables", FaultKind::ReplyAfter(Trigger::Manual)).nth(2)],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    let first = stop_data(&harness);
    let children = harness.session.variables(first.scope("Locals").unwrap());
    wait_requests(&fake, "variables", 2);
    assert!(harness.session.resume(Resume::Next, None));
    // Answered when the step is sent, with its reply still to come.
    assert_eq!(async_io::block_on(children), Err(stale("variables")));
    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    // The reply comes, with the old stop's values, and changes nothing.
    let held = requests(&fake, "variables")[1].seq;
    fake.release();
    fake.wait_until("the held reply", |_, sent| {
        sent.iter().any(|message| {
            matches!(message, Message::Response(response)
                if response.request_seq == held && response.success)
        })
    });
    round_trip(&harness);
    assert_eq!(locals(&stop_data(&harness)), ["a", "b"]);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn more_frames_and_children_come_on_request_for_the_current_stop() {
    let mut harness = start_stopped(Script::default());
    let fake = harness.fake(0);
    let data = stop_data(&harness);
    // The first page was asked for, and nothing deeper than the
    // non-expensive scope.
    let trace = requests(&fake, "stackTrace")[0]
        .arguments::<request::StackTrace>()
        .unwrap();
    assert_eq!(trace.start_frame, Some(0));
    assert_eq!(trace.levels, Some(crate::session::FRAME_PAGE));
    assert_eq!(requests(&fake, "variables").len(), 1);
    assert!(data.scopes.iter().any(|scope| scope.expensive));
    assert!(
        data.scope("Globals")
            .is_some_and(|globals| !data.variables.contains_key(&globals.id))
    );
    // `point`'s children only when asked.
    let point = data.variables[&data.scope("Locals").unwrap().id]
        .iter()
        .find(|variable| variable.name == "point")
        .unwrap()
        .variables_reference;
    assert!(point > 0);
    let children = async_io::block_on(harness.session.variables(data.stop_ref(point))).unwrap();
    assert_eq!(
        children
            .variables
            .iter()
            .map(|variable| variable.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "y"]
    );
    let more = async_io::block_on(harness.session.frames(data.epoch, THREAD_ID, 0)).unwrap();
    assert_eq!(more.stack_frames.len(), data.frames.len());
    // A raw request carries no epoch, and goes.
    let raw: Value = async_io::block_on(harness.session.raw_request(
        "scopes",
        serde_json::json!({ "frameId": data.frames[0].id }),
    ))
    .unwrap();
    assert!(raw.get("scopes").is_some());
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn typed_and_raw_stop_data_requests_are_of_the_stop_they_were_sent_in() {
    // The host's own `variables` and frameless `evaluate` (each the
    // second) are held until the test lets them go.
    let script = Script {
        faults: vec![
            Fault::new("variables", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
            Fault::new("evaluate", FaultKind::ReplyAfter(Trigger::Manual)),
        ],
        ..Script::default()
    };
    let harness = start_stopped(script);
    let fake = harness.fake(0);
    let data = stop_data(&harness);
    let locals = data.scope("Locals").unwrap().id;
    // No `StopRef`: taken as the current stop's.
    let typed =
        harness
            .session
            .request::<request::Variables>(&crate::protocol::VariablesArguments {
                variables_reference: locals,
                ..Default::default()
            });
    let frameless =
        harness
            .session
            .request::<request::Evaluate>(&crate::protocol::EvaluateArguments {
                expression: "COUNT".into(),
                context: Some("watch".into()),
                ..Default::default()
            });
    wait_requests(&fake, "variables", 2);
    wait_requests(&fake, "evaluate", 1);
    assert!(harness.session.resume(Resume::Next, None));
    assert_eq!(async_io::block_on(typed), Err(stale("variables")));
    assert_eq!(async_io::block_on(frameless), Err(stale("evaluate")));
    fake.release();
    harness.session.stop();
}

#[test]
fn while_running_only_a_frameless_evaluate_goes() {
    let mut harness = start(
        super::session_harness::spinning(Script::default()),
        super::session_harness::options(),
    );
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Running);
    for (command, arguments) in [
        ("stackTrace", serde_json::json!({ "threadId": THREAD_ID })),
        ("scopes", serde_json::json!({ "frameId": 1000 })),
        ("variables", serde_json::json!({ "variablesReference": 1 })),
        (
            "evaluate",
            serde_json::json!({ "expression": "x", "frameId": 1000 }),
        ),
    ] {
        let refused = harness.session.raw_request::<Value>(command, arguments);
        assert_eq!(
            async_io::block_on(refused),
            Err(RequestError::NotStopped {
                command: command.into()
            })
        );
    }
    round_trip(&harness);
    for command in ["stackTrace", "scopes", "variables", "evaluate"] {
        assert!(requests(&fake, command).is_empty(), "{command} went");
    }
    // Some adapters evaluate while running: that one goes, about no stop.
    let running = harness.session.evaluate(
        &crate::protocol::EvaluateArguments {
            expression: "COUNT".into(),
            context: Some("repl".into()),
            ..Default::default()
        },
        None,
    );
    assert_eq!(async_io::block_on(running).unwrap().result, "3");
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_dropped_resume_future_cancels_and_its_answer_still_goes_through_the_guard() {
    // The `next` is answered once it is cancelled.
    let script = Script {
        continued_events: false,
        faults: vec![Fault::new("next", FaultKind::ReplyAfter(Trigger::Cancel))],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    drop(
        harness
            .session
            .raw_request::<Value>("next", serde_json::json!({ "threadId": THREAD_ID })),
    );
    // Cancelled at once, with no timeout passed.
    let next = wait_requests(&fake, "next", 1)[0].seq;
    let cancel = &wait_requests(&fake, "cancel", 1)[0];
    assert_eq!(
        cancel.arguments::<request::Cancel>().unwrap().request_id,
        Some(next)
    );
    // Still the pending resume: its answer moves to Running, and the step's
    // stop follows.
    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Running,
            SessionState::Stopped
        ]
    );
    harness.session.stop();
    harness.wait_ended();
}

/// A session on `script` whose sink, on an `output` event, holds the
/// actor until the test lets it go: what the test asks meanwhile is
/// queued, read against the view as it was.
struct Held {
    session: crate::session::Session,
    fake: crate::fake::FakeAdapter,
    blocked: std::sync::mpsc::Receiver<()>,
    go: std::sync::mpsc::Sender<()>,
}

fn held(script: Script) -> Held {
    use std::sync::{Arc, Mutex, mpsc};
    let launcher = Arc::new(crate::fake::FakeLauncher::new(script));
    let clock = Arc::new(crate::clock::ManualClock::new());
    let (blocked_tx, blocked) = mpsc::channel();
    let (go, go_rx) = mpsc::channel::<()>();
    let go_rx = Mutex::new(go_rx);
    let session = crate::session::Session::start(
        options_with_breakpoint(),
        launcher.clone(),
        clock,
        move |event| {
            if matches!(event, DapEvent::Output(_)) {
                let _ = blocked_tx.send(());
                let _ = go_rx.lock().unwrap().recv();
            }
        },
    )
    .unwrap();
    let fake = launcher.wait_for_launch(0);
    Held {
        session,
        fake,
        blocked,
        go,
    }
}

/// Polls until `done` holds, for up to 10 s.
fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "never got to {what}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn a_request_is_of_the_stop_it_was_asked_at_even_if_that_stop_ends_while_it_is_queued() {
    let held = held(Script::default());
    let session = &held.session;
    until("the stop's data", || {
        session
            .stop_data()
            .is_some_and(|data| !data.variables.is_empty())
    });
    let data = session.stop_data().unwrap();
    let locals = data.scope("Locals").unwrap().id;
    // Hold the actor, then ask: the resume and the requests all wait for
    // it, the requests asked while the view still shows the stop.
    held.fake
        .send(Message::Event(crate::protocol::EventMessage::new::<
            crate::protocol::event::Output,
        >(
            0,
            &crate::protocol::OutputBody {
                category: Some("console".into()),
                output: "hold\n".into(),
                ..Default::default()
            },
        )));
    held.blocked.recv().unwrap();
    assert!(session.resume(Resume::Next, None));
    let typed = session.request::<request::Variables>(&crate::protocol::VariablesArguments {
        variables_reference: locals,
        ..Default::default()
    });
    let raw = session.raw_request::<Value>(
        "scopes",
        serde_json::json!({ "frameId": data.frames[0].id }),
    );
    let frameless = session.request::<request::Evaluate>(&crate::protocol::EvaluateArguments {
        expression: "COUNT".into(),
        context: Some("watch".into()),
        ..Default::default()
    });
    held.go.send(()).unwrap();
    // The actor takes the resume first, so the stop is over when it takes
    // the requests: they are of that stop, not the next.
    assert_eq!(async_io::block_on(typed), Err(stale("variables")));
    assert_eq!(async_io::block_on(raw), Err(stale("scopes")));
    assert_eq!(async_io::block_on(frameless), Err(stale("evaluate")));
    // None reached the adapter: one `variables` and `scopes` per stop's
    // fetch, and no `evaluate`.
    until("the step's stop's data", || {
        session
            .stop_data()
            .is_some_and(|stop| stop.epoch > data.epoch && !stop.variables.is_empty())
    });
    assert_eq!(requests(&held.fake, "scopes").len(), 2);
    assert_eq!(requests(&held.fake, "variables").len(), 2);
    assert!(requests(&held.fake, "evaluate").is_empty());
    session.stop();
}
