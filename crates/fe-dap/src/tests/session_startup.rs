// SPDX-License-Identifier: Apache-2.0

//! Startup (docs/decisions/r1b9dw3g0g-debugger.md §2 items 1-3): each of
//! item 3's orderings, scripted on the fake, reaches Stopped at a
//! breakpoint; `setExceptionBreakpoints` goes only under the
//! specification's condition; a missing `initialized` counts configuration
//! complete after 5 s; and `configurationDone` has a timeout (§13 F10).

use std::time::Duration;

use super::session_harness::{
    advance_past, options, options_with_breakpoint, program_path, requests, script, start,
    wait_requests,
};
use crate::fake::{Fault, FaultKind, Script, Startup, Trigger};
use crate::protocol::{SetExceptionBreakpointsArguments, SourceBreakpoint, request};
use crate::session::{
    CONFIGURATION_DONE_TIMEOUT, EndReason, INITIALIZED_GRACE, SessionError, SessionState,
};
use kiln_l10n::tr;

fn arguments(fake: &crate::fake::FakeAdapter) -> Vec<Vec<String>> {
    requests(fake, "setExceptionBreakpoints")
        .iter()
        .map(|request| {
            request
                .arguments::<request::SetExceptionBreakpoints>()
                .map(|args: SetExceptionBreakpointsArguments| args.filters)
                .unwrap()
        })
        .collect()
}

