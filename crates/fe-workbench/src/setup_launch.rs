//! When the setup wizard's sheet and the What's New tab open by themselves,
//! and the sheet on request.
//!
//! [`offer_setup`] is the first-launch offer and [`offer_whats_new`] the
//! first launch of a new version's: one decision for both, and its marker
//! writes, run off the main thread, so the first frame never waits on them.
//! `app.set_up` ([`set_up`]) opens the sheet whatever the marker says, but
//! never a second one in the app.

use std::path::{Path, PathBuf};

use fe_config::{
    first_run::{self, FirstRun, Outcome},
    whats_new::{self, WhatsNew},
};
use futures::{FutureExt as _, channel::oneshot, future::Shared};
use gpui::{App, AppContext as _, AsyncApp, Context, Global, Task, Window, WindowHandle};

use crate::{Workbench, setup_wizard::setup_paths};

/// Whether this launch may offer the wizard, or What's New.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupLaunch {
    /// An ordinary launch: offer the wizard on the machine's first, and
    /// What's New on a new version's.
    Normal,
    /// A launch that must not be interrupted (`--wait`, `--perf`): it shows
    /// nothing. On a machine with no marker and no earlier use it records
    /// `pending`, so that the files it writes meanwhile don't make the next
    /// ordinary launch take them for use; with any marker it writes nothing.
    /// It never writes What's New's.
    Never,
}

/// What the launch's decision found.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Decision {
    /// Show the wizard.
    setup: bool,
    /// Show What's New, then record it as seen: the state directory and
    /// the running version.
    whats_new: Option<(PathBuf, String)>,
}

type Decided = Shared<Task<Decision>>;

/// The decision [`begin_setup`] started, for [`offer_setup`] to take.
struct Started(Task<Decision>);

impl Global for Started {}

/// The decision [`offer_setup`] took, for [`offer_whats_new`].
struct Offered(Decided);

impl Global for Offered {}

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
    let task = decide(launch, true, cx);
    cx.set_global(Started(task));
}

/// The decision, in the background: whether to show the sheet (only with
/// `wizard`, else it's left alone) and What's New.
fn decide(launch: SetupLaunch, wizard: bool, cx: &mut App) -> Task<Decision> {
    let paths = setup_paths(cx);
    let config = paths
        .settings
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_owned);
    let state = paths.state;
    let running = crate::whats_new::running_version(cx);
    #[cfg(test)]
    let gate = cx
        .try_global::<SetupGate>()
        .and_then(|gate| gate.0.lock().unwrap().take());
    cx.background_spawn(async move {
        #[cfg(test)]
        if let Some(gate) = gate {
            gate.await.ok();
        }
        let first = first_run::decide(config.as_deref(), state.as_deref());
        let setup = wizard && decide_setup(first, launch, state.as_deref());
        let whats_new = decide_whats_new(first, launch, state, running);
        Decision { setup, whats_new }
    })
}

/// Whether to show the sheet, by `first`, recording the outcome.
fn decide_setup(first: FirstRun, launch: SetupLaunch, state: Option<&Path>) -> bool {
    let outcome = match (first, launch) {
        (FirstRun::Show, SetupLaunch::Normal) => Outcome::Shown,
        (FirstRun::Show, SetupLaunch::Never) => Outcome::Pending,
        (FirstRun::Existing, SetupLaunch::Normal) => Outcome::Existing,
        // A launch that shows nothing leaves "existing" for the next.
        (FirstRun::Existing, SetupLaunch::Never) | (FirstRun::Done, _) => return false,
    };
    // A pending marker is only for a machine with no marker at all.
    if outcome == Outcome::Pending
        && state.is_some_and(|state| state.join(first_run::MARKER_FILE).exists())
    {
        return false;
    }
    if let Some(state) = state
        && let Err(error) = first_run::record(state, outcome)
    {
        // Unrecorded, it would be offered again at every launch.
        log::warn!("can't record the setup decision: {error}");
        return false;
    }
    outcome == Outcome::Shown
}

