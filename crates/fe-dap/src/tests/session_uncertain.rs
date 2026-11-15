// SPDX-License-Identifier: Apache-2.0

//! The resume guard and Uncertain (docs/decisions/r1b9dw3g0g-debugger.md §2
//! item 4; §13 F2, which wins over §2's "a failure returns to Stopped";
//! §11 D2 C4): a failed or cancelled resume leaves the session Uncertain
//! until a stop is proven, by a `stopped` event or a probe the adapter
//! answers; nothing about the stop is asked for meanwhile; a probe
//! answered `notStopped` is Running; a resume that goes unanswered is
//! Uncertain after its timeout, and its late reply, or a `stopped`, ends
//! that; and a late completion after a newer resume, or Stop, changes
//! nothing.

use serde_json::Value;

use super::session_harness::{
    Harness, advance_past, locals, options_with_breakpoint, program_path, requests, round_trip,
    start, start_stopped, start_stopped_with, states_from, stopped_event, trusting_stacks,
    wait_requests,
};
use crate::fake::{Fault, FaultKind, Program, Script, THREAD_ID, Trigger};
use crate::protocol::{EvaluateArguments, Message, SourceBreakpoint, request};
use crate::session::{
    Control, Controls, DapEvent, Disabled, EndReason, REQUEST_TIMEOUT, RequestError, Resume,
    SessionState, StopData,
};

fn stop_data(harness: &Harness) -> StopData {
    harness.session.stop_data().expect("the session is stopped")
}

fn not_stopped(command: &str) -> RequestError {
    RequestError::NotStopped {
        command: command.into(),
    }
}

/// How many requests of each stop-data command `fake` has had.
fn asked(fake: &crate::fake::FakeAdapter) -> [usize; 3] {
    ["scopes", "variables", "evaluate"].map(|command| requests(fake, command).len())
}

/// Waits for a `Failed` event that `matches` holds of.
fn wait_failed(harness: &mut Harness, matches: impl Fn(&RequestError) -> bool) {
    harness.wait("the failure shown", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::Failed(err) if matches(err)))
    });
}

/// Uncertain's controls: Continue, Pause and Stop, and no stepping.
fn assert_uncertain_controls(controls: Controls) {
    assert!(controls.continue_.is_enabled(), "{controls:?}");
    assert!(controls.pause.is_enabled(), "{controls:?}");
    assert!(controls.stop.is_enabled(), "{controls:?}");
    assert_eq!(controls.step, Control::Disabled(Disabled::Uncertain));
    assert_eq!(controls.restart, Control::Disabled(Disabled::Uncertain));
}

/// Nothing about the stop goes while Uncertain: the old ids are stale,
/// and a frameless evaluation or a raw `scopes` is refused locally.
fn assert_nothing_asked(harness: &Harness, old: &StopData) {
    let fake = harness.fake(0);
    let before = asked(&fake);
    assert_eq!(harness.session.stop_data(), None);
    assert_eq!(
        async_io::block_on(harness.session.variables(old.scope("Locals").unwrap())),
        Err(RequestError::Stale {
            command: "variables".into()
        })
    );
    let watch = EvaluateArguments {
        expression: "x".into(),
        context: Some("watch".into()),
        ..Default::default()
    };
    assert_eq!(
        async_io::block_on(harness.session.evaluate(&watch, None)),
        Err(not_stopped("evaluate"))
    );
    assert_eq!(
        async_io::block_on(harness.session.evaluate(&watch, old.top_frame())),
        Err(RequestError::Stale {
            command: "evaluate".into()
        })
    );
    let scopes = harness
        .session
        .raw_request::<Value>("scopes", serde_json::json!({ "frameId": old.frames[0].id }));
    assert_eq!(async_io::block_on(scopes), Err(not_stopped("scopes")));
    // Nor does the probe's stack count as data.
    let trace = harness
        .session
        .raw_request::<Value>("stackTrace", serde_json::json!({ "threadId": THREAD_ID }));
    assert_eq!(async_io::block_on(trace), Err(not_stopped("stackTrace")));
    round_trip(harness);
    assert_eq!(asked(&fake), before);
}

