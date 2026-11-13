//! The project panel's cut, copy and paste of files and folders, as
//! nvim-tree's `x`, `c` and `p` and VS Code's explorer's cmd/ctrl-x, -c and
//! -v do.
//!
//! The panel keeps the entries to paste, cut or copied, the selected ones
//! (less those inside another, which go with it), by their paths from the
//! root, so they outlive a refresh and the tree being opened again; another
//! cut or copy replaces them, and each is dropped once the tree finds its
//! path gone. A cut row is drawn dimmed until it's pasted.
//!
//! A paste goes in the selected folder, or beside the selected file, or in
//! the root with no row selected; a copied folder pasted on its own row
//! goes beside it.
//! Each entry keeps its name there, or, should that be taken, VS Code's
//! ` copy` (then ` copy 2`, …) one: it never replaces anything. Nothing is
//! pasted into the entry itself or a folder inside it: a toast says why,
//! and the other entries are pasted. A cut pasted where it is stays cut,
//! and nothing moves.
//!
//! Several entries are pasted as one is, each in turn: copies in one
//! background job, moves on this thread once language servers have
//! answered for them all, asked in one request. The names moves take are
//! looked up before the servers are asked, each passing over those taken
//! by the others, so two cut entries of one name from different folders
//! take it and its ` copy` one. The servers' edits, written for all the
//! moves together, are made only if every one was. What failed is named in
//! one toast, with why ([`ops::report_failures`]); what was pasted shows,
//! the first selected.
//!
//! A copy is made off the main thread ([`fs_ops::copy_into`]), selected and
//! announced as a duplicate is; it can be pasted again. A cut is moved much
//! as a rename is: on this thread, refused while something acts on its old
//! or new path ([`Workbench::busy_under`]), with its editors taking the new
//! path ([`PanelEvent::Renamed`]). Language servers whose `willRename`
//! filters match are asked first, but their edits are made after the move,
//! at the moved files' new paths, so a move that fails (its new path taken
//! meanwhile, say) makes none of them. Once it's moved, there's nothing
//! left to paste.
//!
//! A drop of the tree's rows on a folder ([`drag`](super::drag)) is pasted
//! as they would be ([`ProjectPanel::drop_into`]), but for a move's name:
//! one taken in the folder isn't passed over, and the move isn't made.

use kiln_l10n::tr;
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

use fe_lsp::{EditTarget, FileEdits};
use fe_project::{EntryKind, FsChange, NodeId, fs_ops};
use gpui::{App, AppContext as _, Context, Window};

use super::{
    PanelEvent, ProjectPanel,
    ops::{self, Running},
    undo::{self, Made, Reverting, Step},
};
use crate::{
    Workbench,
    workbench::{
        Busy,
        toast::{self, Toast},
    },
};

/// What the entry to paste was taken by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipKind {
    Cut,
    Copy,
}

/// The entries to paste: what took them, their paths from the root, in the
/// list's order, and which cut or copy of [`Clipboard::clips`] it was, so
/// it's never mistaken for another of the same paths.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Clip {
    kind: ClipKind,
    rels: Vec<PathBuf>,
    serial: u64,
}

/// The panel's entries to paste, if any.
#[derive(Default)]
pub(super) struct Clipboard {
    item: Option<Clip>,
    /// Whether a cut's paste waits for language servers' edits: another
    /// paste waits for it.
    moving: bool,
    /// Counts the cuts and copies.
    clips: u64,
    /// Counts what might change whether the entry is there: each cut or
    /// copy, each watcher batch and each listing. A look at the disk counts
    /// only while nothing newer has come.
    seen: u64,
}

/// What a move pastes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MoveOf {
    /// A cut, by its [`Clip::serial`]: an entry moved is no longer to
    /// paste.
    Clip(u64),
    /// Rows dropped on a folder: they keep their names, or aren't moved.
    Drop,
    /// An undo or redo of a move, back to where the entry was.
    Revert(Reverting),
}

impl MoveOf {
    /// Which operation it is, for a toast about several that failed.
    fn many(self) -> ops::Many {
        match self {
            MoveOf::Clip(_) | MoveOf::Drop => ops::Many::Paste,
            MoveOf::Revert(_) => ops::Many::Undo,
        }
    }
}

