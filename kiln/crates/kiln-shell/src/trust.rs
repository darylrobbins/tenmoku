// SPDX-License-Identifier: Apache-2.0

//! Workspace trust: whether a window's root folder may run its project's
//! code, such as through language servers that run build scripts.
//!
//! The decisions are kiln-core's [`TrustStore`], held in a global that
//! [`set_trust_file`] loads and every window shares; deciding writes the
//! decision off the main thread, and quitting waits for it. Each window's
//! host view holds a [`RootTrust`] for its root, follows the decisions
//! and emits [`TrustChanged`] when its root's trust changes. The first
//! time a folder is opened as one, it asks whether to trust it, with the
//! host's [`TrustPrompt`]. Any other folder's trust is the decision
//! [`decided`] gives it, which [`observe`] follows.
//!
//! ```ignore
//! impl TrustHost for Notes {
//!     fn root_trust(&mut self) -> &mut RootTrust {
//!         &mut self.trust
//!     }
//! }
//!
//! RootTrust::follow(&mut notes, window, cx);
//! RootTrust::resolve(&mut notes, Some(root), true, window, cx);
//! ```

use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
};

use gpui::{
    App, AppContext as _, Context, EventEmitter, Global, PromptButton, PromptLevel, Subscription,
    Task, Window,
};
use kiln_core::trust::{Trust, TrustStore, canonical};
use kiln_l10n::{Text, text};

/// The host's root became trusted (`true`) or stopped being trusted
/// (`false`): see [`RootTrust::trusted`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustChanged(pub bool);

/// What the trust prompt says: the question, the detail under it for the
/// folder asked about, and its two buttons. Each is resolved in the active
/// locale when the prompt shows, so a host builds this once.
pub struct TrustPrompt {
    pub question: Text,
    pub detail: Box<dyn Fn(&Path) -> String>,
    pub trust: Text,
    pub dont_trust: Text,
}

impl TrustPrompt {
    /// A prompt asking `question`, with `detail` for a folder under it,
    /// and "Trust Folder" and "Don't Trust" buttons.
    pub fn new(question: impl Into<Text>, detail: impl Fn(&Path) -> String + 'static) -> Self {
        Self {
            question: question.into(),
            detail: Box::new(detail),
            trust: text!("kiln-shell-trust-folder"),
            dont_trust: text!("kiln-shell-trust-dont"),
        }
    }
}

/// The decisions, and where they're written.
#[derive(Default)]
struct TrustState {
    store: TrustStore,
    /// The trust file: `None` until [`set_trust_file`], when decisions last
    /// the session and no prompt is shown.
    path: Option<PathBuf>,
    writes: Arc<Mutex<Writes>>,
    /// The running writer, which quitting waits for.
    writer: Rc<RefCell<Option<Task<()>>>>,
}

impl Global for TrustState {}

/// The decisions still to write to the trust file, by one writer at a
/// time. Each write applies them to the file as it is then, so the
/// decisions another process wrote meanwhile are kept, even for a folder
/// decided here earlier and written already.
#[derive(Default)]
struct Writes {
    /// The trust file.
    path: Option<PathBuf>,
    /// Each folder decided and not written yet: the decision, and its
    /// number, which tells a newer decision from the one being written.
    pending: BTreeMap<PathBuf, (bool, u64)>,
    numbered: u64,
    /// A writer is running, and will write what's pending.
    running: bool,
}

/// Reads the trust file at `path` as it is now (one gone bad stays as it
/// is), applies `decisions` and writes it back, all under the store's
/// lock so another process's write can't be lost. A failure is logged:
/// the decisions hold for the session.
fn write(path: &Path, decisions: &BTreeMap<PathBuf, (bool, u64)>) {
    let updated = TrustStore::update(path, |store| {
        for (folder, &(trusted, _)) in decisions {
            store.decide(folder, trusted);
            if folder.to_str().is_none() {
                log::error!(
                    "{} isn't UTF-8, so its trust can't be saved; it holds for this session",
                    folder.display()
                );
            }
        }
    });
    if let Err(error) = updated {
        log::error!(
            "can't save trust decisions in {}: {error}; they hold for this session",
            path.display()
        );
    }
}