#[test]
fn a_failed_or_cancelled_resume_leaves_the_session_uncertain_until_a_probe_proves_the_stop() {
    for (resume, message) in [
        (Resume::Next, "the step failed"),
        (Resume::Continue, "cancelled"),
        (Resume::StepOut, "the step failed"),
    ] {
        let command = resume.command();
        // The resume fails; the probe's `stackTrace` (the stop's fetch had
        // the first) is held until the test lets it go.
        let script = Script {
            faults: vec![
                Fault::new(command, FaultKind::Fail(message.into())),
                Fault::new("stackTrace", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
            ],
            ..Script::default()
        };
        let mut harness = start_stopped_with(script, trusting_stacks());
        let fake = harness.fake(0);
        let old = stop_data(&harness);
        let before = asked(&fake);

        assert!(harness.session.resume(resume, None), "{command}");
        wait_failed(&mut harness, |err| err.message() == Some(message));
        // The probe: `threads`, then a one-frame `stackTrace`, held.
        let probe = &wait_requests(&fake, "stackTrace", 2)[1];
        let probe = probe.arguments::<request::StackTrace>().unwrap();
        assert_eq!((probe.thread_id, probe.levels), (THREAD_ID, Some(1)));
        harness.drain();
        assert_eq!(
            harness.session.state(),
            SessionState::Uncertain,
            "{command}"
        );
        assert_eq!(
            states_from(&harness, 1, |state| *state == SessionState::Stopped),
            [
                SessionState::Stopped,
                SessionState::Resuming,
                SessionState::Uncertain
            ],
            "{command}"
        );
        assert_uncertain_controls(harness.session.controls());
        assert_nothing_asked(&harness, &old);
        assert_eq!(asked(&fake), before, "{command}");

        // The adapter gives the stack: stopped, fetched afresh under a new
        // epoch.
        fake.release();
        harness.wait_state_times("the proven stop", 2, |state| {
            *state == SessionState::Stopped
        });
        harness.wait_fetched(2);
        let proven = stop_data(&harness);
        assert!(proven.epoch > old.epoch, "{command}");
        assert_eq!(proven.epoch, harness.session.epoch());
        assert_eq!(proven.stopped, None);
        assert_eq!(proven.thread_id, Some(THREAD_ID));
        // The program never moved.
        assert_eq!(proven.frames[0].line, old.frames[0].line);
        assert_eq!(locals(&proven), locals(&old));
        assert_eq!(
            asked(&fake),
            [before[0] + 1, before[1] + 1, before[2]],
            "{command}"
        );
        assert!(harness.session.controls().step.is_enabled());
        harness.session.stop();
        harness.wait_ended();
    }
}

#[test]
fn a_probe_answered_not_stopped_moves_to_running() {
    // The program runs, by the adapter's word: on the probe's `threads`
    // (the third: the baseline's and the fetch's came first) or its
    // `stackTrace` (the second).
    for probe in [
        Fault::new("threads", FaultKind::Fail("notStopped".into())).nth(3),
        Fault::new("stackTrace", FaultKind::Fail("notStopped".into())).nth(2),
    ] {
        let script = Script {
            faults: vec![
                Fault::new("next", FaultKind::Fail("the step failed".into())),
                probe.clone(),
            ],
            ..Script::default()
        };
        let mut harness = start_stopped(script);
        let fake = harness.fake(0);
        let before = asked(&fake);
        assert!(harness.session.resume(Resume::Next, None));
        // Running at startup, then again.
        harness.wait_state_times("running", 2, |state| *state == SessionState::Running);
        assert_eq!(
            states_from(&harness, 1, |state| *state == SessionState::Stopped),
            [
                SessionState::Stopped,
                SessionState::Resuming,
                SessionState::Uncertain,
                SessionState::Running
            ],
            "{probe:?}"
        );
        round_trip(&harness);
        assert_eq!(asked(&fake), before, "{probe:?}");
        assert_eq!(harness.session.stop_data(), None);
        let controls = harness.session.controls();
        assert!(controls.pause.is_enabled());
        assert_eq!(controls.continue_, Control::Disabled(Disabled::Running));
        assert_eq!(controls.step, Control::Disabled(Disabled::Running));
        harness.session.stop();
        harness.wait_ended();
    }
}

#[test]
fn a_resume_failing_not_stopped_is_running_with_no_probe() {
    let script = Script {
        faults: vec![Fault::new("continue", FaultKind::Fail("notStopped".into()))],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    assert!(harness.session.resume(Resume::Continue, None));
    harness.wait_state_times("running", 2, |state| *state == SessionState::Running);
    round_trip(&harness);
    assert_eq!(requests(&fake, "stackTrace").len(), 1);
    assert_eq!(requests(&fake, "threads").len(), 2);
    assert!(
        !harness.states().contains(&SessionState::Uncertain),
        "{:?}",
        harness.states()
    );
}

#[test]
fn a_stopped_event_proves_the_stop_and_the_probes_late_reply_changes_nothing() {
    let script = Script {
        faults: vec![
            Fault::new("next", FaultKind::Fail("the step failed".into())),
            Fault::new("stackTrace", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
        ],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    let old = stop_data(&harness);
    assert!(harness.session.resume(Resume::Next, None));
    wait_requests(&fake, "stackTrace", 2);
    harness.wait_state(SessionState::Uncertain);
    fake.send(stopped_event("pause"));
    harness.wait_state_times("the stop", 2, |state| *state == SessionState::Stopped);
    harness.wait_fetched(2);
    let stopped = stop_data(&harness);
    assert!(stopped.epoch > old.epoch);
    assert_eq!(
        stopped.stopped.as_ref().map(|body| body.reason.as_str()),
        Some("pause")
    );
    // The probe's reply comes now, and changes nothing.
    fake.release();
    let held = requests(&fake, "stackTrace")[1].seq;
    fake.wait_until("the probe's reply", |_, sent| {
        sent.iter().any(
            |message| matches!(message, Message::Response(response) if response.request_seq == held),
        )
    });
    round_trip(&harness);
    harness.drain();
    assert_eq!(
        states_from(&harness, 2, |state| *state == SessionState::Stopped),
        [SessionState::Stopped]
    );
    assert_eq!(harness.session.epoch(), stopped.epoch);
    assert_eq!(requests(&fake, "stackTrace").len(), 3);
    harness.session.stop();
    harness.wait_ended();
}

/// Options with a second breakpoint, on the program's last line.
fn two_breakpoints() -> crate::session::SessionOptions {
    let mut options = options_with_breakpoint();
    let last = Program::default().line_of("println");
    options
        .breakpoints
        .get_mut(&program_path())
        .unwrap()
        .push(SourceBreakpoint {
            line: last,
            ..Default::default()
        });
    options
}

/// Sends `next`, lets it time out, and checks Uncertain's controls.
fn time_out_a_next(harness: &mut Harness) {
    let fake = harness.fake(0);
    assert!(harness.session.resume(Resume::Next, None));
    wait_requests(&fake, "next", 1);
    harness.wait_state(SessionState::Resuming);
    advance_past(&harness.clock, REQUEST_TIMEOUT);
    harness.wait_state(SessionState::Uncertain);
    wait_failed(harness, |err| {
        *err == RequestError::Timeout {
            command: "next".into(),
            after: REQUEST_TIMEOUT,
        }
    });
    // It is cancelled.
    let next = requests(&fake, "next")[0].seq;
    let cancel = &wait_requests(&fake, "cancel", 1)[0];
    assert_eq!(
        cancel.arguments::<request::Cancel>().unwrap().request_id,
        Some(next)
    );
}

#[test]
fn a_next_failure_delayed_past_a_later_continue_changes_nothing() {
    // The `next` fails only once the next resume is answered.
    let script = Script {
        faults: vec![Fault::new(
            "next",
            FaultKind::FailAfter {
                trigger: Trigger::NextResume,
                message: "cancelled".into(),
            },
        )],
        ..Script::default()
    };
    let mut harness = start(script, two_breakpoints());
    harness.wait_state(SessionState::Stopped);
    harness.wait_fetched(1);
    let fake = harness.fake(0);
    time_out_a_next(&mut harness);
    // Continue replaces the pending resume, and runs to the second
    // breakpoint.
    assert!(harness.session.resume(Resume::Continue, None));
    harness.wait_state_times("the second breakpoint", 2, |state| {
        *state == SessionState::Stopped
    });
    let next = requests(&fake, "next")[0].seq;
    fake.wait_until("the next's late failure", |_, sent| {
        sent.iter().any(|message| {
            matches!(message, Message::Response(response)
                if response.request_seq == next && !response.success)
        })
    });
    round_trip(&harness);
    harness.wait_fetched(2);
    harness.drain();
    assert_eq!(
        states_from(&harness, 2, |state| *state == SessionState::Stopped),
        [SessionState::Stopped]
    );
    assert_eq!(harness.session.state(), SessionState::Stopped);
    let data = stop_data(&harness);
    assert_eq!(data.epoch, harness.session.epoch());
    assert_eq!(data.frames[0].line, Program::default().line_of("println"));
    // No probe: the two stops' fetches only.
    assert_eq!(requests(&fake, "stackTrace").len(), 2);
    assert!(
        !harness.events.iter().any(
            |event| matches!(event, DapEvent::Failed(err) if err.message() == Some("cancelled"))
        ),
        "the late failure was shown"
    );
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends() {
    let script = Script {
        faults: vec![Fault::new(
            "next",
            FaultKind::FailAfter {
                trigger: Trigger::Terminate,
                message: "cancelled".into(),
            },
        )],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    assert!(harness.session.resume(Resume::Next, None));
    wait_requests(&fake, "next", 1);
    harness.wait_state(SessionState::Resuming);
    harness.session.stop();
    assert_eq!(harness.session.controls(), Controls::none(Disabled::Ending));
    let ended = harness.wait_ended();
    assert_eq!(ended.reason, EndReason::Stopped);
    let next = requests(&fake, "next")[0].seq;
    assert!(fake.sent().iter().any(|message| {
        matches!(message, Message::Response(response)
            if response.request_seq == next && !response.success)
    }));
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Resuming),
        [
            SessionState::Resuming,
            SessionState::Terminating,
            SessionState::Ended(ended)
        ]
    );
    assert_eq!(harness.session.controls(), Controls::none(Disabled::Ended));
}

#[test]
fn a_next_never_answered_is_uncertain_after_its_timeout_and_a_stopped_event_ends_it() {
    let script = Script {
        faults: vec![Fault::new("next", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    let fake = harness.fake(0);
    let old = stop_data(&harness);
    time_out_a_next(&mut harness);
    assert_uncertain_controls(harness.session.controls());
    assert!(!harness.session.resume(Resume::Next, None));
    assert_nothing_asked(&harness, &old);
    // The adapter didn't answer: no probe.
    assert_eq!(requests(&fake, "stackTrace").len(), 1);

    fake.send(stopped_event("step"));
    harness.wait_state_times("the stop", 2, |state| *state == SessionState::Stopped);
    harness.wait_fetched(2);
    let stopped = stop_data(&harness);
    assert!(stopped.epoch > old.epoch);
    assert_eq!(locals(&stopped), locals(&old));
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Uncertain,
            SessionState::Stopped
        ]
    );
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_next_never_answered_then_its_late_success_moves_to_running() {
    let script = Script {
        continued_events: false,
        faults: vec![Fault::new("next", FaultKind::ReplyAfter(Trigger::Manual))],
        ..Script::default()
    };
    let mut harness = start_stopped(script);
    time_out_a_next(&mut harness);
    assert_uncertain_controls(harness.session.controls());
    // The answer comes after all, and the step's stop with it.
    harness.fake(0).release();
    harness.wait_state_times("the step's stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Uncertain,
            SessionState::Running,
            SessionState::Stopped
        ]
    );
    assert_eq!(locals(&stop_data(&harness)), ["a", "b"]);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_next_never_answered_then_its_late_cancelled_failure_stays_uncertain_until_a_stop_is_proven() {
    // The session's `cancel` lets the failure go; the probe's `stackTrace`
    // is held.
    let script = Script {
        faults: vec![
            Fault::new(
                "next",
                FaultKind::FailAfter {
                    trigger: Trigger::Cancel,
                    message: "cancelled".into(),
                },
            ),
            Fault::new("stackTrace", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
        ],
        ..Script::default()
    };
    let mut harness = start_stopped_with(script, trusting_stacks());
    let fake = harness.fake(0);
    let old = stop_data(&harness);
    let before = asked(&fake);
    time_out_a_next(&mut harness);
    wait_failed(&mut harness, |err| err.message() == Some("cancelled"));
    wait_requests(&fake, "stackTrace", 2);
    harness.drain();
    // Not Stopped: the failure proves nothing.
    assert_eq!(harness.session.state(), SessionState::Uncertain);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Uncertain
        ]
    );
    assert_uncertain_controls(harness.session.controls());
    assert_nothing_asked(&harness, &old);
    assert_eq!(asked(&fake), before);
    // The probe proves the stop.
    fake.release();
    harness.wait_state_times("the proven stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    let proven = stop_data(&harness);
    assert!(proven.epoch > old.epoch);
    assert_eq!(proven.stopped, None);
    assert_eq!(locals(&proven), locals(&old));
    harness.session.stop();
    harness.wait_ended();
}

/// A `next` that fails, from `options`, with the probe's `stackTrace` (the
/// second) answered as `probe` says, or by the fake (a stack: it is
/// stopped); the session has handled that answer on return.
fn probed(probe: Option<FaultKind>, options: crate::session::SessionOptions) -> Harness {
    let mut faults = vec![Fault::new(
        "next",
        FaultKind::Fail("the step failed".into()),
    )];
    faults.extend(probe.map(|probe| Fault::new("stackTrace", probe).nth(2)));
    let script = Script {
        faults,
        ..Script::default()
    };
    let mut harness = start_stopped_with(script, options);
    let fake = harness.fake(0);
    assert!(harness.session.resume(Resume::Next, None));
    let probe = wait_requests(&fake, "stackTrace", 2)[1].seq;
    fake.wait_until("the probe's answer", |_, sent| {
        sent.iter().any(
            |message| matches!(message, Message::Response(response) if response.request_seq == probe),
        )
    });
    round_trip(&harness);
    harness.drain();
    harness
}

#[test]
fn a_stack_proves_no_stop_from_an_adapter_that_answers_for_running_threads() {
    // By default the session doesn't trust a stack: debugpy gives one for a
    // running thread. The fake answers the probe with a good stack.
    let mut harness = probed(None, options_with_breakpoint());
    let fake = harness.fake(0);
    let probe = &fake.sent();
    assert!(probe.iter().any(|message| matches!(message,
        Message::Response(response) if response.command == "stackTrace" && response.success
            && response.request_seq == requests(&fake, "stackTrace")[1].seq)));
    assert_eq!(harness.session.state(), SessionState::Uncertain);
    assert_eq!(harness.session.stop_data(), None);
    assert_eq!(requests(&fake, "scopes").len(), 1);
    assert_uncertain_controls(harness.session.controls());
    // Only a reported stop ends it.
    fake.send(stopped_event("pause"));
    harness.wait_state_times("the stop", 2, |state| *state == SessionState::Stopped);
    harness.wait_fetched(2);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_probe_stack_with_no_frames_or_a_malformed_one_proves_nothing() {
    for body in [
        serde_json::json!({ "stackFrames": [] }),
        serde_json::json!({ "stackFrames": [], "totalFrames": 0 }),
        serde_json::json!({ "stackFrames": "not a list" }),
        serde_json::json!({ "stackFrames": [{ "id": "one" }] }),
    ] {
        let harness = probed(Some(FaultKind::Answer(body.clone())), trusting_stacks());
        let fake = harness.fake(0);
        assert_eq!(harness.session.state(), SessionState::Uncertain, "{body}");
        assert_eq!(harness.session.stop_data(), None, "{body}");
        assert_eq!(requests(&fake, "scopes").len(), 1, "{body}");
    }
}
