// SPDX-License-Identifier: Apache-2.0

//! Threads across stops (docs/decisions/r1b9dw3g0g-debugger.md §2 items
//! 3-5): which threads are suspended, by `allThreadsStopped` and
//! `allThreadsContinued`, with the ones a single-thread `continue` or
//! `continued` leaves stopped fetched again under the new epoch; a
//! `threads` reply older than the list it would replace dropped; and a
//! stopped thread a newer list leaves out never targeted.

use serde_json::{Value, json};

use super::session_harness::{
    Harness, options, requests, round_trip, spinning, start, start_stopped, states_from,
    stopped_event_without_thread, wait_requests,
};
use crate::fake::{Fault, FaultKind, Script, THREAD_ID, Trigger};
use crate::protocol::{ContinuedBody, EventMessage, Message, ResponseMessage, event, request};
use crate::session::{RequestError, Resume, SessionState, StopData};

const WORKER: i64 = 2;

fn stop_data(harness: &Harness) -> StopData {
    harness.session.stop_data().expect("the session is stopped")
}

/// The thread ids the session knows.
fn thread_ids(harness: &Harness) -> Vec<i64> {
    harness
        .session
        .threads()
        .iter()
        .map(|thread| thread.id)
        .collect()
}

/// `threads` answers listing `ids`.
fn listing(ids: &[i64]) -> FaultKind {
    let threads: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "id": id, "name": format!("thread {id}") }))
        .collect();
    FaultKind::Answer(json!({ "threads": threads }))
}

/// Stopped at the breakpoint with every thread suspended, the stop's
/// fetch (the second `threads`) listing the main thread and a worker;
/// `faults` besides.
fn two_threads_stopped(mut faults: Vec<Fault>) -> Harness {
    faults.push(Fault::new("threads", listing(&[THREAD_ID, WORKER])).nth(2));
    let harness = start_stopped(Script {
        faults,
        ..Script::default()
    });
    assert_eq!(thread_ids(&harness), [THREAD_ID, WORKER]);
    harness
}

/// How many `Threads` events the session has sent.
fn threads_events(harness: &Harness) -> usize {
    harness
        .events
        .iter()
        .filter(|event| matches!(event, crate::session::DapEvent::Threads(_)))
        .count()
}

/// A program that runs at once, with `threads` answered as `lists` says,
/// occurrence by occurrence.
fn running_with_lists(lists: &[&[i64]], mut faults: Vec<Fault>) -> Harness {
    faults.extend(
        lists
            .iter()
            .enumerate()
            .map(|(at, ids)| Fault::new("threads", listing(ids)).nth(at + 1)),
    );
    let mut harness = start(
        spinning(Script {
            faults,
            ..Script::default()
        }),
        options(),
    );
    harness.wait_state(SessionState::Running);
    harness
}

/// A `stopped` event for `thread_id` alone.
fn stopped_alone(thread_id: i64) -> Message {
    Message::Event(EventMessage::new::<event::Stopped>(
        0,
        &crate::protocol::StoppedBody {
            reason: "breakpoint".into(),
            thread_id: Some(thread_id),
            all_threads_stopped: Some(false),
            ..Default::default()
        },
    ))
}

/// A `thread` event: `thread_id` exited.
fn thread_exited(thread_id: i64) -> Message {
    Message::Event(EventMessage::new::<event::ThreadEvent>(
        0,
        &crate::protocol::ThreadBody {
            reason: "exited".into(),
            thread_id,
            ..Default::default()
        },
    ))
}

/// The thread of each `stackTrace` the fake got.
fn stack_threads(fake: &crate::fake::FakeAdapter) -> Vec<i64> {
    requests(fake, "stackTrace")
        .iter()
        .map(stack_thread)
        .collect()
}

/// Running, then the main thread and the worker each stopped alone, every
/// `threads` listing both (the baseline and two fetches), and `lists` for
/// the `threads` after; the worker's stop is shown.
fn both_stopped_alone(lists: &[&[i64]], faults: Vec<Fault>) -> Harness {
    let mut all: Vec<&[i64]> = vec![&[THREAD_ID, WORKER]; 3];
    all.extend_from_slice(lists);
    let mut harness = running_with_lists(&all, faults);
    let fake = harness.fake(0);
    wait_requests(&fake, "threads", 1);
    fake.send(stopped_alone(THREAD_ID));
    wait_requests(&fake, "stackTrace", 1);
    fake.send(stopped_alone(WORKER));
    wait_requests(&fake, "stackTrace", 2);
    round_trip(&harness);
    harness.drain();
    assert_eq!(stack_threads(&fake), [THREAD_ID, WORKER]);
    assert_eq!(harness.session.state(), SessionState::Stopped);
    harness
}

