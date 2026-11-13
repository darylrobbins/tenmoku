//! Undo and redo of the project panel's file operations, as VS Code's
//! explorer undoes them with cmd-z: the last [`DEPTH`] creates, renames,
//! moves (a cut's paste or a drop), duplicates (and pasted copies) and
//! moves to the Trash, each undone as a whole, however many entries it
//! did.
//!
//! What was made is undone by moving it to the Trash; what was renamed or
//! moved, by moving it back, as a cut's paste moves it
//! ([`ProjectPanel::move_back`]): its editors take the old path
//! ([`PanelEvent::Renamed`]) and language servers whose `willRename`
//! filters match are asked first. What went to the Trash is put back from
//! where the Trash says it went ([`fs_ops::restore`]). Undone, a step goes
//! on the redo stack as what the undo did, and redo undoes that; a new
//! operation that changed something empties the redo stack, even while an
//! undo is under way (its redo step is then dropped).
//!
//! Only where the Trash says where it put an entry
//! ([`fs_ops::restores_from_trash`]: macOS) is anything recorded, and are
//! the commands there; elsewhere a create undone couldn't be redone.
//!
//! Each entry is recorded as what the operation made or moved (its device
//! and inode, as the operation found it), and checked before it's
//! reverted. One that's gone, or no longer in the Trash, drops the step,
//! with a toast. One replaced since is reverted as it is if an open editor
//! last read or saved it as it is now (saves replace a file); otherwise
//! the panel asks first. A path taken that an entry would go back to, or
//! something acting on its paths ([`Workbench::busy_under`]), refuses the
//! step, with a toast, and it stays to revert once that's over. An entry
//! that fails as it's reverted stays to revert again, unless it was found
//! changed; those reverted go the other way.
//!
//! Steps are kept with the tree's (canonical) root, and dropped once
//! another root is open. `project_panel.undo` and `project_panel.redo` are
//! the tree's own (cmd-z and cmd-shift-z while it has focus). In the name
//! field and the filter's box the panel gives them to the box, to undo its
//! text.

use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
};

use fe_project::{
    FsChange,
    fs_ops::{self, EntryId},
};
use gpui::{AppContext as _, Context, PromptButton, PromptLevel, Window};
use kiln_l10n::tr;

use super::{
    PanelEvent, ProjectPanel,
    clipboard::file_name,
    ops::{self, Failure, Many, Running},
};
use crate::{Workbench, workbench::Busy};

/// How many steps are kept to undo.
pub(super) const DEPTH: usize = 20;

/// Whether file operations are recorded to undo here: where what goes to
/// the Trash can be put back.
pub(super) const fn supported() -> bool {
    fs_ops::restores_from_trash()
}

/// Which way through the history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    Undo,
    Redo,
}

/// What an operation did, as it's reverted.
#[derive(Clone, Debug)]
pub(super) enum Step {
    /// Entries made (a new file or folder, a duplicate, a pasted copy) or
    /// put back from the Trash: reverted by moving them to the Trash.
    Made(Vec<Made>),
    /// Entries renamed or moved: reverted by moving each back.
    Moved(Vec<Moved>),
    /// Entries moved to the Trash: reverted by putting each back.
    Trashed(Vec<fs_ops::Trashed>),
}

/// An entry made, at `path` under the root, and what it is.
#[derive(Clone, Debug)]
pub(super) struct Made {
    pub(super) path: PathBuf,
    pub(super) identity: EntryId,
}

impl From<fs_ops::Placed> for Made {
    fn from(placed: fs_ops::Placed) -> Self {
        Self {
            path: placed.path,
            identity: placed.id,
        }
    }
}

/// An entry moved `from` `to`, both under the root, and what it is.
#[derive(Clone, Debug)]
pub(super) struct Moved {
    pub(super) from: PathBuf,
    pub(super) to: PathBuf,
    pub(super) identity: EntryId,
}

impl Step {
    fn is_empty(&self) -> bool {
        match self {
            Step::Made(made) => made.is_empty(),
            Step::Moved(moved) => moved.is_empty(),
            Step::Trashed(trashed) => trashed.is_empty(),
        }
    }

