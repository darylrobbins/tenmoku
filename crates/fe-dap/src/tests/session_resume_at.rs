// SPDX-License-Identifier: Apache-2.0

//! `Session::resume_at`: a resume asked for at one stop, and sent only if
//! that stop is still the session's. A new stop that comes in between, as
//! the host waited on a request, refuses it, so it never resumes the newer
//! stop (debugger card D8, run to cursor).

use super::session_harness::{requests, start_stopped, stopped_event_without_thread};
use crate::fake::Script;
use crate::session::{Resume, SessionState};

#[test]
fn a_resume_at_a_stop_since_replaced_is_refused() {
    let mut harness = start_stopped(Script::default());
    let fake = harness.fake(0);
    let asked = harness.session.epoch();
    // Another stop while stopped: the epoch moves on, still Stopped.
    fake.send(stopped_event_without_thread("pause"));
    harness.wait_fetched(2);
    assert!(harness.session.epoch() > asked);
    assert_eq!(harness.session.state(), SessionState::Stopped);

    assert!(!harness.session.resume_at(Resume::Continue, None, asked));
    assert!(
        harness.session.controls().continue_.is_enabled(),
        "nothing asked"
    );
    // At the stop that's current, it resumes.
    let current = harness.session.epoch();
    assert!(harness.session.resume_at(Resume::Continue, None, current));
    harness.wait_ended();
    assert_eq!(requests(&fake, "continue").len(), 1);
}
