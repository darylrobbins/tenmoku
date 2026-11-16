//! Pending verification (debugger D6, T1849 F12): a breakpoint enabled
//! again waits for the answer to its own replacement, not for any event of
//! the file's breakpoints.

use fe_dap::{
    fake::{FakeAdapter, Fault, FaultKind, Script, Trigger},
    protocol::Message,
};
use fe_gpui::BreakpointState;
use gpui::TestAppContext;

use super::frames_after_save::{Debug, debug_with, relocate, spinning};

/// A session with rows 1 and 3 verified, whose next sends (the 2nd to the
/// 5th) the fake answers as `fault` says: the adapter's ids for them.
fn verified_then(cx: &mut TestAppContext, fault: FaultKind) -> (Debug<'_>, FakeAdapter, [i64; 2]) {
    let mut debug = debug_with(cx, move |script| Script {
        faults: (2..=5)
            .map(|nth| Fault::new("setBreakpoints", fault.clone()).nth(nth))
            .collect(),
        ..spinning(script)
    });
    debug.toggle(1);
    debug.toggle(3);
    debug.start();
    let fake = debug.fake();
    let verified = [(1, BreakpointState::Enabled), (3, BreakpointState::Enabled)];
    debug.until("the verification", |debug| debug.breakpoints() == verified);
    let ids = [debug.adapter_id(2), debug.adapter_id(4)];
    (debug, fake, ids)
}

impl Debug<'_> {
    /// The adapter's id for the breakpoint sent on `line`.
    fn adapter_id(&mut self, line: u32) -> i64 {
        let (debugger, main) = (self.debugger(), self.main.clone());
        debugger.read_with(self.cx, |debugger, _| {
            (debugger.verification(&main).iter())
                .find(|said| said.line == line)
                .and_then(|said| said.id)
                .expect("the adapter's id")
        })
    }
}

/// Row 1 disabled and enabled again, its replacement held or refused.
fn re_enabled(debug: &mut Debug<'_>) {
    let (store, path) = (debug.store(), debug.main.clone());
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, false, cx));
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, true, cx));
    debug.draw();
}

/// What the gutter shows while row 1 waits: a ring, and row 3's own dot.
fn waiting() -> Vec<(u32, BreakpointState)> {
    vec![
        (1, BreakpointState::Unverified { message: None }),
        (3, BreakpointState::Enabled),
    ]
}

/// Events for the old id of row 1 and for the unrelated row 3's.
fn events(debug: &mut Debug<'_>, fake: &FakeAdapter, ids: [i64; 2]) {
    relocate(fake, ids[0], 2);
    relocate(fake, ids[1], 4);
    debug.draw();
    debug.draw();
}

/// F12: a delayed replacement keeps the breakpoint a ring through events
/// naming its old id and an unrelated breakpoint's, until its own reply.
#[gpui::test]
fn a_delayed_replacement_stays_pending_through_other_events(cx: &mut TestAppContext) {
    let (mut debug, fake, ids) = verified_then(cx, FaultKind::ReplyAfter(Trigger::Manual));
    re_enabled(&mut debug);
    assert_eq!(debug.breakpoints(), waiting());
    events(&mut debug, &fake, ids);
    assert_eq!(debug.breakpoints(), waiting());
    fake.release();
    debug.until("the answer", |debug| {
        debug.breakpoints() == [(1, BreakpointState::Enabled), (3, BreakpointState::Enabled)]
    });
}

/// F12: a refused replacement stays a ring through the same events.
#[gpui::test]
fn a_failed_replacement_stays_pending_through_other_events(cx: &mut TestAppContext) {
    let (mut debug, fake, ids) = verified_then(cx, FaultKind::Fail("refused".into()));
    re_enabled(&mut debug);
    debug.until("both refused", |_| {
        (fake.sent().iter())
            .filter(|m| matches!(m, Message::Response(r) if r.command == "setBreakpoints" && !r.success))
            .count()
            == 2
    });
    events(&mut debug, &fake, ids);
    assert_eq!(debug.breakpoints(), waiting());
}