fn stack_thread(request: &crate::protocol::RequestMessage) -> i64 {
    request
        .arguments::<request::StackTrace>()
        .unwrap()
        .thread_id
}

#[test]
fn a_continued_event_for_one_thread_keeps_the_others_stopped_under_a_new_epoch() {
    // The refetch's `threads` (the third) still lists both.
    let mut harness = two_threads_stopped(vec![
        Fault::new("threads", listing(&[THREAD_ID, WORKER])).nth(3),
    ]);
    let fake = harness.fake(0);
    let first = stop_data(&harness);
    fake.send(Message::Event(EventMessage::new::<event::Continued>(
        0,
        &ContinuedBody {
            thread_id: WORKER,
            all_threads_continued: Some(false),
            ..Default::default()
        },
    )));
    harness.wait_fetched(2);
    let retained = stop_data(&harness);
    assert!(retained.epoch > first.epoch);
    assert_eq!(retained.thread_id, Some(THREAD_ID));
    assert_eq!(retained.stopped, None);
    assert!(!retained.frames.is_empty());
    // Stopped throughout.
    assert_eq!(harness.session.state(), SessionState::Stopped);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [SessionState::Stopped]
    );
    // The worker runs: its stack isn't asked for.
    let worker = async_io::block_on(harness.session.frames(retained.epoch, WORKER, 0));
    assert_eq!(
        worker,
        Err(RequestError::NotStopped {
            command: "stackTrace".into()
        })
    );
    assert_eq!(stack_thread(&requests(&fake, "stackTrace")[1]), THREAD_ID);
    // A `continued` for all of them is Running.
    fake.send(Message::Event(EventMessage::new::<event::Continued>(
        0,
        &ContinuedBody {
            thread_id: THREAD_ID,
            ..Default::default()
        },
    )));
    harness.wait_state_times("running", 2, |state| *state == SessionState::Running);
    assert_eq!(harness.session.stop_data(), None);
}

#[test]
fn a_single_thread_continue_leaves_the_other_threads_stopped() {
    // The worker's `continue` is answered `allThreadsContinued: false`;
    // the refetch's `threads` still lists both.
    let mut harness = two_threads_stopped(vec![
        Fault::new(
            "continue",
            FaultKind::Answer(json!({ "allThreadsContinued": false })),
        ),
        Fault::new("threads", listing(&[THREAD_ID, WORKER])).nth(3),
    ]);
    let fake = harness.fake(0);
    let first = stop_data(&harness);
    assert!(harness.session.resume(Resume::Continue, Some(WORKER)));
    harness.wait_state_times("the retained stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Stopped
        ]
    );
    let continued = &requests(&fake, "continue")[0];
    assert_eq!(
        continued
            .arguments::<request::Continue>()
            .unwrap()
            .thread_id,
        WORKER
    );
    let retained = stop_data(&harness);
    assert!(retained.epoch > first.epoch);
    assert_eq!(retained.thread_id, Some(THREAD_ID));
    assert_eq!(stack_thread(&requests(&fake, "stackTrace")[1]), THREAD_ID);
    // The main thread steps from here.
    assert!(harness.session.controls().step.is_enabled());
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_stop_naming_no_thread_follows_the_threads_listed_not_the_last_stopped() {
    // The main thread has gone, with no `thread` event: the stop's
    // `threads` (the third) lists only the worker.
    let mut harness = start_stopped(Script {
        faults: vec![Fault::new("threads", listing(&[WORKER])).nth(3)],
        ..Script::default()
    });
    let fake = harness.fake(0);
    fake.send(stopped_event_without_thread("pause"));
    harness.wait_fetched(2);
    assert_eq!(stack_thread(&requests(&fake, "stackTrace")[1]), WORKER);
    assert_eq!(stop_data(&harness).thread_id, Some(WORKER));
    assert!(harness.session.resume(Resume::Next, None));
    let next = &wait_requests(&fake, "next", 1)[0];
    assert_eq!(next.arguments::<request::Next>().unwrap().thread_id, WORKER);
}

