//! When the setup wizard's sheet opens by itself, and on request.
//!
//! [`offer_setup`] is the first-launch offer: the decision and the marker
//! write run off the main thread, so the first frame never waits on them.
//! `app.set_up` ([`set_up`]) opens the sheet whatever the marker says, but
//! never a second one in the app.

use std::path::Path;

use fe_config::first_run::{self, FirstRun, Outcome};
use gpui::{App, AppContext as _, Context, Global, Task, Window, WindowHandle};

use crate::{Workbench, setup_wizard::setup_paths};

/// Whether this launch may offer the wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupLaunch {
    /// An ordinary launch: offer it on the machine's first.
    Normal,
    /// A launch that must not be interrupted (`--wait`, `--perf`): it shows
    /// nothing. On a machine with no marker and no earlier use it records
    /// `pending`, so that the files it writes meanwhile don't make the next
    /// ordinary launch take them for use; with any marker it writes nothing.
    Never,
}

/// The decision [`begin_setup`] started, for [`offer_setup`] to take.
struct Started(Task<bool>);

impl Global for Started {}

/// Holds the background decision until the test releases it, so the launch's
/// own writes come first.
#[cfg(test)]
pub(crate) struct SetupGate(
    pub(crate) std::sync::Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
);

#[cfg(test)]
impl Global for SetupGate {}

/// Starts the first-launch decision, off the main thread, and returns at
/// once: nothing waits on it. Call it first thing in the app's run closure.
/// What the launch writes after that is not taken for earlier use however the
/// two interleave, by [`first_run::note_writing`] before each such write.
/// [`offer_setup`] takes the outcome.
pub fn begin_setup(launch: SetupLaunch, cx: &mut App) {
    let task = decide(launch, cx);
    cx.set_global(Started(task));
}

/// The decision, in the background: whether to show the sheet.
fn decide(launch: SetupLaunch, cx: &mut App) -> Task<bool> {
    let paths = setup_paths(cx);
    let config = paths
        .settings
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_owned);
    let state = paths.state;
    #[cfg(test)]
    let gate = cx
        .try_global::<SetupGate>()
        .and_then(|gate| gate.0.lock().unwrap().take());
    cx.background_spawn(async move {
        #[cfg(test)]
        if let Some(gate) = gate {
            gate.await.ok();
        }
        let decision = first_run::decide(config.as_deref(), state.as_deref());
        let outcome = match (decision, launch) {
            (FirstRun::Show, SetupLaunch::Normal) => Outcome::Shown,
            (FirstRun::Show, SetupLaunch::Never) => Outcome::Pending,
            (FirstRun::Existing, SetupLaunch::Normal) => Outcome::Existing,
            // A launch that shows nothing leaves "existing" for the next.
            (FirstRun::Existing, SetupLaunch::Never) | (FirstRun::Done, _) => return false,
        };
        // A pending marker is only for a machine with no marker at all.
        if outcome == Outcome::Pending
            && state
                .as_deref()
                .is_some_and(|state| state.join(first_run::MARKER_FILE).exists())
        {
            return false;
        }
        if let Some(state) = &state
            && let Err(error) = first_run::record(state, outcome)
        {
            // Unrecorded, it would be offered again at every launch.
            log::warn!("can't record the setup decision: {error}");
            return false;
        }
        outcome == Outcome::Shown
    })
}

/// Offers the setup wizard in `window` if this is the machine's first
/// launch, by the decision [`begin_setup`] started or else a new one. Call it
/// once, for the launch's first window.
pub fn offer_setup(window: WindowHandle<Workbench>, launch: SetupLaunch, cx: &mut App) {
    let decided = match cx.try_global::<Started>() {
        Some(_) => cx.remove_global::<Started>().0,
        None => decide(launch, cx),
    };
    if launch == SetupLaunch::Never {
        // Finishes its decision and its marker; shows nothing.
        decided.detach();
        return;
    }
    cx.spawn(async move |cx| {
        if decided.await {
            // A window closed meanwhile takes nothing; one that showed a
            // sheet meanwhile, or another window that does, keeps the one.
            window.update(cx, set_up).ok();
        }
    })
    .detach();
}

/// `app.set_up`: the sheet in `workbench`'s window, or if another window
/// shows one, that window.
pub(crate) fn set_up(workbench: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) {
    if workbench.is_setup_showing() {
        return;
    }
    // This window is being updated, so its handle can't be read: it is not
    // among the others.
    let showing = (cx.windows().into_iter())
        .filter_map(|other| other.downcast::<Workbench>())
        .find(|other| other.read(cx).is_ok_and(|other| other.is_setup_showing()));
    match showing {
        Some(other) => {
            other.update(cx, |_, other, _| other.activate_window()).ok();
        }
        None => workbench.show_setup(window, cx),
    }
}
