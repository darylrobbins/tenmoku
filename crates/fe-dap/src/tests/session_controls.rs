// SPDX-License-Identifier: Apache-2.0

//! The control-state query (card D2b), state by state: what each state
//! enables, a resume asked for and not yet taken reading as Resuming only
//! over Stopped and Uncertain, and nothing enabled once the user has
//! stopped the session (§13 F11).

use crate::session::{Control, Controls, Disabled, EndReason, Ended, SessionState, View};

fn view(state: SessionState, resume_requested: bool) -> View {
    View {
        state,
        resume_requested,
        ..View::default()
    }
}

fn ended() -> SessionState {
    SessionState::Ended(Ended {
        exit_code: None,
        reason: EndReason::Finished,
    })
}

#[test]
fn a_queued_resume_never_outlasts_an_end_a_shutdown_or_a_restart() {
    for requested in [false, true] {
        assert_eq!(
            Controls::of(&view(ended(), requested), false),
            Controls::none(Disabled::Ended)
        );
        assert_eq!(
            Controls::of(&view(SessionState::Terminating, requested), false),
            Controls::none(Disabled::Ending)
        );
        assert_eq!(
            Controls::of(&view(SessionState::Restarting, requested), false),
            Controls {
                stop: Control::Enabled,
                ..Controls::none(Disabled::Restarting)
            }
        );
        assert_eq!(
            Controls::of(&view(SessionState::Launching, requested), false),
            Controls {
                stop: Control::Enabled,
                ..Controls::none(Disabled::Starting)
            }
        );
    }
}

#[test]
fn a_queued_resume_reads_as_resuming_over_stopped_and_uncertain() {
    let resuming = Controls::of(&view(SessionState::Resuming, false), false);
    assert_eq!(resuming.step, Control::Disabled(Disabled::Resuming));
    assert_eq!(resuming.continue_, Control::Disabled(Disabled::Resuming));
    assert!(resuming.stop.is_enabled());
    for state in [SessionState::Stopped, SessionState::Uncertain] {
        assert_eq!(Controls::of(&view(state, true), false), resuming);
    }
    let stopped = Controls::of(&view(SessionState::Stopped, false), false);
    assert!(stopped.step.is_enabled() && stopped.continue_.is_enabled());
    assert_eq!(stopped.pause, Control::Disabled(Disabled::AlreadyStopped));
}

#[test]
fn nothing_is_enabled_once_the_user_has_stopped() {
    for state in [
        SessionState::Running,
        SessionState::Stopped,
        SessionState::Uncertain,
        SessionState::Resuming,
        SessionState::Terminating,
    ] {
        assert_eq!(
            Controls::of(&view(state, false), true),
            Controls::none(Disabled::Ending)
        );
    }
    assert_eq!(
        Controls::of(&view(ended(), false), true),
        Controls::none(Disabled::Ended)
    );
}
