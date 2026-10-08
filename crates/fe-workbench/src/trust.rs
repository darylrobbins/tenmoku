//! Workspace trust (docs/decisions/lsp-client.md Q7): whether the
//! workbench's root may have language servers, which can run the project's
//! code.
//!
//! The decisions are fe-config's [`TrustStore`], held in a global that
//! [`set_trust_file`] loads; each window's [`Workbench`] follows its root's
//! trust there and emits [`TrustChanged`] when it changes. The first time
//! a folder is opened as one (a folder launch, or Open Folder…), the
//! workbench asks whether to trust it. `workbench.trust_folder` and
//! `workbench.untrust_folder` change the root's decision; their handlers
//! only name it, and the workbench records it ([`Workbench::set_root_trust`]).

use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_config::trust::{Trust, TrustStore, canonical};
use fe_gpui::commands::RunCommand;
use gpui::{
    App, AppContext as _, Context, Global, PromptButton, PromptLevel, Subscription, Task, Window,
};

use crate::Workbench;

/// The workbench's root became trusted (`true`) or stopped being trusted
/// (`false`): see [`Workbench::root_trusted`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustChanged(pub bool);

/// The prompt's question.
const QUESTION: &str = "Do you trust the authors of the files in this folder?";

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
/// decisions another fastedit wrote meanwhile are kept, even for a folder
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
/// lock so another fastedit's write can't be lost. A failure is logged:
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

/// Loads the trust decisions from `path` (fe-config's `trust.toml`), on
/// this thread, and turns prompting on: from now on the workbench asks
/// about a folder opened for the first time, and writes each decision to
/// `path`. Until this is called, decisions last the session, nothing is
/// asked, and every folder is untrusted.
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