/// A cut's paste: `from` into folder `dir` as `name` (which needn't be
/// UTF-8), all under the tree's (canonical) `root`, and the panel's
/// spelling of the old and new paths.
struct Move {
    /// What it pastes, and the entry's path from the root.
    of: MoveOf,
    rel: PathBuf,
    /// The entry's device and inode when it was asked about
    /// ([`ops::entry_identity`]).
    identity: Option<(u64, u64)>,
    root: PathBuf,
    from: PathBuf,
    dir: PathBuf,
    name: OsString,
    shown_from: PathBuf,
    shown_to: PathBuf,
}

/// A move not made: why, and for an undo or redo, the step's entry to
/// revert again (none for one found changed).
struct NotMoved {
    failure: ops::Failure,
    kept: Option<undo::Moved>,
}

/// A cut entry moved: from and to, as the panel hands paths out, and its
/// old path with its folders resolved ([`by_folder`]), as servers name it.
struct Moved {
    shown_from: PathBuf,
    shown_to: PathBuf,
    from_resolved: Option<PathBuf>,
    /// As the history keeps it: from and to under the tree's root, and
    /// what it is.
    step: undo::Moved,
}

impl Moved {
    /// `edits`, language servers', made to follow this move ([`moved_edits`]).
    fn edits(&self, edits: Vec<FileEdits>) -> Vec<FileEdits> {
        let from_resolved = self.from_resolved.as_deref();
        moved_edits(edits, &self.shown_from, &self.shown_to, from_resolved)
    }
}

impl Move {
    /// The entry's name, for messages.
    fn entry_name(&self) -> String {
        file_name(&self.from)
    }
}

impl ProjectPanel {
    /// `x` and `c`: takes the selected entries to paste, cut or copied, in
    /// place of those before.
    pub(super) fn clip(&mut self, kind: ClipKind, cx: &mut Context<Self>) {
        let Some((root, entries)) = self.selected_entries() else {
            return;
        };
        let rels: Vec<PathBuf> = (ops::topmost(entries).into_iter())
            .filter_map(|(path, _)| Some(path.strip_prefix(&root).ok()?.to_owned()))
            .collect();
        if rels.is_empty() {
            return;
        }
        let clipboard = &mut self.clipboard;
        clipboard.clips += 1;
        clipboard.seen += 1;
        clipboard.item = Some(Clip {
            kind,
            rels,
            serial: clipboard.clips,
        });
        cx.notify();
    }

