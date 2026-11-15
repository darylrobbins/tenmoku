// SPDX-License-Identifier: Apache-2.0

//! Pause (docs/decisions/r1b9dw3g0g-debugger.md §2 "Pause"; §11 D2 C5): it
//! targets the selected thread, else the one that last stopped, else the
//! first of the startup `threads` baseline, so a program that runs at once
//! and sends no `thread` events can still be paused; with no thread known
//! it asks for `threads` first; and once the program has reported none,
//! Pause is disabled with that reason.

use kiln_l10n::tr;
use serde_json::json;

use super::session_harness::{
    Harness, options, requests, round_trip, spinning, start, wait_requests,
};
use crate::fake::{FakeAdapter, Fault, FaultKind, Script, THREAD_ID};
use crate::protocol::{Message, request};
use crate::session::{Control, DapEvent, Disabled, SessionState};

/// Waits until the baseline's reply has been handled.
fn wait_baseline(harness: &mut Harness) {
    harness.wait("the thread baseline", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::Threads(_)))
    });
}

fn paused_thread(fake: &FakeAdapter) -> i64 {
    wait_requests(fake, "pause", 1)[0]
        .arguments::<request::Pause>()
        .unwrap()
        .thread_id
}

fn sent_thread_events(fake: &FakeAdapter) -> bool {
    fake.sent()
        .iter()
        .any(|message| matches!(message, Message::Event(event) if event.event == "thread"))
}

#[test]
fn pause_on_a_launch_that_runs_at_once_targets_the_startup_baseline() {
    for script in [
        spinning(Script::default()),
        spinning(Script::default().without_configuration_done()),
    ] {
        let configuration_done = script.capabilities.supports_configuration_done_request;
        let mut harness = start(script, options());
        let fake = harness.fake(0);
        harness.wait_state(SessionState::Running);
        wait_baseline(&mut harness);
        assert_eq!(
            harness
                .session
                .threads()
                .iter()
                .map(|thread| thread.id)
                .collect::<Vec<_>>(),
            [THREAD_ID],
            "{configuration_done:?}"
        );
        assert!(!sent_thread_events(&fake));
        let controls = harness.session.controls();
        assert!(controls.pause.is_enabled(), "{configuration_done:?}");
        assert_eq!(controls.step, Control::Disabled(Disabled::Running));

        assert!(harness.session.pause(None));
        assert_eq!(paused_thread(&fake), THREAD_ID, "{configuration_done:?}");
        // The baseline was enough: no `threads` went first.
        let commands = fake.commands();
        let before_pause = commands
            .iter()
            .take_while(|command| *command != "pause")
            .filter(|command| *command == "threads")
            .count();
        assert_eq!(before_pause, 1, "{commands:?}");
        // Pause changes no state; the `stopped` that follows does.
        harness.wait_state(SessionState::Stopped);
        harness.wait_fetched(1);
        let data = harness.session.stop_data().unwrap();
        assert_eq!(
            data.stopped.as_ref().map(|body| body.reason.as_str()),
            Some("pause")
        );
        assert_eq!(
            harness.session.controls().pause,
            Control::Disabled(Disabled::AlreadyStopped)
        );
        harness.session.stop();
        harness.wait_ended();
    }
}

#[test]
fn with_no_threads_known_pause_stays_disabled_with_the_reason() {
    // The baseline answers that there are none.
    let script = spinning(Script {
        faults: vec![Fault::new(
            "threads",
            FaultKind::Answer(json!({ "threads": [] })),
        )],
        ..Script::default()
    });
    let mut harness = start(script, options());
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Running);
    wait_baseline(&mut harness);
    assert_eq!(harness.session.threads(), []);
    let pause = harness.session.controls().pause;
    assert_eq!(pause, Control::Disabled(Disabled::NoThreads));
    assert_eq!(
        pause.reason().unwrap().to_string(),
        tr!("dap-control-no-threads").to_string()
    );
    assert!(!harness.session.pause(None));
    round_trip(&harness);
    assert!(requests(&fake, "pause").is_empty());
    assert_eq!(requests(&fake, "threads").len(), 1);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn pause_with_no_thread_known_asks_for_threads_first() {
    // The baseline fails: no thread is known, but none was reported
    // either.
    let script = spinning(Script {
        faults: vec![Fault::new("threads", FaultKind::Fail("not yet".into()))],
        ..Script::default()
    });
    let mut harness = start(script, options());
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Running);
    wait_requests(&fake, "threads", 1);
    round_trip(&harness);
    assert_eq!(harness.session.threads(), []);
    assert!(harness.session.controls().pause.is_enabled());
    assert!(harness.session.pause(None));
    assert_eq!(paused_thread(&fake), THREAD_ID);
    // The failed baseline, then this Pause's own `threads`, then `pause`.
    let commands = fake.commands();
    let before_pause: Vec<&String> = commands
        .iter()
        .take_while(|command| *command != "pause")
        .filter(|command| *command == "threads")
        .collect();
    assert_eq!(before_pause.len(), 2, "{commands:?}");
    let pause = commands
        .iter()
        .position(|command| command == "pause")
        .unwrap();
    assert_eq!(commands[pause - 1], "threads", "{commands:?}");
    harness.wait_state(SessionState::Stopped);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn pause_targets_the_thread_asked_for() {
    let mut harness = start(spinning(Script::default()), options());
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Running);
    wait_baseline(&mut harness);
    assert!(harness.session.pause(Some(7)));
    assert_eq!(paused_thread(&fake), 7);
}

#[test]
fn a_failed_pause_is_shown_and_leaves_the_state() {
    let script = spinning(Script {
        faults: vec![Fault::new("pause", FaultKind::Fail("can't".into()))],
        ..Script::default()
    });
    let mut harness = start(script, options());
    harness.wait_state(SessionState::Running);
    wait_baseline(&mut harness);
    assert!(harness.session.pause(None));
    harness.wait("the failure", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::Failed(err) if err.message() == Some("can't")))
    });
    assert_eq!(harness.session.state(), SessionState::Running);
    assert!(harness.session.controls().pause.is_enabled());
}