/// Whether to show What's New for `running`, by `first` and its marker: on
/// a new machine (`first` shows the wizard) it records `running` and shows
/// nothing; a launch that must not be interrupted, or has no state
/// directory to record it in, shows and writes nothing.
fn decide_whats_new(
    first: FirstRun,
    launch: SetupLaunch,
    state: Option<PathBuf>,
    running: String,
) -> Option<(PathBuf, String)> {
    let state = state?;
    if launch == SetupLaunch::Never {
        return None;
    }
    if first == FirstRun::Show {
        // A new install: its notes are no news.
        if let Err(error) = whats_new::record(&state, &running) {
            log::warn!("can't record What's New as seen: {error}");
        }
        return None;
    }
    // No marker on a machine that isn't new: an upgrade from a build
    // before What's New.
    (whats_new::decide(&state, &running) == WhatsNew::Show).then_some((state, running))
}

/// Offers the setup wizard in `window` if this is the machine's first
/// launch, by the decision [`begin_setup`] started or else a new one. Call it
/// once, for the launch's first window, before [`offer_whats_new`].
pub fn offer_setup(window: WindowHandle<Workbench>, launch: SetupLaunch, cx: &mut App) {
    let decided = match cx.try_global::<Started>() {
        Some(_) => cx.remove_global::<Started>().0,
        None => decide(launch, true, cx),
    };
    if launch == SetupLaunch::Never {
        // Finishes its decision and its marker; shows nothing.
        decided.detach();
        return;
    }
    let decided = decided.shared();
    cx.set_global(Offered(decided.clone()));
    cx.spawn(async move |cx| {
        if decided.await.setup {
            // A window closed meanwhile takes nothing; one that showed a
            // sheet meanwhile, or another window that does, keeps the one.
            window.update(cx, set_up).ok();
        }
    })
    .detach();
}

/// Offers What's New in `window` if this is the first launch of a new
/// version on a machine that isn't new, by the decision [`offer_setup`]
/// took or else a new one (which leaves the wizard's marker alone). Call it
/// once, for the launch's first window, after [`offer_setup`]: other
/// windows never offer it.
///
/// The tab opens over what the launch puts in the window, never under it:
/// once `settled` resolves (the launch's files are in their tabs) and the
/// window's restored layout has its items ([`Workbench::restoring`]),
/// whichever the decision finishes before. The running version is recorded
/// once the tab has drawn.
pub fn offer_whats_new(
    window: WindowHandle<Workbench>,
    launch: SetupLaunch,
    settled: impl Future<Output = ()> + 'static,
    cx: &mut App,
) {
    let offered = cx
        .has_global::<Offered>()
        .then(|| cx.remove_global::<Offered>().0);
    if launch == SetupLaunch::Never {
        return;
    }
    let decided = offered.unwrap_or_else(|| decide(launch, false, cx).shared());
    cx.spawn(async move |cx| {
        let Some((state, version)) = decided.await.whats_new else {
            return;
        };
        settled.await;
        restored(window, cx).await;
        window
            .update(cx, |workbench, window, cx| {
                workbench.show_whats_new_once(state, version, window, cx)
            })
            .ok();
    })
    .detach();
}

/// Resolves once `window`'s restored layout has its items, at once if none
/// are being read, or once the window closes.
async fn restored(window: WindowHandle<Workbench>, cx: &mut AsyncApp) {
    let (done, finished) = oneshot::channel::<()>();
    let observing = window.update(cx, |workbench, _, cx| {
        if !workbench.restoring() {
            return None;
        }
        let mut done = Some(done);
        Some(cx.observe_self(move |workbench, _| {
            if !workbench.restoring()
                && let Some(done) = done.take()
            {
                done.send(()).ok();
            }
        }))
    });
    if let Ok(Some(_observing)) = observing {
        // Its sender goes with the window.
        finished.await.ok();
    }
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