#[test]
fn a_held_baseline_reply_never_replaces_a_newer_stops_threads() {
    // The startup baseline goes unanswered; the stop's fetch lists the
    // thread; then the baseline's reply comes, listing none.
    let mut harness = start_stopped(Script {
        faults: vec![Fault::new("threads", FaultKind::Unanswered)],
        ..Script::default()
    });
    let fake = harness.fake(0);
    assert_eq!(thread_ids(&harness), [THREAD_ID]);
    let baseline = &requests(&fake, "threads")[0];
    fake.send(Message::Response(ResponseMessage {
        request_seq: baseline.seq,
        success: true,
        command: "threads".into(),
        body: Some(json!({ "threads": [] })),
        ..Default::default()
    }));
    round_trip(&harness);
    harness.drain();
    // The obsolete empty list was dropped, so it can't turn Pause off.
    assert_eq!(thread_ids(&harness), [THREAD_ID]);
    assert!(!harness.events.iter().any(
        |event| matches!(event, crate::session::DapEvent::Threads(threads) if threads.is_empty())
    ));
}

#[test]
fn a_pauses_threads_reply_after_a_stop_sends_no_pause() {
    // No thread known (the baseline fails), so Pause asks for `threads`
    // first; that reply is held, and a stop comes before it.
    // The stop's own `threads` (the third) is held too, behind it: the
    // obsolete reply comes after the stop, before the stop's list.
    let script = spinning(Script {
        faults: vec![
            Fault::new("threads", FaultKind::Fail("not yet".into())),
            Fault::new("threads", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
            Fault::new("threads", FaultKind::ReplyAfter(Trigger::Manual)).nth(3),
        ],
        ..Script::default()
    });
    let mut harness = start(script, options());
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Running);
    wait_requests(&fake, "threads", 1);
    round_trip(&harness);
    assert!(harness.session.pause(None));
    let held = wait_requests(&fake, "threads", 2)[1].seq;
    fake.send(super::session_harness::stopped_event("breakpoint"));
    harness.wait_state(SessionState::Stopped);
    let fresh = wait_requests(&fake, "threads", 3)[2].seq;
    harness.drain();
    let before = threads_events(&harness);
    fake.release();
    fake.wait_until("the held replies", |_, sent| {
        [held, fresh].iter().all(|seq| {
            sent.iter().any(
                |message| matches!(message, Message::Response(response) if response.request_seq == *seq),
            )
        })
    });
    round_trip(&harness);
    harness.drain();
    // The obsolete reply changed nothing and paused nothing; the stop's own
    // list was the one applied.
    assert!(requests(&fake, "pause").is_empty(), "{:?}", fake.commands());
    assert_eq!(threads_events(&harness), before + 1);
    assert_eq!(thread_ids(&harness), [THREAD_ID]);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn an_obsolete_baseline_reply_after_a_stop_never_changes_its_threads() {
    // No `thread` events. The baseline goes unanswered and the stop's own
    // `threads` (the second) is held: the baseline's reply, listing none,
    // comes after the stop and before the stop's list.
    let script = Script {
        thread_events: false,
        faults: vec![
            Fault::new("threads", FaultKind::Unanswered),
            Fault::new("threads", FaultKind::ReplyAfter(Trigger::Manual)).nth(2),
        ],
        ..Script::default()
    };
    let mut harness = start(script, super::session_harness::options_with_breakpoint());
    let fake = harness.fake(0);
    harness.wait_state(SessionState::Stopped);
    wait_requests(&fake, "threads", 2);
    let baseline = &requests(&fake, "threads")[0];
    fake.send(Message::Response(ResponseMessage {
        request_seq: baseline.seq,
        success: true,
        command: "threads".into(),
        body: Some(json!({ "threads": [] })),
        ..Default::default()
    }));
    round_trip(&harness);
    harness.drain();
    assert_eq!(threads_events(&harness), 0);
    assert_eq!(thread_ids(&harness), Vec::<i64>::new());
    // The stop's list comes, and is the one used.
    fake.release();
    harness.wait_fetched(1);
    assert_eq!(thread_ids(&harness), [THREAD_ID]);
    assert_eq!(stop_data(&harness).thread_id, Some(THREAD_ID));
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_partial_continued_before_the_continues_answer_keeps_the_other_threads_stopped() {
    // The worker's `continue` goes unanswered for now; the refetch's
    // `threads` (the third) lists both.
    let mut harness = two_threads_stopped(vec![
        Fault::new("continue", FaultKind::Unanswered),
        Fault::new("threads", listing(&[THREAD_ID, WORKER])).nth(3),
    ]);
    let fake = harness.fake(0);
    let first = stop_data(&harness);
    assert!(harness.session.resume(Resume::Continue, Some(WORKER)));
    let continued = wait_requests(&fake, "continue", 1)[0].clone();
    harness.wait_state(SessionState::Resuming);
    // The adapter says only the worker went, before its answer.
    fake.send(Message::Event(EventMessage::new::<event::Continued>(
        0,
        &ContinuedBody {
            thread_id: WORKER,
            all_threads_continued: Some(false),
            ..Default::default()
        },
    )));
    harness.wait_state_times("the retained stop", 2, |state| {
        *state == SessionState::Stopped
    });
    harness.wait_fetched(2);
    let retained = stop_data(&harness);
    assert!(retained.epoch > first.epoch);
    assert_eq!(retained.thread_id, Some(THREAD_ID));
    // Its answer comes late, and changes nothing.
    fake.send(Message::Response(continued.respond::<request::Continue>(
        0,
        &crate::protocol::ContinueBody {
            all_threads_continued: Some(false),
            ..Default::default()
        },
    )));
    round_trip(&harness);
    harness.drain();
    assert_eq!(harness.session.state(), SessionState::Stopped);
    assert_eq!(harness.session.epoch(), retained.epoch);
    assert_eq!(
        states_from(&harness, 1, |state| *state == SessionState::Stopped),
        [
            SessionState::Stopped,
            SessionState::Resuming,
            SessionState::Stopped
        ]
    );
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_thread_that_exits_while_a_continue_is_pending_isnt_left_stopped() {
    // Both stopped alone; the main thread's `continue` is held; the worker
    // exits; then the `continue` is answered for its thread alone.
    let mut harness = both_stopped_alone(&[], vec![Fault::new("continue", FaultKind::Unanswered)]);
    let fake = harness.fake(0);
    assert!(harness.session.resume(Resume::Continue, Some(THREAD_ID)));
    let continued = wait_requests(&fake, "continue", 1)[0].clone();
    fake.send(thread_exited(WORKER));
    fake.send(Message::Response(continued.respond::<request::Continue>(
        0,
        &crate::protocol::ContinueBody {
            all_threads_continued: Some(false),
            ..Default::default()
        },
    )));
    round_trip(&harness);
    harness.drain();
    // Nothing is left suspended: Running, with no stop.
    assert_eq!(harness.session.state(), SessionState::Running);
    assert_eq!(harness.session.stop_data(), None);
    assert_eq!(stack_threads(&fake), [THREAD_ID, WORKER]);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_retained_stop_on_a_thread_that_silently_went_ends() {
    // Both stopped alone; the worker's `continued` leaves the main thread,
    // but the refetch's `threads` (the fourth) lists only the worker.
    let mut harness = both_stopped_alone(&[&[WORKER]], Vec::new());
    let fake = harness.fake(0);
    fake.send(Message::Event(EventMessage::new::<event::Continued>(
        0,
        &ContinuedBody {
            thread_id: WORKER,
            all_threads_continued: Some(false),
            ..Default::default()
        },
    )));
    wait_requests(&fake, "threads", 4);
    harness.wait_state_times("running", 2, |state| *state == SessionState::Running);
    round_trip(&harness);
    assert_eq!(harness.session.stop_data(), None);
    // No stack was asked for of the gone thread.
    assert_eq!(stack_threads(&fake), [THREAD_ID, WORKER]);
    harness.session.stop();
    harness.wait_ended();
}

#[test]
fn a_shown_thread_that_exits_moves_the_stop_to_another_suspended_one() {
    // Both stopped alone, the worker's stop shown; it exits, and the
    // refetch's `threads` (the fourth) lists the main thread.
    let mut harness = both_stopped_alone(&[&[THREAD_ID]], Vec::new());
    let fake = harness.fake(0);
    let shown = harness.session.stop_data().unwrap();
    assert_eq!(shown.thread_id, Some(WORKER));
    fake.send(thread_exited(WORKER));
    wait_requests(&fake, "stackTrace", 3);
    round_trip(&harness);
    let moved = stop_data(&harness);
    assert!(moved.epoch > shown.epoch);
    assert_eq!(moved.thread_id, Some(THREAD_ID));
    assert_eq!(stack_threads(&fake), [THREAD_ID, WORKER, THREAD_ID]);
    assert_eq!(harness.session.state(), SessionState::Stopped);
    harness.session.stop();
    harness.wait_ended();
}