    /// Each entry's path now, and what it's recorded as: what's checked
    /// before it's reverted, and refreshed after the app writes it.
    fn entries_mut(&mut self) -> Vec<(&Path, &mut EntryId)> {
        match self {
            Step::Made(made) => (made.iter_mut())
                .map(|made| (made.path.as_path(), &mut made.identity))
                .collect(),
            Step::Moved(moved) => (moved.iter_mut())
                .map(|moved| (moved.to.as_path(), &mut moved.identity))
                .collect(),
            Step::Trashed(_) => Vec::new(),
        }
    }
}

/// A revert under way: which way, and the history as it began, so what it
/// did lands where it belongs whatever was done meanwhile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reverting {
    pub(super) way: Way,
    /// [`History::generation`] then.
    generation: u64,
    /// [`History::pushed`] then.
    pushed: u64,
}

/// A step, the root its paths are under, and which it is.
struct Entry {
    serial: u64,
    root: PathBuf,
    step: Step,
}

/// The panel's steps to undo and redo.
#[derive(Default)]
pub(super) struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    /// Whether a step is being reverted: another waits.
    reverting: bool,
    /// Moves on with each new operation that changed something, which
    /// empties the redo stack.
    generation: u64,
    /// Counts the steps put on the undo stack.
    pushed: u64,
    /// Counts the steps, to tell one from another.
    serial: u64,
}

impl History {
    fn entry(&mut self, root: PathBuf, step: Step) -> Entry {
        self.serial += 1;
        Entry {
            serial: self.serial,
            root,
            step,
        }
    }

    /// Puts `entry` on the redo stack, the oldest dropped past [`DEPTH`].
    fn push_redo(&mut self, entry: Entry) {
        self.redo.push(entry);
        if self.redo.len() > DEPTH {
            self.redo.drain(..self.redo.len() - DEPTH);
        }
    }

    /// Puts `entry` on the undo stack, `back` steps below its top.
    fn push_undo(&mut self, entry: Entry, back: usize) {
        let at = self.undo.len().saturating_sub(back);
        self.undo.insert(at, entry);
        self.pushed += 1;
        while self.undo.len() > DEPTH {
            self.undo.pop_front();
        }
    }

    /// Takes in a new operation's `step`, under `root`: one that `changed`
    /// something empties the redo stack.
    fn landed_new(&mut self, root: PathBuf, step: Step, changed: bool) {
        if changed {
            self.redo.clear();
            self.generation += 1;
        }
        if !step.is_empty() {
            let entry = self.entry(root, step);
            self.push_undo(entry, 0);
        }
    }

    /// Takes in what `reverting` did, `done`, which goes the other way, and
    /// what's `kept` of its step to revert again, which goes back where it
    /// came from: unless a new operation emptied the redo stack meanwhile.
    fn landed_revert(&mut self, reverting: Reverting, root: PathBuf, done: Step, kept: Step) {
        self.reverting = false;
        let redo_kept = self.generation == reverting.generation;
        // Below the steps operations put there meanwhile.
        let since = usize::try_from(self.pushed - reverting.pushed).unwrap_or(usize::MAX);
        if !kept.is_empty() {
            let entry = self.entry(root.clone(), kept);
            match reverting.way {
                Way::Undo => self.push_undo(entry, since),
                Way::Redo if redo_kept => self.push_redo(entry),
                Way::Redo => {}
            }
        }
        if !done.is_empty() {
            let entry = self.entry(root, done);
            match reverting.way {
                Way::Undo if redo_kept => self.push_redo(entry),
                Way::Undo => {}
                Way::Redo => self.push_undo(entry, 0),
            }
        }
    }

    fn stack(&self, way: Way) -> Option<&Entry> {
        match way {
            Way::Undo => self.undo.back(),
            Way::Redo => self.redo.last(),
        }
    }

    fn stack_mut(&mut self, way: Way) -> Option<&mut Entry> {
        match way {
            Way::Undo => self.undo.back_mut(),
            Way::Redo => self.redo.last_mut(),
        }
    }