/// Registers the trust commands, and the session's decisions unless
/// [`set_trust_file`] came first. Quitting waits for a write under way.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
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
fn decide(folder: &Path, trusted: bool, cx: &mut App) {
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

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A workbench's root, as trust sees it.
#[derive(Default)]
pub(crate) struct RootTrust {
    /// Bumped with each new root, so a resolution for an old one lands
    /// nowhere.
    generation: u64,
    /// There's a root.
    rooted: bool,
    /// The root's canonical path, once resolved, and whether it was opened
    /// as a folder (not as a file's parent), which is when it's prompted
    /// for.
    resolved: Option<(PathBuf, bool)>,
    /// A command's decision made before the root was resolved, for it.
    pending: Option<bool>,
    /// What [`Workbench::root_trusted`] last said.
    trusted: bool,
    /// The folder the prompt that shows asks about.
    asking: Option<PathBuf>,
    _observer: Option<Subscription>,
}

impl Workbench {
    /// Follows the trust decisions; call once, from `new`.
    pub(crate) fn follow_trust(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let observer = cx.observe_global_in::<TrustState>(window, |workbench, window, cx| {
            workbench.trust_changed(cx);
            workbench.ask_trust(window, cx);
        });
        self.trust._observer = Some(observer);
    }

    /// Resolves the trust of a new root, `root`, opened as a folder if
    /// `as_folder` (else as a file's parent), off the main thread; it's
    /// untrusted until then. Then asks about it, if that's due.
    pub(crate) fn resolve_trust(
        &mut self,
        root: Option<PathBuf>,
        as_folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.trust.generation += 1;
        self.trust.resolved = None;
        self.trust.pending = None;
        self.trust.rooted = root.is_some();
        self.trust_changed(cx);
        let Some(root) = root else {
            return;
        };
        let generation = self.trust.generation;
        let resolved = cx.background_spawn(async move { canonical(&root) });
        cx.spawn_in(window, async move |workbench, cx| {
            let resolved = resolved.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    if workbench.trust.generation != generation {
                        return;
                    }
                    workbench.trust.resolved = Some((resolved.clone(), as_folder));
                    if let Some(trusted) = workbench.trust.pending.take() {
                        decide(&resolved, trusted, cx);
                    }
                    workbench.trust_changed(cx);
                    workbench.ask_trust(window, cx);
                })
                .ok();
        })
        .detach();
    }

    /// Whether the root is trusted to start language servers in: true only
    /// once its canonical path is resolved and is `Trusted`, its own
    /// decision or its nearest decided ancestor's.
    pub fn root_trusted(&self) -> bool {
        self.trust.trusted
    }

    fn trust_of_root(&self, cx: &App) -> bool {
        let (Some((root, _)), Some(state)) = (&self.trust.resolved, cx.try_global::<TrustState>())
        else {
            return false;
        };
        state.store.trust_of(root) == Trust::Trusted
    }

    /// Emits [`TrustChanged`] if the root's trust changed.
    fn trust_changed(&mut self, cx: &mut Context<Self>) {
        let trusted = self.trust_of_root(cx);
        if trusted != self.trust.trusted {
            self.trust.trusted = trusted;
            cx.emit(TrustChanged(trusted));
            cx.notify();
        }
    }

    /// Records whether the root's canonical folder is trusted, as Trust
    /// This Folder and Untrust This Folder do, for every window. One
    /// before the root is resolved applies once it is; with no root it
    /// does nothing.
    pub fn set_root_trust(&mut self, trusted: bool, cx: &mut Context<Self>) {
        if !self.trust.rooted {
            return;
        }
        match &self.trust.resolved {
            Some((folder, _)) => {
                let folder = folder.clone();
                decide(&folder, trusted, cx);
                self.trust_changed(cx);
            }
            None => self.trust.pending = Some(trusted),
        }
    }

    /// Asks whether to trust the root, if prompting is on, the root is
    /// resolved and undecided, it was opened as a folder, and no prompt
    /// shows already. The answer applies to the folder asked about, even if
    /// the root has changed since; the new root is then asked about if
    /// it's undecided. Escape is "Don't Trust", and is remembered too.
    fn ask_trust(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = cx.try_global::<TrustState>() else {
            return;
        };
        let Some((folder, true)) = &self.trust.resolved else {
            return;
        };
        if state.path.is_none()
            || self.trust.asking.is_some()
            || state.store.trust_of(folder) != Trust::Undecided
        {
            return;
        }
        let folder = folder.clone();
        let detail = format!(
            "{}\n\nTrusting this folder lets fastedit start language servers in it, which \
             can run the project's code: rust-analyzer runs build scripts and proc macros. \
             Its subfolders are trusted too. Change this later with File > Trust This \
             Folder or Untrust This Folder.",
            folder.display()
        );
        let buttons = [
            PromptButton::ok("Trust Folder"),
            PromptButton::cancel("Don't Trust"),
        ];
        let answer = window.prompt(PromptLevel::Warning, QUESTION, Some(&detail), &buttons, cx);
        self.trust.asking = Some(folder.clone());
        cx.spawn_in(window, async move |workbench, cx| {
            let answer = answer.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.trust.asking = None;
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
                    workbench.trust_changed(cx);
                    workbench.ask_trust(window, cx);
                })
                .ok();
        })
        .detach();
    }
}

/// The [`CommandTarget`] of the trust commands: whether to trust the root.
pub(crate) struct TrustCommands;

impl CommandTarget for TrustCommands {
    type Tx = ();
    type Target<'a> = Option<bool>;
}

struct Commands(Rc<Registry<TrustCommands>>);

impl Global for Commands {}

pub(crate) fn registry() -> Registry<TrustCommands> {
    let commands = [
        ("workbench.trust_folder", "Trust This Folder", true),
        ("workbench.untrust_folder", "Untrust This Folder", false),
    ];
    let mut registry = Registry::new();
    for (id, title, trusted) in commands {
        registry
            .register(id, title, move |target: &mut Option<bool>, _: &Actor| {
                *target = Some(trusted);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("trust command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// Whether `run` trusts (`true`) or untrusts the root, if it's a trust
/// command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<bool> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut trusted = None;
    if let Err(error) = registry.dispatch(&mut trusted, &run.0) {
        log::warn!("{error}");
    }
    trusted
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