    /// `p`: pastes the entries cut or copied, in the selected folder or
    /// beside the selected file ([`Self::paste_folder`]).
    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip) = self.clipboard.item.clone() else {
            return;
        };
        if self.clipboard.moving {
            return;
        }
        let Some(tree) = self.tree() else {
            return;
        };
        let root = tree.root_path().to_owned();
        let sources: Vec<PathBuf> = clip.rels.iter().map(|rel| root.join(rel)).collect();
        // A copy pasted on its own folder's row goes beside it; a cut
        // would go into itself.
        let beside: &[PathBuf] = match clip.kind {
            ClipKind::Copy => &sources,
            ClipKind::Cut => &[],
        };
        let Some(dir) = self.paste_folder(beside) else {
            return;
        };
        let (into_itself, sources): (Vec<PathBuf>, Vec<PathBuf>) =
            (sources.into_iter()).partition(|source| dir.starts_with(source));
        for source in into_itself {
            let name = file_name(&source);
            let message = match clip.kind {
                ClipKind::Cut => tr!("workbench-project-paste-into-itself-cut", name = name),
                ClipKind::Copy => tr!("workbench-project-paste-into-itself-copy", name = name),
            };
            toast::show(cx, Toast::error(message));
        }
        let several = clip.rels.len() > 1;
        let of = MoveOf::Clip(clip.serial);
        self.paste_into(clip.kind, of, sources, several, dir, root, window, cx);
    }

    /// Pastes `sources`, cut or copied as `kind` says (a cut's by `of`),
    /// into `dir`, all under `root`: copies in the background, moves once
    /// language servers have answered, as one entry is unless `several`
    /// were taken. A cut entry already in `dir` isn't moved.
    #[allow(clippy::too_many_arguments)]
    fn paste_into(
        &mut self,
        kind: ClipKind,
        of: MoveOf,
        sources: Vec<PathBuf>,
        several: bool,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ClipKind::Copy if several => self.paste_copies(sources, dir, root, window, cx),
            ClipKind::Copy => {
                if let Some(source) = sources.into_iter().next() {
                    self.paste_copy(source, dir, root, window, cx);
                }
            }
            ClipKind::Cut => {
                // Where they are already: nothing to move.
                let sources: Vec<PathBuf> = (sources.into_iter())
                    .filter(|source| source.parent() != Some(&dir))
                    .collect();
                match sources.as_slice() {
                    [] => {}
                    [source] if !several => {
                        let Ok(rel) = source.strip_prefix(&root).map(Path::to_owned) else {
                            return;
                        };
                        self.paste_cut(of, rel, source.clone(), dir, root, window, cx)
                    }
                    _ => self.paste_cuts(of, sources, dir, root, window, cx),
                }
            }
        }
    }

    /// Rows dropped on folder `dir`, all under `root`: `sources`, moved or
    /// copied as `kind` says, as a paste of them would be
    /// ([`Self::paste_into`]), except that a move keeps its entry's name or
    /// isn't made. Nothing is dropped while a cut's paste or another
    /// drop's move waits for language servers.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn drop_into(
        &mut self,
        kind: ClipKind,
        sources: Vec<PathBuf>,
        several: bool,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.clipboard.moving {
            return;
        }
        self.paste_into(kind, MoveOf::Drop, sources, several, dir, root, window, cx);
    }

    /// The folder a paste goes in: the selected one, unless that's one of
    /// `beside`, or the selected entry's folder, or the root with no row
    /// selected.
    fn paste_folder(&self, beside: &[PathBuf]) -> Option<PathBuf> {
        let tree = self.tree()?;
        let selected = self.selected_index().and(self.selected);
        let Some(id) = selected else {
            return Some(tree.root_path().to_owned());
        };
        let path = tree.path(id)?;
        match tree.row(id)?.kind {
            EntryKind::Dir if !beside.contains(&path) => Some(path),
            // A symlink to a folder too: a paste never goes through one.
            _ => path.parent().map(Path::to_owned),
        }
    }

    /// Copies `source` into `dir`, both under `root`, in the background,
    /// then selects the copy.
    fn paste_copy(
        &mut self,
        source: PathBuf,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = file_name(&source);
        // A rename of either waits for the copy.
        let running = [Running::start(dir.clone()), Running::start(source.clone())];
        let job = cx.background_spawn({
            let running = running.clone();
            let root = root.clone();
            async move {
                let _running = running;
                fs_ops::copy_into_recorded(&root, &source, &dir)
            }
        });
        cx.spawn_in(window, async move |panel, cx| {
            let copied = job.await;
            panel
                .update(cx, |panel, cx| match copied {
                    Ok(copy) => {
                        let shown = panel.shown(&copy.path, &root);
                        panel.landed_new(&root, Step::Made(vec![Made::from(copy)]), true);
                        if let Some(copy) = shown {
                            panel.changed(&copy, cx);
                            let folder = copy.symlink_metadata().is_ok_and(|meta| meta.is_dir());
                            cx.emit(PanelEvent::Created(vec![(copy.clone(), folder)]));
                            panel.reveal(copy, cx);
                        }
                    }
                    Err(error) => {
                        let message = tr!("workbench-project-paste-failed", name = &name);
                        ops::Failure::error(message, &error).show(cx)
                    }
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Copies each of `sources` into `dir`, all under `root`, in turn, in
    /// one background job, then shows the copies, the first selected, and
    /// names those that failed.
    fn paste_copies(
        &mut self,
        sources: Vec<PathBuf>,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            return;
        }
        // A rename of the folder or of any of them waits for the copies.
        let running: Vec<_> = (std::iter::once(&dir).chain(&sources))
            .map(|path| Running::start(path.clone()))
            .collect();
        let job = cx.background_spawn({
            let running = running.clone();
            let root = root.clone();
            let sources = sources.clone();
            async move {
                let _running = running;
                (sources.iter())
                    .map(|source| fs_ops::copy_into_recorded(&root, source, &dir))
                    .collect::<Vec<_>>()
            }
        });
        cx.spawn_in(window, async move |panel, cx| {
            let copied = job.await;
            panel
                .update(cx, |panel, cx| {
                    let mut made = Vec::new();
                    let mut copies = Vec::new();
                    let mut failures = Vec::new();
                    for (source, copied) in sources.iter().zip(copied) {
                        match copied {
                            Ok(copy) => {
                                made.extend(panel.shown(&copy.path, &root));
                                copies.push(Made::from(copy));
                            }
                            Err(error) => failures.push(ops::Failure::error(
                                tr!("workbench-project-paste-failed", name = file_name(source)),
                                &error,
                            )),
                        }
                    }
                    let changed = !copies.is_empty();
                    panel.landed_new(&root, Step::Made(copies), changed);
                    panel.made(made, cx);
                    ops::report_failures(failures, ops::Many::Paste, cx);
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// The move of `source`, at `rel` from the root, for `of`, into `dir`,
    /// all under `root`, under the name it would take there now passing
    /// over those `planned` for others of its paste, or why it can't be. A
    /// drop's move keeps its name: if that's taken, it can't be.
    fn plan_move(
        &self,
        of: MoveOf,
        rel: PathBuf,
        source: PathBuf,
        dir: PathBuf,
        root: PathBuf,
        planned: &[OsString],
    ) -> Result<Option<Move>, ops::Failure> {
        let name = fs_ops::paste_name_besides(&root, &source, &dir, planned).map_err(|error| {
            let message = tr!("workbench-project-move-failed", name = file_name(&source));
            ops::Failure::error(message, &error)
        })?;
        if of == MoveOf::Drop && Some(name.as_os_str()) != source.file_name() {
            let message = tr!(
                "workbench-project-drop-name-taken",
                name = file_name(&source),
                folder = file_name(&dir)
            );
            return Err(ops::Failure::said(message));
        }
        let shown = (self.shown(&source, &root)).zip(self.shown(&dir.join(&name), &root));
        let Some((shown_from, shown_to)) = shown else {
            return Ok(None);
        };
        Ok(Some(Move {
            of,
            rel,
            identity: ops::entry_identity(&source),
            root,
            from: source,
            dir,
            name,
            shown_from,
            shown_to,
        }))
    }

    /// Moves `source` into `dir`, both under `root`, as a rename is made
    /// ([`Self::move_now`]), once language servers whose `willRename`
    /// filters match have answered with their edits. The name it takes
    /// there is looked up first, for them to be asked about.
    #[allow(clippy::too_many_arguments)]
    fn paste_cut(
        &mut self,
        of: MoveOf,
        rel: PathBuf,
        source: PathBuf,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moving = match self.plan_move(of, rel, source, dir, root, &[]) {
            Ok(Some(moving)) => moving,
            Ok(None) => return,
            Err(failure) => return failure.show(cx),
        };
        if let Some(busy) = self.move_busy(&moving, window, cx) {
            return busy.show(cx);
        }
        let asked = window.root::<Workbench>().flatten().map(|workbench| {
            (workbench.read(cx)).will_rename(&moving.shown_from, &moving.shown_to, cx)
        });
        let Some(mut asked) = asked else {
            return self.move_now(moving, Vec::new(), window, cx);
        };
        // Ready at once when no server's filters match.
        if let Some(edits) = ops::ready_now(&mut asked) {
            return self.move_now(moving, edits, window, cx);
        }
        self.clipboard.moving = true;
        cx.spawn_in(window, async move |panel, cx| {
            let edits = asked.await;
            panel
                .update_in(cx, |panel, window, cx| {
                    panel.clipboard.moving = false;
                    panel.move_now(moving, edits, window, cx);
                })
                .ok();
        })
        .detach();
    }

    /// Moves each of `sources` into `dir`, all under `root`, for `of`, as
    /// one is moved ([`Self::paste_cut`]): each takes a name
    /// no other of them does, the language servers are asked about them
    /// all in one request, and once they've answered each is moved in turn
    /// ([`Self::move_each`]). What can't be moved is named in one toast,
    /// with why.
    fn paste_cuts(
        &mut self,
        of: MoveOf,
        sources: Vec<PathBuf>,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut failures = Vec::new();
        let mut moves = Vec::new();
        let mut planned = Vec::new();
        for source in sources {
            let Ok(rel) = source.strip_prefix(&root).map(Path::to_owned) else {
                continue;
            };
            let dir = dir.clone();
            match self.plan_move(of, rel, source, dir, root.clone(), &planned) {
                Ok(Some(moving)) => match self.move_busy(&moving, window, cx) {
                    Some(busy) => failures.push(busy),
                    None => {
                        planned.push(moving.name.clone());
                        moves.push(moving);
                    }
                },
                Ok(None) => {}
                Err(failure) => failures.push(failure),
            }
        }
        self.ask_and_move(moves, failures, window, cx);
    }

    /// Moves each of `moves` in turn ([`Self::move_each`]) once language
    /// servers whose `willRename` filters match have answered for them all,
    /// asked in one request. `failures`, from before, are named with those
    /// of the moves.
    fn ask_and_move(
        &mut self,
        moves: Vec<Move>,
        failures: Vec<ops::Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if moves.is_empty() {
            return ops::report_failures(failures, ops::Many::Paste, cx);
        }
        let renames: Vec<(PathBuf, PathBuf)> = (moves.iter())
            .map(|moving| (moving.shown_from.clone(), moving.shown_to.clone()))
            .collect();
        let asked = (window.root::<Workbench>().flatten())
            .map(|workbench| workbench.read(cx).will_rename_many(&renames, cx));
        let Some(mut asked) = asked else {
            return self.move_each(moves, Vec::new(), failures, window, cx);
        };
        // Ready at once when no server's filters match.
        if let Some(edits) = ops::ready_now(&mut asked) {
            return self.move_each(moves, edits, failures, window, cx);
        }
        self.clipboard.moving = true;
        cx.spawn_in(window, async move |panel, cx| {
            let edits = asked.await;
            panel
                .update_in(cx, |panel, window, cx| {
                    panel.clipboard.moving = false;
                    panel.move_each(moves, edits, failures, window, cx);
                })
                .ok();
        })
        .detach();
    }

    /// Moves each of `moves` in turn, as one is moved ([`Self::move_one`]),
    /// then makes `edits`, the language servers' for them all, at the moved
    /// files' new paths: only if every one was moved, since they were
    /// written for all the moves together. Then it selects the first moved
    /// and names those that failed, with `failures` from before.
    fn move_each(
        &mut self,
        moves: Vec<Move>,
        edits: Vec<FileEdits>,
        mut failures: Vec<ops::Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = moves.len();
        let Some(first) = moves.first() else {
            return;
        };
        let (of, root) = (first.of, first.root.clone());
        let mut moved = Vec::new();
        let mut kept = Vec::new();
        for moving in moves {
            match self.move_one(moving, window, cx) {
                Ok(done) => moved.push(done),
                Err(not) => {
                    failures.push(not.failure);
                    kept.extend(not.kept);
                }
            }
        }
        self.land_moves(of, &root, &moved, kept);
        if moved.len() == asked {
            let edits = (moved.iter()).fold(edits, |edits, done| done.edits(edits));
            // Once the editors have taken the new paths, which the moves'
            // events have them do.
            self.apply_edits_later(edits, window, cx);
        } else if !edits.is_empty() {
            log::warn!(
                "not every entry was moved: the language servers' edits for them aren't made"
            );
        }
        if let Some(first) = moved.into_iter().next() {
            self.reveal(first.shown_to, cx);
        }
        ops::report_failures(failures, of.many(), cx);
    }

    /// Why `moving` must wait, if it must: something acts on its old path
    /// or on its new one, in the folder it goes in ([`Workbench::busy_under`]:
    /// a save, a file being read, or another of the panel's operations, such
    /// as that folder being moved to the Trash).
    fn move_busy(&self, moving: &Move, window: &Window, cx: &App) -> Option<ops::Failure> {
        let (busy, name) = match Workbench::busy_under(&moving.shown_from, window, cx) {
            Some(busy) => (busy, moving.entry_name()),
            None => (
                Workbench::busy_under(&moving.shown_to, window, cx)?,
                file_name(&moving.dir),
            ),
        };
        if let MoveOf::Revert(_) = moving.of {
            return Some(undo::busy_failure(busy, &name));
        }
        let message = match busy {
            Busy::Saving => tr!("workbench-project-paste-busy-saving", name = &name),
            Busy::Opening => tr!("workbench-project-paste-busy-opening", name = &name),
            Busy::Working => tr!("workbench-project-paste-busy-working", name = &name),
        };
        Some(ops::Failure::busy(message))
    }

    /// Moves `moving`'s entry on this thread, then makes `edits`, language
    /// servers' for the move, with its editors taking the new path in the
    /// same turn; then brings the tree up to date and selects it. A folder
    /// expanded before is after.
    ///
    /// Everything is checked again first, as the servers may have taken a
    /// while: nothing is moved if something acts on either path now
    /// ([`Self::move_busy`]), if the entry was moved or replaced since
    /// they were asked about it, or if its new path is taken: a toast says
    /// why. The move is made before the edits, so one that fails makes none
    /// of them.
    fn move_now(
        &mut self,
        moving: Move,
        edits: Vec<FileEdits>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (of, root) = (moving.of, moving.root.clone());
        match self.move_one(moving, window, cx) {
            Ok(moved) => {
                self.land_moves(of, &root, std::slice::from_ref(&moved), Vec::new());
                let edits = moved.edits(edits);
                // Once the editors have taken the new paths, which the
                // event has them do: an open file in a moved folder is
                // found by its new one.
                self.apply_edits_later(edits, window, cx);
                self.reveal(moved.shown_to, cx);
            }
            Err(not) => {
                self.land_moves(of, &root, &[], not.kept.into_iter().collect());
                not.failure.show(cx);
            }
        }
    }

    /// Takes in what moves for `of`, under `root`, did: those `moved`, and
    /// for an undo or redo, the step's entries `kept` to revert again.
    fn land_moves(&mut self, of: MoveOf, root: &Path, moved: &[Moved], kept: Vec<undo::Moved>) {
        let done = Step::Moved(moved.iter().map(|done| done.step.clone()).collect());
        match of {
            MoveOf::Revert(reverting) => {
                self.landed_revert(reverting, root, done, Step::Moved(kept));
            }
            MoveOf::Clip(_) | MoveOf::Drop => self.landed_new(root, done, !moved.is_empty()),
        }
    }

    /// Makes `edits`, language servers' for moves, once the editors have
    /// taken the new paths. A file they replace is asked about before an
    /// undo or redo moves it again, as any replaced since is.
    fn apply_edits_later(
        &mut self,
        edits: Vec<FileEdits>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.defer(cx, move |window, cx| ops::apply_edits(edits, window, cx));
    }

    /// [`Self::move_now`]'s move, short of making the language servers'
    /// edits and selecting the entry: where it went, or why it wasn't
    /// moved.
    fn move_one(
        &mut self,
        moving: Move,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Moved, NotMoved> {
        // An undo's or redo's entry, to revert again.
        let kept = match moving.of {
            MoveOf::Revert(_) => moving.identity.map(|identity| undo::Moved {
                from: moving.dir.join(&moving.name),
                to: moving.from.clone(),
                identity,
            }),
            MoveOf::Clip(_) | MoveOf::Drop => None,
        };
        if let Some(busy) = self.move_busy(&moving, window, cx) {
            return Err(NotMoved {
                failure: busy,
                kept,
            });
        }
        let entry = moving.entry_name();
        let found = undo::probe(&moving.from);
        let identity = match moving.identity {
            Some(identity) if found == undo::Probe::Is(identity) => identity,
            _ => {
                let (message, kept) = match moving.of {
                    // Gone: never reverted. Replaced (a save that finished
                    // while the servers were asked, say) or out of reach:
                    // kept, to be asked about or waited for next time.
                    MoveOf::Revert(_) if found == undo::Probe::Missing => {
                        (tr!("workbench-project-undo-changed", name = &entry), None)
                    }
                    MoveOf::Revert(_) => (
                        tr!("workbench-project-undo-changed-kept", name = &entry),
                        kept,
                    ),
                    MoveOf::Clip(_) | MoveOf::Drop => {
                        (tr!("workbench-project-paste-raced", name = &entry), None)
                    }
                };
                return Err(NotMoved {
                    failure: ops::Failure::said(message),
                    kept,
                });
            }
        };
        // Planned while every spelling of the old path still resolves.
        let moves = Workbench::plan_rename(&moving.shown_from, &moving.shown_to, window, cx);
        // By its folder: a symlink moved is the link, not what it leads to.
        let from_resolved = by_folder(&moving.shown_from);
        let expanded = self.tree().is_some_and(|tree| {
            (tree.find(&moving.from))
                .and_then(|id| tree.row(id))
                .is_some_and(|row| row.kind == EntryKind::Dir && row.expanded)
        });
        // Back to another name in its folder, as an undone rename is: as
        // a rename, which also changes only its case where the volume
        // ignores case.
        let renamed = (moving.from.parent() == Some(moving.dir.as_path()))
            .then(|| moving.name.to_str())
            .flatten();
        let moved = match renamed {
            Some(name) => fs_ops::rename(&moving.root, &moving.from, name),
            None => fs_ops::move_into(&moving.root, &moving.from, &moving.dir, &moving.name),
        };
        let path = match moved {
            Ok(path) => path,
            Err(error) => {
                let message = tr!("workbench-project-move-failed", name = &entry);
                return Err(NotMoved {
                    failure: ops::Failure::error(message, &error),
                    kept,
                });
            }
        };
        let step = undo::Moved {
            from: moving.from.clone(),
            to: path.clone(),
            identity,
        };
        let Move {
            of,
            rel,
            shown_from,
            shown_to,
            ..
        } = moving;
        cx.emit(PanelEvent::Renamed {
            from: shown_from.clone(),
            to: shown_to.clone(),
            moves,
        });
        // Once moved, it's no longer to paste.
        let item = (self.clipboard.item.as_mut()).filter(|item| of == MoveOf::Clip(item.serial));
        if let Some(item) = item {
            item.rels.retain(|kept| *kept != rel);
            if item.rels.is_empty() {
                self.clipboard.item = None;
            }
        }
        self.apply_fs_changes(
            &[
                FsChange::Path(shown_from.clone()),
                FsChange::Path(shown_to.clone()),
            ],
            cx,
        );
        if let Some(id) = (self.tree().and_then(|tree| tree.find(&path))).filter(|_| expanded) {
            self.expand(id, cx);
        }
        Ok(Moved {
            shown_from,
            shown_to,
            from_resolved,
            step,
        })
    }

    /// Moves each of `moved`, all under `root`, back from where it was
    /// moved to, as `reverting` reverts a move: as a cut's paste moves them
    /// ([`Self::ask_and_move`]), each to its old name.
    pub(super) fn move_back(
        &mut self,
        reverting: Reverting,
        root: PathBuf,
        moved: Vec<undo::Moved>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moves: Vec<Move> = (moved.into_iter())
            .filter_map(|moved| {
                let undo::Moved {
                    from: back,
                    to: from,
                    identity,
                } = moved;
                Some(Move {
                    of: MoveOf::Revert(reverting),
                    rel: from.strip_prefix(&root).ok()?.to_owned(),
                    identity: Some(identity),
                    shown_from: self.shown(&from, &root)?,
                    shown_to: self.shown(&back, &root)?,
                    dir: back.parent()?.to_owned(),
                    name: back.file_name()?.to_owned(),
                    root: root.clone(),
                    from,
                })
            })
            .collect();
        if moves.is_empty() {
            let none = || Step::Moved(Vec::new());
            return self.landed_revert(reverting, &root, none(), none());
        }
        self.ask_and_move(moves, Vec::new(), window, cx);
    }

    /// Whether a cut's paste, a drop's move or an undone move waits for
    /// language servers' edits.
    pub(super) fn clipboard_moving(&self) -> bool {
        self.clipboard.moving
    }

    /// Whether a paste now, on the selected row (or the root with none),
    /// would do anything: there's an entry to paste, no cut's paste waits,
    /// and a cut entry wouldn't all land where they are. For the tree's
    /// menu, which leaves out Paste otherwise.
    pub(super) fn can_paste(&self) -> bool {
        let Some(clip) = &self.clipboard.item else {
            return false;
        };
        if self.clipboard.moving {
            return false;
        }
        let Some(tree) = self.tree() else {
            return false;
        };
        match clip.kind {
            ClipKind::Copy => true,
            ClipKind::Cut => {
                let Some(dir) = self.paste_folder(&[]) else {
                    return false;
                };
                (clip.rels.iter()).any(|rel| tree.root_path().join(rel).parent() != Some(&dir))
            }
        }
    }

    /// The cut entries' nodes, those loaded, for their rows to be dimmed.
    pub(super) fn cut_nodes(&self) -> HashSet<NodeId> {
        let clip = (self.clipboard.item.as_ref()).filter(|clip| clip.kind == ClipKind::Cut);
        let (Some(clip), Some(tree)) = (clip, self.tree()) else {
            return HashSet::new();
        };
        (clip.rels.iter())
            .filter_map(|rel| tree.find(tree.root_path().join(rel)))
            .collect()
    }

    /// The tree is opened anew: a look at the disk that began under the old
    /// one no longer counts, and the new one reconciles the entry once it's
    /// listed ([`Self::drop_gone_clip`]).
    pub(super) fn clip_reopened(&mut self) {
        self.clipboard.seen += 1;
    }

    /// Drops each entry to paste once it's gone. When the tree may say so
    /// (the nearest folder above it that's loaded is listed, and up to
    /// date, without it), the disk is asked, off this thread: a tree that
    /// hides ignored entries doesn't list one that's there. The answer
    /// counts only if nothing newer was seen meanwhile ([`Self::clip_gone`]).
    pub(super) fn drop_gone_clip(&mut self, cx: &mut Context<Self>) {
        if self.clipboard.item.is_none() {
            return;
        }
        self.clipboard.seen += 1;
        let (Some(clip), Some(tree)) = (&self.clipboard.item, self.tree()) else {
            return;
        };
        let root = tree.root_path().to_owned();
        let look: Vec<PathBuf> = (clip.rels.iter())
            .filter(|rel| {
                let path = root.join(rel);
                if tree.find(&path).is_some() {
                    return false;
                }
                let nearest = path.ancestors().skip(1).find_map(|dir| tree.find(dir));
                nearest.is_some_and(|dir| tree.listing(dir).is_none())
            })
            .cloned()
            .collect();
        if look.is_empty() {
            return;
        }
        let seen = self.clipboard.seen;
        let gone = cx.background_spawn(async move {
            (look.into_iter())
                .filter(|rel| fs_ops::exists(&root, &root.join(rel)).is_ok_and(|exists| !exists))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |panel, cx| {
            let gone = gone.await;
            panel
                .update(cx, |panel, cx| panel.clip_gone(seen, gone, cx))
                .ok();
        })
        .detach();
    }

    /// Takes in a look at the disk for the entries to paste, started when
    /// [`Clipboard::seen`] was `seen`, which found `gone` gone (each a path
    /// from the root): unless something has been seen since (another cut
    /// or copy, a watcher batch, a listing), they're dropped. An answer
    /// that can't tell keeps an entry.
    fn clip_gone(&mut self, seen: u64, gone: Vec<PathBuf>, cx: &mut Context<Self>) {
        if seen != self.clipboard.seen || gone.is_empty() {
            return;
        }
        let Some(item) = self.clipboard.item.as_mut() else {
            return;
        };
        item.rels.retain(|rel| !gone.contains(rel));
        if item.rels.is_empty() {
            self.clipboard.item = None;
        }
        cx.notify();
    }
}

/// `edits`, language servers' for moving `from` to `to`, to make once it's
/// moved: every path they name under `from`, as the panel spells it or with
/// its folders resolved (`from_resolved`, taken before the move, as
/// [`by_folder`] resolves it) as servers name it, is
/// under `to` now. That's each entry's path, text edits' and file
/// operations' alike, and a rename's destination, so the edit's operations
/// and the text edits after them still name the same files, in the same
/// order; text edits that depend on an operation follow its entry's path.
fn moved_edits(
    mut edits: Vec<FileEdits>,
    from: &Path,
    to: &Path,
    from_resolved: Option<&Path>,
) -> Vec<FileEdits> {
    let to_resolved = by_folder(to);
    let moved = |path: &mut PathBuf| {
        let under = |from: &Path, to: &Path| {
            let rest = path.strip_prefix(from).ok()?;
            Some(if rest.as_os_str().is_empty() {
                to.to_owned()
            } else {
                to.join(rest)
            })
        };
        let new = under(from, to).or_else(|| {
            let (from, to) = from_resolved.zip(to_resolved.as_deref())?;
            under(from, to)
        });
        if let Some(new) = new {
            *path = new;
        }
    };
    for file in &mut edits {
        moved(&mut file.path);
        if let EditTarget::Rename { to, .. } = &mut file.target {
            moved(to);
        }
    }
    edits
}

/// `path` with its folder resolved and its own name kept, so a symlink is
/// the link, never what it leads to; `None` if its folder doesn't resolve.
fn by_folder(path: &Path) -> Option<PathBuf> {
    let (folder, name) = (path.parent()?, path.file_name()?);
    Some(dunce::canonicalize(folder).ok()?.join(name))
}

/// `path`'s last component, for messages.
pub(super) fn file_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    name.to_string_lossy().into_owned()
}

#[cfg(test)]
impl ProjectPanel {
    /// The entry to paste, whether it was cut, and its path from the root:
    /// the first, if there are several.
    pub(crate) fn clipboard_entry(&self) -> Option<(bool, PathBuf)> {
        let clip = self.clipboard.item.as_ref()?;
        Some((clip.kind == ClipKind::Cut, clip.rels.first()?.clone()))
    }

    /// The entries to paste, whether they were cut, and their paths from
    /// the root.
    pub(crate) fn clipboard_entries(&self) -> Option<(bool, Vec<PathBuf>)> {
        let clip = self.clipboard.item.as_ref()?;
        Some((clip.kind == ClipKind::Cut, clip.rels.clone()))
    }

    /// A look at the disk started when [`Clipboard::seen`] was `seen`, as
    /// [`Self::clip_gone`] takes it in: every entry to paste is gone if
    /// `exists` says so.
    pub(crate) fn clip_looked(
        &mut self,
        seen: u64,
        exists: std::io::Result<bool>,
        cx: &mut Context<Self>,
    ) {
        if exists.is_ok_and(|exists| !exists) {
            let gone =
                (self.clipboard.item.as_ref()).map_or_else(Vec::new, |item| item.rels.clone());
            self.clip_gone(seen, gone, cx);
        }
    }

    /// What a look at the disk for the entry to paste started now would
    /// take in ([`Self::clip_looked`]).
    pub(crate) fn clip_seen(&self) -> u64 {
        self.clipboard.seen
    }
}