    fn pop(&mut self, way: Way) -> Option<Entry> {
        match way {
            Way::Undo => self.undo.pop_back(),
            Way::Redo => self.redo.pop(),
        }
    }

    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// What the entry at a path is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Probe {
    /// There's none.
    Missing,
    Is(EntryId),
    /// It can't be told now (a folder on the way can't be searched, say).
    Unknown,
}

/// What the entry at `path` is now, a symlink itself rather than its
/// target.
pub(super) fn probe(path: &Path) -> Probe {
    match kiln::core::platform::path_identity(path, false) {
        Ok(id) => Probe::Is(id),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Probe::Missing
        }
        Err(_) => Probe::Unknown,
    }
}

/// What an entry of a step would meet if it were reverted now.
enum Member {
    Ready,
    /// It never can be (it's gone, or no longer in the Trash): dropped
    /// from its step, with why.
    Gone(Failure),
    /// Not now (a path it goes back to is taken, it can't be reached, or
    /// something acts on it): kept, with why.
    Wait(Failure),
    /// It was replaced since, and is this now: reverted only if the user
    /// says so.
    Changed(EntryId),
}

impl Step {
    /// The entries' paths now, in order.
    fn paths(&self) -> Vec<&Path> {
        match self {
            Step::Made(made) => made.iter().map(|made| made.path.as_path()).collect(),
            Step::Moved(moved) => moved.iter().map(|moved| moved.to.as_path()).collect(),
            Step::Trashed(trashed) => trashed.iter().map(fs_ops::Trashed::path).collect(),
        }
    }

    /// Keeps the entries whose index `keep` holds for.
    fn retain(&mut self, keep: impl Fn(usize) -> bool) {
        fn retain<T>(items: &mut Vec<T>, keep: impl Fn(usize) -> bool) {
            let mut ix = 0;
            items.retain(|_| {
                ix += 1;
                keep(ix - 1)
            });
        }
        match self {
            Step::Made(made) => retain(made, keep),
            Step::Moved(moved) => retain(moved, keep),
            Step::Trashed(trashed) => retain(trashed, keep),
        }
    }
}

impl ProjectPanel {
    /// Takes in what a new operation did, `step`, under the tree's
    /// (canonical) `root`; one that `changed` something, whether or not it
    /// can be undone, leaves nothing to redo.
    pub(super) fn landed_new(&mut self, root: &Path, step: Step, changed: bool) {
        if supported() {
            self.history.landed_new(root.to_owned(), step, changed);
        }
    }

    /// Takes in what `reverting` did and what's kept of its step.
    pub(super) fn landed_revert(
        &mut self,
        reverting: Reverting,
        root: &Path,
        done: Step,
        kept: Step,
    ) {
        self.history
            .landed_revert(reverting, root.to_owned(), done, kept);
    }

    /// Undoes or redoes the last step, `way`. Its entries that never can
    /// be are dropped from it first, with a toast; then, should an entry
    /// have to wait, a toast says why and it stays; should one have been
    /// replaced since, the panel asks first.
    pub(super) fn revert(&mut self, way: Way, window: &mut Window, cx: &mut Context<Self>) {
        if !supported() || self.history.reverting || self.clipboard_moving() {
            return;
        }
        let Some(root) = self.tree().map(|tree| tree.root_path().to_owned()) else {
            return;
        };
        let Some(entry) = self.history.stack(way) else {
            return;
        };
        if entry.root != root {
            // Of another root: none of them apply.
            return self.history.clear();
        }
        let serial = entry.serial;
        let members = self.members(&entry.step, &root, window, cx);
        let names: Vec<String> = entry.step.paths().into_iter().map(file_name).collect();
        let mut gone = Vec::new();
        let mut wait = None;
        let mut changed = Vec::new();
        for (ix, member) in members.into_iter().enumerate() {
            match member {
                Member::Ready => {}
                Member::Gone(failure) => gone.push((ix, failure)),
                Member::Wait(failure) => {
                    wait.get_or_insert(failure);
                }
                Member::Changed(now) => changed.push((ix, now)),
            }
        }
        if !gone.is_empty() {
            let dropped: Vec<usize> = gone.iter().map(|(ix, _)| *ix).collect();
            if let Some(entry) = self.history.stack_mut(way) {
                entry.step.retain(|ix| !dropped.contains(&ix));
                if entry.step.is_empty() {
                    self.history.pop(way);
                }
            }
            let failures = gone.into_iter().map(|(_, failure)| failure).collect();
            ops::report_failures(failures, Many::Undo, cx);
            // The rest are reverted, if there are any; never an older step.
            if self
                .history
                .stack(way)
                .is_some_and(|entry| entry.serial == serial)
            {
                self.revert(way, window, cx);
            }
            return;
        }
        if let Some(failure) = wait {
            return failure.show(cx);
        }
        if !changed.is_empty() {
            let named = changed.iter().map(|(ix, _)| names[*ix].clone()).collect();
            return self.ask_to_revert(way, serial, changed, named, window, cx);
        }
        let Some(Entry { step, .. }) = self.history.pop(way) else {
            return;
        };
        let reverting = Reverting {
            way,
            generation: self.history.generation,
            pushed: self.history.pushed,
        };
        self.history.reverting = true;
        match step {
            Step::Made(made) => self.trash_made(reverting, root, made, window, cx),
            Step::Trashed(trashed) => self.put_back(reverting, root, trashed, cx),
            Step::Moved(moved) => self.move_back(reverting, root, moved, window, cx),
        }
        cx.notify();
    }

