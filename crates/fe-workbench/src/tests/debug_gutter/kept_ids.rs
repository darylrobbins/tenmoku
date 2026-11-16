//! Pending verification waits on the awaited send's answer (T667, from
//! T1881 F1 and F5): not on breakpoint ids, which an adapter may keep
//! across a replacement or never give.

use fe_dap::{
    fake::{FakeAdapter, Fault, FaultKind, Script, Startup, Trigger},
    protocol::Message,
};
use fe_gpui::BreakpointState;
use gpui::TestAppContext;
use serde_json::{Value, json};

use super::frames_after_save::{Debug, debug_with, relocate, spinning};

/// A session with rows 1 and 3 (lines 2 and 4) verified, whose next sends
/// (the 2nd to the 5th) the fake answers as `fault` says. `first` is the
/// first send's answer, if not the fake's own; `keep` has the fake hand
/// ids back.
fn verified_then(
    cx: &mut TestAppContext,
    fault: FaultKind,
    keep: bool,
    first: Option<Value>,
) -> (Debug<'_>, FakeAdapter) {
    let mut debug = debug_with(cx, move |script| Script {
        keep_breakpoint_ids: keep,
        faults: first
            .map(|body| Fault::new("setBreakpoints", FaultKind::Answer(body)).nth(1))
            .into_iter()
            .chain((2..=5).map(|nth| Fault::new("setBreakpoints", fault.clone()).nth(nth)))
            .collect(),
        ..spinning(script)
    });
    debug.toggle(1);
    debug.toggle(3);
    debug.start();
    let fake = debug.fake();
    let verified = [(1, BreakpointState::Enabled), (3, BreakpointState::Enabled)];
    debug.until("the verification", |debug| debug.breakpoints() == verified);
    (debug, fake)
}

/// Row 1 disabled and enabled again, its replacement held or refused.
fn re_enabled(debug: &mut Debug<'_>) {
    let (store, path) = (debug.store(), debug.main.clone());
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, false, cx));
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, true, cx));
    debug.draw();
}

/// What the gutter shows while row 1 waits.
fn waiting() -> Vec<(u32, BreakpointState)> {
    vec![
        (1, BreakpointState::Unverified { message: None }),
        (3, BreakpointState::Enabled),
    ]
}

fn both_verified() -> Vec<(u32, BreakpointState)> {
    vec![(1, BreakpointState::Enabled), (3, BreakpointState::Enabled)]
}

/// The answer to the first send: row 1's breakpoint has no id, row 3's has.
fn no_id_for_row_1() -> Value {
    json!({"breakpoints": [
        {"verified": true, "line": 2},
        {"verified": true, "line": 4, "id": 77},
    ]})
}

/// F1: the adapter keeps ids across the replacement. An earlier send,
/// holding the reused id, answers on its own and an event names the id:
/// the row stays pending. The replacement's answer, naming the same id,
/// clears it.
#[gpui::test]
fn kept_ids_stay_pending_until_the_replacements_answer(cx: &mut TestAppContext) {
    let (mut debug, fake) = verified_then(cx, FaultKind::ReplyAfter(Trigger::Manual), true, None);
    let id = debug.debugger().read_with(debug.cx, |debugger, _| {
        let main = debug.main.clone();
        debugger.verification(&main)[0].id.expect("an id")
    });
    let (store, path) = (debug.store(), debug.main.clone());
    // An earlier send still holding row 1 (line 2) and its id, then the
    // replacement, which the row waits on.
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 3, false, cx));
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, false, cx));
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, true, cx));
    debug.draw();
    assert_eq!(
        row_1(&mut debug),
        BreakpointState::Unverified { message: None }
    );
    // Both replacements reach the adapter and are held.
    let sets = || {
        (fake.received().iter())
            .filter(|m| matches!(m, Message::Request(r) if r.command == "setBreakpoints"))
            .count()
    };
    debug.until("the sends are held", |_| sets() == 4);
    // The earlier send is answered on its own, then an event names the id.
    // The event follows its reply on the one channel, so once it has been
    // heard the reply has been too.
    fake.release_oldest(1);
    relocate(&fake, id, 3);
    debug.until("the event", |debug| {
        let (debugger, main) = (debug.debugger(), debug.main.clone());
        debugger.read_with(debug.cx, |debugger, _| {
            (debugger.verification(&main).iter())
                .any(|said| said.id == Some(id) && said.adapter_line == Some(3))
        })
    });
    assert_eq!(
        row_1(&mut debug),
        BreakpointState::Unverified { message: None }
    );
    fake.release();
    debug.until("the answer", |debug| {
        row_1(debug) == BreakpointState::Enabled
    });
}

