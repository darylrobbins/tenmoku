// SPDX-License-Identifier: Apache-2.0

//! The control-state query (card D2b): which of Continue, Pause, the steps,
//! Restart and Stop are enabled, and why the others aren't, from the
//! session's state alone, so the host's toolbar and commands hold no logic
//! of their own.

use std::fmt;

use kiln_l10n::tr;

use super::{SessionState, View};

/// One control: enabled, or disabled for a reason the host shows (its
/// tooltip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Enabled,
    Disabled(Disabled),
}

impl Control {
    pub fn is_enabled(self) -> bool {
        self == Control::Enabled
    }

    /// Why it is disabled, if it is.
    pub fn reason(self) -> Option<Disabled> {
        match self {
            Control::Enabled => None,
            Control::Disabled(why) => Some(why),
        }
    }
}

/// Why a control is disabled. Its `Display` is the tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disabled {
    /// The adapter is still starting the program.
    Starting,
    /// The program is running (Continue, the steps).
    Running,
    /// The program is already stopped (Pause).
    AlreadyStopped,
    /// A resume is waiting for the adapter's answer.
    Resuming,
    /// The program may be running or stopped: step only once a stop is
    /// proven.
    Uncertain,
    /// The program reported no threads, so there is nothing to pause.
    NoThreads,
    /// A restart is in flight.
    Restarting,
    /// The session is ending: the user stopped it, or it is shutting down.
    Ending,
    Ended,
}

impl fmt::Display for Disabled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Disabled::Starting => tr!("dap-control-starting"),
            Disabled::Running => tr!("dap-control-running"),
            Disabled::AlreadyStopped => tr!("dap-control-already-stopped"),
            Disabled::Resuming => tr!("dap-control-resuming"),
            Disabled::Uncertain => tr!("dap-control-uncertain"),
            Disabled::NoThreads => tr!("dap-control-no-threads"),
            Disabled::Restarting => tr!("dap-control-restarting"),
            Disabled::Ending => tr!("dap-control-ending"),
            Disabled::Ended => tr!("dap-control-ended"),
        };
        f.write_str(&text)
    }
}

/// The session's controls. Stepping (`next`, `stepIn`, `stepOut`) is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Controls {
    pub continue_: Control,
    pub pause: Control,
    pub step: Control,
    pub restart: Control,
    pub stop: Control,
}

impl Controls {
    /// Every control disabled, for `why`.
    pub fn none(why: Disabled) -> Self {
        let off = Control::Disabled(why);
        Self {
            continue_: off,
            pause: off,
            step: off,
            restart: off,
            stop: off,
        }
    }

    /// The controls for `view`; `stopping` once the user has stopped the
    /// session, after which none is ever enabled again (§13 F11), whatever
    /// a late event does.
    pub(crate) fn of(view: &View, stopping: bool) -> Self {
        use Control::{Disabled as Off, Enabled};
        if stopping {
            return match view.state {
                SessionState::Ended(_) => Controls::none(Disabled::Ended),
                _ => Controls::none(Disabled::Ending),
            };
        }
        // Pause needs a thread to name: with none known yet, it asks for
        // `threads` first; once the program has answered none, it can't.
        let pause = if view.threads.is_empty() && view.threads_answered {
            Off(Disabled::NoThreads)
        } else {
            Enabled
        };
        // A resume asked for and not yet taken reads as Resuming, but only
        // over the states it can be asked in: an end, a shutdown or a
        // restart that came first wins.
        let state = match view.state {
            SessionState::Stopped | SessionState::Uncertain if view.resume_requested => {
                &SessionState::Resuming
            }
            ref state => state,
        };
        match state {
            SessionState::Launching | SessionState::Initializing | SessionState::Configuring => {
                Controls {
                    stop: Enabled,
                    ..Controls::none(Disabled::Starting)
                }
            }
            SessionState::Running => Controls {
                continue_: Off(Disabled::Running),
                pause,
                step: Off(Disabled::Running),
                restart: Enabled,
                stop: Enabled,
            },
            SessionState::Stopped => Controls {
                continue_: Enabled,
                pause: Off(Disabled::AlreadyStopped),
                step: Enabled,
                restart: Enabled,
                stop: Enabled,
            },
            SessionState::Resuming => Controls {
                continue_: Off(Disabled::Resuming),
                pause,
                step: Off(Disabled::Resuming),
                restart: Off(Disabled::Resuming),
                stop: Enabled,
            },
            SessionState::Uncertain => Controls {
                continue_: Enabled,
                pause,
                step: Off(Disabled::Uncertain),
                restart: Off(Disabled::Uncertain),
                stop: Enabled,
            },
            SessionState::Restarting => Controls {
                stop: Enabled,
                ..Controls::none(Disabled::Restarting)
            },
            SessionState::Terminating => Controls::none(Disabled::Ending),
            SessionState::Ended(_) => Controls::none(Disabled::Ended),
        }
    }
}