    /// Asks whether to revert step `serial`, whose entries `changed` (by
    /// index, with what each is now; `named` by name) were replaced since,
    /// and does if the answer is yes and it's still the next to revert
    /// `way`: taking each as what it was shown as, unless it changed again
    /// meanwhile, when it's asked about again.
    fn ask_to_revert(
        &mut self,
        way: Way,
        serial: u64,
        changed: Vec<(usize, EntryId)>,
        named: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = named.len();
        let first = named.first().cloned().unwrap_or_default();
        let (question, go) = match way {
            Way::Undo => (
                tr!(
                    "workbench-project-undo-changed-question",
                    count = count,
                    name = first
                ),
                tr!("workbench-project-undo-anyway"),
            ),
            Way::Redo => (
                tr!(
                    "workbench-project-redo-changed-question",
                    count = count,
                    name = first
                ),
                tr!("workbench-project-redo-anyway"),
            ),
        };
        // Each one, a line apiece, when there are several.
        let detail = (count > 1).then(|| named.join("\n"));
        // Cancel first, as the Trash's question has it: Return cancels.
        let buttons = [
            PromptButton::Ok(tr!("workbench-project-cancel")),
            PromptButton::Other(go),
        ];
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            detail.as_deref(),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |panel, cx| {
            if answer.await != Ok(1) {
                return;
            }
            panel
                .update_in(cx, |panel, window, cx| {
                    let Some(entry) = panel.history.stack_mut(way) else {
                        return;
                    };
                    if entry.serial != serial {
                        return;
                    }
                    let mut entries = entry.step.entries_mut();
                    for (ix, shown) in changed {
                        let Some((path, identity)) = entries.get_mut(ix) else {
                            continue;
                        };
                        if probe(path) == Probe::Is(shown) {
                            **identity = shown;
                        }
                    }
                    panel.revert(way, window, cx);
                })
                .ok();
        })
        .detach();
    }

    /// What each entry of `step`, under `root`, would meet if it were
    /// reverted now, in order.
    fn members(&self, step: &Step, root: &Path, window: &Window, cx: &gpui::App) -> Vec<Member> {
        let unreachable = |path: &Path| {
            let message = tr!("workbench-project-undo-unreachable", name = file_name(path));
            Member::Wait(Failure::said(message))
        };
        let taken = |path: &Path| {
            let message = tr!("workbench-project-undo-taken", name = file_name(path));
            Member::Wait(Failure::said(message))
        };
        let busy = |path: &Path| {
            let shown = self.shown(path, root)?;
            let busy = Workbench::busy_under(&shown, window, cx)?;
            Some(Member::Wait(busy_failure(busy, &file_name(path))))
        };
        // The entry at `path`, recorded as `identity`.
        let what = |path: &Path, identity: EntryId| match probe(path) {
            Probe::Missing => {
                let message = tr!("workbench-project-undo-changed", name = file_name(path));
                Member::Gone(Failure::said(message))
            }
            Probe::Unknown => unreachable(path),
            Probe::Is(now) if now == identity => Member::Ready,
            Probe::Is(now) => Member::Changed(now),
        };
        // Something acting on it waits before anything but its being gone.
        let or_busy = |member: Member, paths: &[&Path]| match member {
            Member::Gone(_) => member,
            _ => (paths.iter()).find_map(|path| busy(path)).unwrap_or(member),
        };
        match step {
            Step::Made(made) => (made.iter())
                .map(|made| or_busy(what(&made.path, made.identity), &[&made.path]))
                .collect(),
            Step::Moved(moved) => (moved.iter())
                .map(|moved| {
                    let member = match (probe(&moved.from), probe(&moved.to)) {
                        (Probe::Unknown, _) => unreachable(&moved.from),
                        // The entry itself under another case, on a volume
                        // that ignores it, isn't another there.
                        (Probe::Is(at_from), to)
                            if at_from != moved.identity && to != Probe::Is(at_from) =>
                        {
                            taken(&moved.from)
                        }
                        _ => what(&moved.to, moved.identity),
                    };
                    or_busy(member, &[&moved.to, &moved.from])
                })
                .collect(),
            Step::Trashed(trashed) => (trashed.iter())
                .map(|trashed| {
                    let path = trashed.path();
                    let member = match (trashed.in_trash(), probe(path)) {
                        (Ok(false), _) => {
                            let message = tr!(
                                "workbench-project-undo-not-in-trash",
                                name = file_name(path)
                            );
                            Member::Gone(Failure::said(message))
                        }
                        (Err(_), _) | (_, Probe::Unknown) => unreachable(path),
                        (Ok(true), Probe::Is(_)) => taken(path),
                        (Ok(true), Probe::Missing) => Member::Ready,
                    };
                    or_busy(member, &[path])
                })
                .collect(),
        }
    }

    /// Moves `made`, under `root`, to the Trash in one background job, each
    /// in turn unless it's no longer what was made; those moved leave the
    /// tree, and are what's reverted the other way. Those that fail stay,
    /// but for those found changed.
    fn trash_made(
        &mut self,
        reverting: Reverting,
        root: PathBuf,
        made: Vec<Made>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries: Vec<(Made, Option<PathBuf>, bool)> = (made.into_iter())
            .map(|made| {
                let shown = self.shown(&made.path, &root);
                let folder = made.path.symlink_metadata().is_ok_and(|meta| meta.is_dir());
                (made, shown, folder)
            })
            .collect();
        let running: Vec<_> = (entries.iter())
            .map(|(made, _, _)| Running::start(made.path.clone()))
            .collect();
        let jobs: Vec<Made> = entries.iter().map(|(made, _, _)| made.clone()).collect();
        let job = cx.background_spawn({
            let running = running.clone();
            let root = root.clone();
            async move {
                let _running = running;
                (jobs.into_iter())
                    .map(|made| match probe(&made.path) {
                        Probe::Is(now) if now == made.identity => {
                            fs_ops::trash_recorded(&root, &[&made.path]).map_err(Untrashed::Failed)
                        }
                        Probe::Missing => Err(Untrashed::Gone),
                        Probe::Is(_) | Probe::Unknown => Err(Untrashed::Changed),
                    })
                    .collect::<Vec<_>>()
            }
        });
        cx.spawn_in(window, async move |panel, cx| {
            let trashed = job.await;
            panel
                .update(cx, |panel, cx| {
                    let mut deleted = Vec::new();
                    let mut done = Vec::new();
                    let mut kept = Vec::new();
                    let mut failures = Vec::new();
                    for ((made, shown, folder), trashed) in entries.into_iter().zip(trashed) {
                        let name = file_name(&made.path);
                        match trashed {
                            Ok(trashed) => {
                                deleted.extend(shown.map(|shown| (shown, folder)));
                                // Only what can be put back is offered.
                                done.extend(
                                    (trashed.into_iter()).filter(|t| t.location().is_some()),
                                );
                            }
                            Err(Untrashed::Gone) => failures.push(Failure::said(tr!(
                                "workbench-project-undo-changed",
                                name = name
                            ))),
                            // Asked about, or waited for, the next time.
                            Err(Untrashed::Changed) => {
                                kept.push(made);
                                failures.push(Failure::said(tr!(
                                    "workbench-project-undo-changed-kept",
                                    name = name
                                )));
                            }
                            Err(Untrashed::Failed(error)) => {
                                kept.push(made);
                                failures.push(Failure::error(
                                    tr!("workbench-project-trash-failed", name = name),
                                    &error,
                                ));
                            }
                        }
                    }
                    if !deleted.is_empty() {
                        let changes: Vec<FsChange> = (deleted.iter())
                            .map(|(shown, _)| FsChange::Path(shown.clone()))
                            .collect();
                        panel.apply_fs_changes(&changes, cx);
                        cx.emit(PanelEvent::Deleted(deleted));
                    }
                    let (done, kept) = (Step::Trashed(done), Step::Made(kept));
                    panel.landed_revert(reverting, &root, done, kept);
                    ops::report_failures(failures, Many::Undo, cx);
                    cx.notify();
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Puts `trashed`, under `root`, back from the Trash, each in turn,
    /// here: they show again, the first selected, and are what's reverted
    /// the other way. Those that fail stay (where the Trash has them now),
    /// but for those no longer in it.
    fn put_back(
        &mut self,
        reverting: Reverting,
        root: PathBuf,
        trashed: Vec<fs_ops::Trashed>,
        cx: &mut Context<Self>,
    ) {
        let mut done = Vec::new();
        let mut kept = Vec::new();
        let mut shown = Vec::new();
        let mut failures = Vec::new();
        for mut trashed in trashed {
            match fs_ops::restore(&root, &mut trashed) {
                Ok(path) => {
                    shown.extend(self.shown(&path, &root));
                    done.push(Made {
                        path,
                        identity: trashed.id(),
                    });
                }
                Err(error) => {
                    // Its folder gone, say, it's still in the Trash.
                    let in_trash = !matches!(trashed.in_trash(), Ok(false));
                    failures.push(restore_failure(trashed.path(), &error, in_trash));
                    if in_trash {
                        kept.push(trashed);
                    }
                }
            }
        }
        let (done, kept) = (Step::Made(done), Step::Trashed(kept));
        self.landed_revert(reverting, &root, done, kept);
        self.made(shown, cx);
        ops::report_failures(failures, Many::Undo, cx);
    }

    /// How many steps there are to undo and to redo.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn history_depth(&self) -> (usize, usize) {
        (self.history.undo.len(), self.history.redo.len())
    }
}

/// Why a made entry wasn't moved to the Trash as it was reverted.
enum Untrashed {
    /// It's gone.
    Gone,
    /// It was replaced since, or can't be reached now.
    Changed,
    Failed(io::Error),
}

/// Why an entry couldn't be put back from the Trash, which it's still
/// `in_trash` or not.
fn restore_failure(path: &Path, error: &io::Error, in_trash: bool) -> Failure {
    let name = file_name(path);
    match error.kind() {
        io::ErrorKind::AlreadyExists => {
            Failure::said(tr!("workbench-project-undo-taken", name = name))
        }
        _ if !in_trash => Failure::said(tr!("workbench-project-undo-not-in-trash", name = name)),
        _ => Failure::error(tr!("workbench-project-undo-failed", name = name), error),
    }
}

/// What's said of entry `name` that can't be reverted while `busy`.
pub(super) fn busy_failure(busy: Busy, name: &str) -> Failure {
    let message = match busy {
        Busy::Saving => tr!("workbench-project-undo-busy-saving", name = name),
        Busy::Opening => tr!("workbench-project-undo-busy-opening", name = name),
        Busy::Working => tr!("workbench-project-undo-busy-working", name = name),
    };
    Failure::busy(message)
}

#[cfg(test)]
mod history_tests {
    use std::path::{Path, PathBuf};

    use super::{History, Made, Reverting, Step, Way};

    /// A step that made the entry at `name`, its identity standing for it.
    fn made(name: &str, id: u64) -> Step {
        Step::Made(vec![Made {
            path: PathBuf::from(name),
            identity: (0, id),
        }])
    }

    /// The identities of each stack's steps, bottom first.
    fn ids(history: &History) -> (Vec<u64>, Vec<u64>) {
        let id = |step: &Step| match step {
            Step::Made(made) => made[0].identity.1,
            _ => unreachable!(),
        };
        (
            history.undo.iter().map(|entry| id(&entry.step)).collect(),
            history.redo.iter().map(|entry| id(&entry.step)).collect(),
        )
    }

    /// Pops the last step `way` and starts reverting it.
    fn start(history: &mut History, way: Way) -> Reverting {
        history.pop(way).unwrap();
        history.reverting = true;
        Reverting {
            way,
            generation: history.generation,
            pushed: history.pushed,
        }
    }

    fn root() -> PathBuf {
        Path::new("/root").to_owned()
    }

    #[test]
    fn an_operation_that_changed_nothing_keeps_the_redo_stack() {
        let mut history = History::default();
        history.landed_new(root(), made("a", 1), true);
        let undo = start(&mut history, Way::Undo);
        history.landed_revert(undo, root(), made("a", 2), Step::Made(Vec::new()));
        assert_eq!(ids(&history), (vec![], vec![2]));

        // A paste or Trash that failed for every entry.
        history.landed_new(root(), Step::Made(Vec::new()), false);
        assert_eq!(ids(&history), (vec![], vec![2]));
        // One that changed something, even if it can't be undone.
        history.landed_new(root(), Step::Trashed(Vec::new()), true);
        assert_eq!(ids(&history), (vec![], vec![]));
    }

    #[test]
    fn a_new_operation_during_an_undo_drops_its_redo_and_stays_on_top() {
        let mut history = History::default();
        history.landed_new(root(), made("a", 1), true);
        history.landed_new(root(), made("b", 2), true);
        let undo = start(&mut history, Way::Undo);
        // While it waits (for language servers, or the Trash in the
        // background), a new operation lands.
        history.landed_new(root(), made("c", 3), true);
        // Part of it went back (4), part failed and is kept (2).
        history.landed_revert(undo, root(), made("b", 4), made("b", 2));

        assert!(!history.reverting);
        // Kept below the newer operation; nothing to redo.
        assert_eq!(ids(&history), (vec![1, 2, 3], vec![]));
    }

    #[test]
    fn a_redo_lands_on_top_and_keeps_its_rest_unless_redo_was_emptied() {
        let mut history = History::default();
        history.landed_new(root(), made("a", 1), true);
        let undo = start(&mut history, Way::Undo);
        history.landed_revert(undo, root(), made("a", 2), Step::Made(Vec::new()));
        history.landed_new(root(), made("b", 3), false);

        let redo = start(&mut history, Way::Redo);
        history.landed_revert(redo, root(), made("a", 5), made("a", 6));
        assert_eq!(ids(&history), (vec![3, 5], vec![6]));

        let redo = start(&mut history, Way::Redo);
        history.landed_new(root(), made("c", 7), true);
        history.landed_revert(redo, root(), made("a", 8), made("a", 9));
        // The rest of a redo whose stack was emptied is dropped.
        assert_eq!(ids(&history), (vec![3, 5, 7, 8], vec![]));
    }

    #[test]
    fn partial_reverts_keep_the_redo_stack_bounded() {
        let mut history = History::default();
        history.landed_new(root(), made("a", 0), true);
        for id in 1..30 {
            // Part of it reverted, the rest kept to undo again.
            let undo = start(&mut history, Way::Undo);
            history.landed_revert(undo, root(), made("a", 100 + id), made("a", id));
        }
        let (undo, redo) = ids(&history);
        assert_eq!(undo, [29]);
        assert_eq!(redo, (110..130).collect::<Vec<_>>());
    }

    #[test]
    fn only_the_last_steps_are_kept() {
        let mut history = History::default();
        for id in 0..30 {
            history.landed_new(root(), made("a", id), true);
        }
        let (undo, _) = ids(&history);
        assert_eq!(undo, (10..30).collect::<Vec<_>>());
    }
}