/// Loads the trust decisions from `path`, on this thread, and turns
/// prompting on: from now on a host asks about a folder opened for the
/// first time, and each decision is written to `path`. Until this is
/// called, decisions last the session, nothing is asked, and every folder
/// is untrusted.
pub fn set_trust_file(cx: &mut App, path: PathBuf) {
    let (writes, writer) = cx
        .try_global::<TrustState>()
        .map(|state| (state.writes.clone(), state.writer.clone()))
        .unwrap_or_default();
    {
        // Decisions not written yet were for the old file.
        let mut writes = lock(&writes);
        writes.path = Some(path.clone());
        writes.pending.clear();
    }
    cx.set_global(TrustState {
        store: TrustStore::load(&path),
        path: Some(path),
        writes,
        writer,
    });
}

/// Holds the session's decisions unless [`set_trust_file`] came first,
/// and has quitting wait for a write under way. Call once, at startup.
pub fn init(cx: &mut App) {
    if !cx.has_global::<TrustState>() {
        cx.set_global(TrustState::default());
    }
    cx.on_app_quit(|cx| {
        let writer = cx
            .try_global::<TrustState>()
            .and_then(|state| state.writer.take());
        async move {
            if let Some(writer) = writer {
                writer.await;
            }
        }
    })
    .detach();
}

/// Records whether canonical folder `folder` is trusted, for every window
/// at once, and writes it off the main thread, if there's a trust file.
/// A failed write is logged; the decision still holds for the session.
pub fn decide(folder: &Path, trusted: bool, cx: &mut App) {
    if !cx.has_global::<TrustState>() {
        return;
    }
    let state = cx.global_mut::<TrustState>();
    state.store.decide(folder, trusted);
    if state.path.is_none() {
        return;
    }
    let (writes, writer) = (state.writes.clone(), state.writer.clone());
    {
        let mut writes = lock(&writes);
        writes.numbered += 1;
        let number = writes.numbered;
        writes.pending.insert(folder.to_owned(), (trusted, number));
        if writes.running {
            return;
        }
        writes.running = true;
    }
    let task = cx.background_spawn(async move {
        loop {
            let (path, decisions) = {
                let mut writes = lock(&writes);
                match writes.path.clone() {
                    Some(path) if !writes.pending.is_empty() => (path, writes.pending.clone()),
                    _ => {
                        writes.running = false;
                        return;
                    }
                }
            };
            write(&path, &decisions);
            // Written, or failed for the session: not again, unless decided
            // anew meanwhile.
            let mut writes = lock(&writes);
            for (folder, (_, number)) in decisions {
                if writes
                    .pending
                    .get(&folder)
                    .is_some_and(|&(_, n)| n == number)
                {
                    writes.pending.remove(&folder);
                }
            }
        }
    });
    writer.replace(Some(task));
}

/// The decision recorded for canonical path `path`, or for its nearest
/// decided ancestor: `None` if neither it nor an ancestor has one. No I/O.
pub fn decided(path: &Path, cx: &App) -> Option<bool> {
    trust_of(cx.try_global::<TrustState>()?.store.trust_of(path))
}

/// The decisions as they are now, as [`decided`] gives them, for a check
/// off the main thread or without the app; they don't follow later ones
/// ([`observe`] does).
pub fn decisions(cx: &App) -> impl Fn(&Path) -> Option<bool> + Send + Sync + 'static {
    let store = cx
        .try_global::<TrustState>()
        .map(|state| state.store.clone());
    move |path| trust_of(store.as_ref()?.trust_of(path))
}