/// What the gutter shows on row 1.
fn row_1(debug: &mut Debug<'_>) -> BreakpointState {
    (debug.breakpoints().into_iter())
        .find(|(row, _)| *row == 1)
        .map(|(_, state)| state)
        .expect("row 1 has a mark")
}

/// F5: a pending line whose earlier verification had no id stays pending
/// through an unrelated breakpoint's event while its replacement is delayed.
#[gpui::test]
fn an_idless_line_stays_pending_through_an_event_while_delayed(cx: &mut TestAppContext) {
    let (mut debug, fake) = verified_then(
        cx,
        FaultKind::ReplyAfter(Trigger::Manual),
        false,
        Some(no_id_for_row_1()),
    );
    re_enabled(&mut debug);
    assert_eq!(debug.breakpoints(), waiting());
    relocate(&fake, 77, 4);
    debug.draw();
    debug.draw();
    assert_eq!(debug.breakpoints(), waiting());
    fake.release();
    debug.until("the answer", |debug| debug.breakpoints() == both_verified());
}

/// F5: and through the same event after a refused replacement.
#[gpui::test]
fn an_idless_line_stays_pending_through_an_event_when_refused(cx: &mut TestAppContext) {
    let (mut debug, fake) = verified_then(
        cx,
        FaultKind::Fail("refused".into()),
        false,
        Some(no_id_for_row_1()),
    );
    re_enabled(&mut debug);
    debug.until("both refused", |_| {
        (fake.sent().iter())
            .filter(|m| matches!(m, Message::Response(r) if r.command == "setBreakpoints" && !r.success))
            .count()
            == 2
    });
    relocate(&fake, 77, 4);
    debug.draw();
    debug.draw();
    assert_eq!(debug.breakpoints(), waiting());
}

/// F1 (review): a change made while the session is still launching waits
/// for the replacement sent once it is configured, not the session's own
/// first answer.
#[gpui::test]
fn a_change_during_launch_stays_pending_through_the_initial_answer(cx: &mut TestAppContext) {
    let mut debug = debug_with(cx, |script| Script {
        startup: Startup::InitializedOnRelease,
        faults: vec![Fault::new("setBreakpoints", FaultKind::ReplyAfter(Trigger::Manual)).nth(2)],
        ..spinning(script)
    });
    debug.toggle(1);
    debug.toggle(3);
    debug.start();
    let fake = debug.fake();
    // Still launching: row 1 disabled and enabled again.
    debug.toggle(1);
    debug.toggle(1);
    fake.release();
    let sets = || {
        (fake.received().iter())
            .filter(|m| matches!(m, Message::Request(r) if r.command == "setBreakpoints"))
            .count()
    };
    debug.until(
        "the first send answered and the replacement held",
        |debug| {
            let (debugger, main) = (debug.debugger(), debug.main.clone());
            sets() == 2
                && debugger.read_with(debug.cx, |debugger, _| {
                    !debugger.verification(&main).is_empty()
                })
        },
    );
    debug.draw();
    assert_eq!(
        row_1(&mut debug),
        BreakpointState::Unverified { message: None }
    );
    fake.release();
    debug.until("the answer", |debug| {
        row_1(debug) == BreakpointState::Enabled
    });
}