#[test]
fn initialized_then_configuration_then_launch_response() {
    let mut harness = start(script(), options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    let fake = harness.fake(0);
    harness.wait_fetched(1);
    // The baseline, then the stop's fetch.
    assert_eq!(
        fake.commands(),
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
    // The adapter's order: `initialized` first, `launch` answered last.
    let initialized = harness.log_index(&["event=initialized"]);
    let done = harness.log_index(&["response", "command=configurationDone"]);
    let launched = harness.log_index(&["response", "command=launch"]);
    let baseline = harness.log_index(&["request", "command=threads"]);
    assert!(
        initialized < done && done < launched,
        "{:#?}",
        harness.log_lines()
    );
    // `launch` went before configuration, not awaited.
    assert!(
        harness.log_index(&["request", "command=launch"])
            < harness.log_index(&["request", "command=setBreakpoints"])
    );
    // The baseline once both are in.
    assert!(launched < baseline);
    assert_eq!(
        harness.states(),
        [
            SessionState::Initializing,
            SessionState::Configuring,
            SessionState::Running,
            SessionState::Stopped,
        ]
    );
    let breakpoints = requests(&fake, "setBreakpoints");
    let args = breakpoints[0]
        .arguments::<request::SetBreakpoints>()
        .unwrap();
    assert_eq!(args.source.path.as_deref(), Some(program_path().as_str()));
}

#[test]
fn launch_response_then_initialized() {
    let script = Script {
        startup: Startup::LaunchResponseFirst,
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    let fake = harness.fake(0);
    harness.wait_fetched(1);
    // The baseline, then the stop's fetch.
    assert_eq!(
        fake.commands(),
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
    let launched = harness.log_index(&["response", "command=launch"]);
    let initialized = harness.log_index(&["event=initialized"]);
    let done = harness.log_index(&["response", "command=configurationDone"]);
    let baseline = harness.log_index(&["request", "command=threads"]);
    assert!(launched < initialized && initialized < done && done < baseline);
    // A launch response alone doesn't make the session Running.
    assert_eq!(
        harness.states(),
        [
            SessionState::Initializing,
            SessionState::Configuring,
            SessionState::Running,
            SessionState::Stopped,
        ]
    );
}

#[test]
fn without_configuration_done_support_configuration_ends_on_the_last_reply() {
    for startup in [Startup::InitializedFirst, Startup::LaunchResponseFirst] {
        // Two files: the second's reply is held, so it is the last
        // configuration reply.
        let script = Script {
            startup,
            faults: vec![
                Fault::new("setBreakpoints", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
            ],
            ..Script::default()
        }
        .without_configuration_done();
        let mut options = options_with_breakpoint();
        options.breakpoints.insert(
            "/fake/src/zz_other.rs".into(),
            vec![SourceBreakpoint {
                line: 1,
                ..Default::default()
            }],
        );
        let mut harness = start(script, options);
        let fake = harness.fake(0);
        // The fake starts the program on `setExceptionBreakpoints`, so it
        // stops while one configuration reply is still to come.
        harness.wait_state(SessionState::Stopped);
        wait_requests(&fake, "setExceptionBreakpoints", 1);
        // The stop's own fetch goes, but not the baseline.
        harness.wait_fetched(1);
        assert_eq!(
            requests(&fake, "threads").len(),
            1,
            "{startup:?}: the baseline went before the last configuration reply: {:?}",
            fake.commands()
        );
        fake.release();
        wait_requests(&fake, "threads", 2);
        assert_eq!(
            fake.commands(),
            [
                "initialize",
                "launch",
                "setBreakpoints",
                "setBreakpoints",
                "setExceptionBreakpoints",
                "threads",
                "stackTrace",
                "scopes",
                "variables",
                "threads"
            ],
            "{startup:?}"
        );
        // The baseline followed the held reply, the last one.
        let lines = harness.log_lines();
        let held = lines
            .iter()
            .rposition(|line| line.contains("response") && line.contains("command=setBreakpoints"));
        let baseline = lines
            .iter()
            .rposition(|line| line.contains("request") && line.contains("command=threads"))
            .unwrap();
        assert!(held.unwrap() < baseline, "{startup:?}");
        // It never went to Running over the stop.
        assert_eq!(
            harness.session.state(),
            SessionState::Stopped,
            "{startup:?}"
        );
        assert!(
            !harness.states().contains(&SessionState::Running),
            "{startup:?}: {:?}",
            harness.states()
        );
    }
}

#[test]
fn exception_breakpoints_are_sent_only_under_the_spec_condition() {
    // Filters offered (the fake's default: `raised`, and `uncaught` on by
    // default) and `configurationDone` supported: sent, with the defaults.
    let mut harness = start(script(), options());
    harness.wait_ended();
    assert_eq!(arguments(&harness.fake(0)), [vec!["uncaught".to_owned()]]);

    // The user's filters replace the defaults.
    let mut chosen = options();
    chosen.exception_filters = Some(vec!["raised".into()]);
    let mut harness = start(script(), chosen);
    harness.wait_ended();
    assert_eq!(arguments(&harness.fake(0)), [vec!["raised".to_owned()]]);

    // No filters, with `configurationDone`: not sent.
    let mut bare = Script::default();
    bare.capabilities.exception_breakpoint_filters = None;
    let mut harness = start(bare.clone(), options());
    harness.wait_ended();
    let fake = harness.fake(0);
    assert!(arguments(&fake).is_empty(), "{:?}", fake.commands());
    assert!(fake.commands().contains(&"configurationDone".to_owned()));

    // An empty list counts as none.
    let mut empty = Script::default();
    empty.capabilities.exception_breakpoint_filters = Some(Vec::new());
    let mut harness = start(empty, options());
    harness.wait_ended();
    assert!(arguments(&harness.fake(0)).is_empty());

    // No filters and no `configurationDone`: sent, empty, since it is
    // what ends configuration.
    let mut harness = start(bare.without_configuration_done(), options());
    harness.wait_ended();
    let fake = harness.fake(0);
    assert_eq!(arguments(&fake), [Vec::<String>::new()]);
    assert!(!fake.commands().contains(&"configurationDone".to_owned()));
}

#[test]
fn a_missing_initialized_counts_configuration_complete_after_5_s() {
    let script = Script {
        startup: Startup::InitializedOnRelease,
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Initializing);
    advance_past(&harness.clock, INITIALIZED_GRACE);
    // Counted complete with nothing sent: the baseline goes.
    harness.wait_state(SessionState::Running);
    wait_requests(&fake, "threads", 1);
    assert_eq!(fake.commands(), ["initialize", "launch", "threads"]);
    harness.log_index(&[&tr!(
        "dap-log-no-initialized",
        seconds = INITIALIZED_GRACE.as_secs()
    )]);
    // A late `initialized` still gets the breakpoints, and the program
    // stops at one.
    fake.release();
    harness.wait_state(SessionState::Stopped);
    // Then the stop's fetch.
    assert_eq!(
        fake.commands()[..6],
        [
            "initialize",
            "launch",
            "threads",
            "setBreakpoints",
            "setExceptionBreakpoints",
            "configurationDone"
        ]
    );
}

#[test]
fn configuration_done_timeout_ends_the_session_with_a_named_error() {
    let script = Script {
        faults: vec![Fault::new("configurationDone", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    let fake = harness.fake(0);
    wait_requests(&fake, "configurationDone", 1);
    assert!(!fake.killed());
    advance_past(&harness.clock, CONFIGURATION_DONE_TIMEOUT);
    let ended = harness.wait_ended();
    assert_eq!(
        ended.reason,
        EndReason::Failed(SessionError::ConfigurationDoneTimeout)
    );
    assert_eq!(
        SessionError::ConfigurationDoneTimeout.to_string(),
        tr!(
            "dap-session-configuration-done-timeout",
            seconds = CONFIGURATION_DONE_TIMEOUT.as_secs()
        )
        .to_string()
    );
    assert!(fake.killed(), "the adapter wasn't killed");
    fake.wait_exit();
    assert_eq!(CONFIGURATION_DONE_TIMEOUT, Duration::from_secs(10));
    // Nothing more went to an adapter that wasn't answering.
    assert_eq!(
        fake.commands().last().map(String::as_str),
        Some("configurationDone")
    );
}

fn initialized() -> crate::protocol::Message {
    crate::protocol::Message::Event(crate::protocol::EventMessage::new::<
        crate::protocol::event::Initialized,
    >(0, &crate::protocol::Empty::default()))
}

fn breakpoint_ids(harness: &super::session_harness::Harness) -> Vec<i64> {
    harness
        .events
        .iter()
        .filter_map(|event| match event {
            crate::session::DapEvent::Breakpoints { breakpoints, .. } => {
                Some(breakpoints.iter().filter_map(|b| b.id).collect::<Vec<_>>())
            }
            _ => None,
        })
        .flatten()
        .collect()
}

/// A request the fake answers, awaited: what it sent before has been
/// handled once this returns.
fn round_trip(harness: &super::session_harness::Harness) {
    let source =
        harness
            .session
            .request::<request::SourceRequest>(&crate::protocol::SourceArguments {
                source_reference: 1,
                ..Default::default()
            });
    async_io::block_on(source).expect("source is answered");
}

#[test]
fn an_earlier_rounds_breakpoint_reply_never_replaces_the_latest() {
    // The first round's `setBreakpoints` reply is held past a second
    // `initialized`, whose own reply (breakpoint id 2) comes at once.
    let script = Script {
        faults: vec![Fault::new(
            "setBreakpoints",
            FaultKind::ReplyAfter(Trigger::Manual),
        )],
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    let fake = harness.fake(0);
    wait_requests(&fake, "setExceptionBreakpoints", 1);
    fake.send(initialized());
    harness.wait_state(SessionState::Stopped);
    assert_eq!(breakpoint_ids(&harness), [2]);
    fake.release();
    round_trip(&harness);
    harness.drain();
    assert_eq!(
        breakpoint_ids(&harness),
        [2],
        "the first round's reply landed"
    );
}

#[test]
fn a_new_initialized_round_must_complete_before_the_baseline() {
    // `launch` is answered only when the test says; the second round's
    // `setBreakpoints` reply is held.
    let script = Script {
        faults: vec![
            Fault::new("launch", FaultKind::Unanswered),
            Fault::new("setBreakpoints", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
        ],
        ..Script::default()
    };
    let mut harness = start(script, options_with_breakpoint());
    let fake = harness.fake(0);
    // The first round completes while `launch` is still unanswered.
    wait_requests(&fake, "configurationDone", 1);
    fake.wait_until("configurationDone's answer", |_, sent| {
        sent.iter().any(|message| matches!(
            message,
            crate::protocol::Message::Response(response) if response.command == "configurationDone"
        ))
    });
    // A second round begins; then `launch` succeeds during it.
    fake.send(initialized());
    wait_requests(&fake, "setExceptionBreakpoints", 2);
    let launch = &requests(&fake, "launch")[0];
    fake.send(crate::protocol::Message::Response(
        launch.respond::<request::Launch>(0, &crate::protocol::Empty::default()),
    ));
    round_trip(&harness);
    assert!(
        !fake.commands().contains(&"threads".to_owned()),
        "the baseline went before the latest round completed: {:?}",
        fake.commands()
    );
    assert_eq!(harness.session.state(), SessionState::Configuring);
    // The second round's last reply completes it.
    fake.release();
    wait_requests(&fake, "threads", 1);
    harness.wait_state(SessionState::Running);
    assert_eq!(requests(&fake, "configurationDone").len(), 2);
}