/// Calls `changed` on `host` whenever a decision may have changed, any
/// folder's, in any window, for as long as the subscription is held.
pub fn observe<H: 'static>(
    cx: &mut Context<H>,
    mut changed: impl FnMut(&mut H, &mut Context<H>) + 'static,
) -> Subscription {
    cx.observe_global::<TrustState>(move |host, cx| changed(host, cx))
}

fn trust_of(trust: Trust) -> Option<bool> {
    match trust {
        Trust::Trusted => Some(true),
        Trust::Untrusted => Some(false),
        Trust::Undecided => None,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A view holding a [`RootTrust`]: it emits [`TrustChanged`] for it.
pub trait TrustHost: EventEmitter<TrustChanged> + Sized + 'static {
    fn root_trust(&mut self) -> &mut RootTrust;
}

/// A host's root folder, as trust sees it.
pub struct RootTrust {
    prompt: Rc<TrustPrompt>,
    /// Bumped with each new root, so a resolution for an old one lands
    /// nowhere.
    generation: u64,
    /// There's a root.
    rooted: bool,
    /// The root's canonical path, once resolved, and whether it was opened
    /// as a folder (not as a file's parent), which is when it's prompted
    /// for.
    resolved: Option<(PathBuf, bool)>,
    /// A decision made before the root was resolved, for it.
    pending: Option<bool>,
    /// What [`RootTrust::trusted`] last said.
    trusted: bool,
    /// The folder the prompt that shows asks about.
    asking: Option<PathBuf>,
    _observer: Option<Subscription>,
}

impl RootTrust {
    /// No root yet, asking with `prompt` once one is resolved.
    pub fn new(prompt: TrustPrompt) -> Self {
        Self {
            prompt: Rc::new(prompt),
            generation: 0,
            rooted: false,
            resolved: None,
            pending: None,
            trusted: false,
            asking: None,
            _observer: None,
        }
    }

    /// Whether the root is trusted: true only once its canonical path is
    /// resolved and is `Trusted`, its own decision or its nearest decided
    /// ancestor's.
    pub fn trusted(&self) -> bool {
        self.trusted
    }

    /// Has `host` follow the decisions, wherever they're made; call once,
    /// as it's made.
    pub fn follow<H: TrustHost>(host: &mut H, window: &mut Window, cx: &mut Context<H>) {
        let observer = cx.observe_global_in::<TrustState>(window, |host, window, cx| {
            Self::changed(host, cx);
            Self::ask(host, window, cx);
        });
        host.root_trust()._observer = Some(observer);
    }

    /// Resolves the trust of `host`'s new root, `root`, opened as a folder
    /// if `as_folder` (else as a file's parent), off the main thread; it's
    /// untrusted until then. Then asks about it, if that's due.
    pub fn resolve<H: TrustHost>(
        host: &mut H,
        root: Option<PathBuf>,
        as_folder: bool,
        window: &mut Window,
        cx: &mut Context<H>,
    ) {
        let trust = host.root_trust();
        trust.generation += 1;
        trust.resolved = None;
        trust.pending = None;
        trust.rooted = root.is_some();
        let generation = trust.generation;
        Self::changed(host, cx);
        let Some(root) = root else {
            return;
        };
        let resolved = cx.background_spawn(async move { canonical(&root) });
        cx.spawn_in(window, async move |host, cx| {
            let resolved = resolved.await;
            host.update_in(cx, |host, window, cx| {
                let trust = host.root_trust();
                if trust.generation != generation {
                    return;
                }
                trust.resolved = Some((resolved.clone(), as_folder));
                if let Some(trusted) = trust.pending.take() {
                    decide(&resolved, trusted, cx);
                }
                Self::changed(host, cx);
                Self::ask(host, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Records whether `host`'s root is trusted, for every window. One
    /// before the root is resolved applies once it is; with no root it
    /// does nothing.
    pub fn set_trusted<H: TrustHost>(host: &mut H, trusted: bool, cx: &mut Context<H>) {
        let trust = host.root_trust();
        if !trust.rooted {
            return;
        }
        match &trust.resolved {
            Some((folder, _)) => {
                let folder = folder.clone();
                decide(&folder, trusted, cx);
                Self::changed(host, cx);
            }
            None => trust.pending = Some(trusted),
        }
    }

    fn trust_of_root(&self, cx: &App) -> bool {
        let (Some((root, _)), Some(state)) = (&self.resolved, cx.try_global::<TrustState>()) else {
            return false;
        };
        state.store.trust_of(root) == Trust::Trusted
    }

    /// Emits [`TrustChanged`] if the root's trust changed.
    fn changed<H: TrustHost>(host: &mut H, cx: &mut Context<H>) {
        let trust = host.root_trust();
        let trusted = trust.trust_of_root(cx);
        if trusted != trust.trusted {
            trust.trusted = trusted;
            cx.emit(TrustChanged(trusted));
            cx.notify();
        }
    }

    /// Asks whether to trust the root, if prompting is on, the root is
    /// resolved and undecided, it was opened as a folder, and no prompt
    /// shows already. The answer applies to the folder asked about, even if
    /// the root has changed since; the new root is then asked about if
    /// it's undecided. Escape is "Don't Trust", and is remembered too.
    fn ask<H: TrustHost>(host: &mut H, window: &mut Window, cx: &mut Context<H>) {
        let Some(state) = cx.try_global::<TrustState>() else {
            return;
        };
        let trust = host.root_trust();
        let Some((folder, true)) = &trust.resolved else {
            return;
        };
        if state.path.is_none()
            || trust.asking.is_some()
            || state.store.trust_of(folder) != Trust::Undecided
        {
            return;
        }
        let folder = folder.clone();
        let prompt = trust.prompt.clone();
        let detail = (prompt.detail)(&folder);
        let buttons = [
            PromptButton::ok(prompt.trust.resolve()),
            PromptButton::cancel(prompt.dont_trust.resolve()),
        ];
        let answer = window.prompt(
            PromptLevel::Warning,
            &prompt.question.resolve(),
            Some(&detail),
            &buttons,
            cx,
        );
        trust.asking = Some(folder.clone());
        cx.spawn_in(window, async move |host, cx| {
            let answer = answer.await;
            host.update_in(cx, |host, window, cx| {
                host.root_trust().asking = None;
                // Gone without an answer (the window closed): nothing
                // is decided. Nor is a folder given a decision of its
                // own meanwhile (in another window, say): that one
                // stands. One an ancestor got doesn't count.
                let undecided = cx
                    .try_global::<TrustState>()
                    .is_some_and(|state| state.store.decided(&folder).is_none());
                if let Ok(button) = answer
                    && undecided
                {
                    decide(&folder, button == 0, cx);
                }
                Self::changed(host, cx);
                Self::ask(host, window, cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui::TestAppContext;

    use super::{decide, decided, decisions, init};

    #[gpui::test]
    fn decided_is_the_nearest_decided_ancestors(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(decided(Path::new("/t"), cx), None, "no decisions yet");
            init(cx);
            decide(Path::new("/t"), true, cx);
            decide(Path::new("/t/w/no"), false, cx);
            let snapshot = decisions(cx);
            for (path, expected) in [
                ("/t", Some(true)),
                ("/t/other/src", Some(true)),
                ("/t/w/no", Some(false)),
                ("/t/w/no/deeper", Some(false)),
                ("/t/w", Some(true)),
                // By components: `/tt` isn't under `/t`.
                ("/tt", None),
                ("/", None),
            ] {
                assert_eq!(decided(Path::new(path), cx), expected, "{path}");
                assert_eq!(snapshot(Path::new(path)), expected, "{path}");
            }
            // The snapshot keeps the decisions it was taken with.
            decide(Path::new("/tt"), true, cx);
            assert_eq!(decided(Path::new("/tt/x"), cx), Some(true));
            assert_eq!(snapshot(Path::new("/tt/x")), None);
        });
    }
}
