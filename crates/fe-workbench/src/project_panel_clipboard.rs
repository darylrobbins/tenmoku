//! The project panel's cut, copy and paste of files and folders, as
//! nvim-tree's `x`, `c` and `p` and VS Code's explorer's cmd/ctrl-x, -c and
//! -v do.
//!
//! The panel keeps one entry to paste, cut or copied, by its path from the
//! root, so it outlives a refresh and the tree being opened again; another
//! cut or copy replaces it, and it's dropped once the tree finds its path
//! gone. A cut row is drawn dimmed until it's pasted.
//!
//! A paste goes in the selected folder, or beside the selected file, or in
//! the root with no row selected; a copied folder pasted on its own row
//! goes beside it.
//! It keeps the entry's name there, or, should that be taken, VS Code's
//! ` copy` (then ` copy 2`, …) one: it never replaces anything. Nothing is
//! pasted into the entry itself or a folder inside it: a toast says why. A
//! cut pasted where it is stays cut, and nothing moves.
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

use kiln_l10n::tr;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use fe_lsp::{EditTarget, FileEdits};
use fe_project::{EntryKind, FsChange, NodeId, fs_ops};
use gpui::{AppContext as _, Context, Window};

use super::{
    PanelEvent, ProjectPanel,
    ops::{self, Running},
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

/// The entry to paste: what took it, its path from the root, and which
/// cut or copy of [`Clipboard::clips`] it was, so it's never mistaken for
/// another of the same path.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Clip {
    kind: ClipKind,
    rel: PathBuf,
    serial: u64,
}

/// The panel's entry to paste, if any.
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

/// A cut's paste: `from` into folder `dir` as `name` (which needn't be
/// UTF-8), all under the tree's (canonical) `root`, and the panel's
/// spelling of the old and new paths.
struct Move {
    clip: Clip,
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

impl Move {
    /// The entry's name, for messages.
    fn entry_name(&self) -> String {
        file_name(&self.from)
    }
}

impl ProjectPanel {
    /// `x` and `c`: takes the selected entry to paste, cut or copied, in
    /// place of the one before.
    pub(super) fn clip(&mut self, kind: ClipKind, cx: &mut Context<Self>) {
        let Some(tree) = self.tree() else {
            return;
        };
        let Some(path) = self
            .selected_index()
            .and(self.selected)
            .and_then(|id| tree.path(id))
        else {
            return;
        };
        let Ok(rel) = path.strip_prefix(tree.root_path()) else {
            return;
        };
        let rel = rel.to_owned();
        let clipboard = &mut self.clipboard;
        clipboard.clips += 1;
        clipboard.seen += 1;
        clipboard.item = Some(Clip {
            kind,
            rel,
            serial: clipboard.clips,
        });
        cx.notify();
    }

    /// `p`: pastes the entry cut or copied, in the selected folder or
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
        let source = root.join(&clip.rel);
        // A copy pasted on its own folder's row goes beside it; a cut
        // would go into itself.
        let beside = (clip.kind == ClipKind::Copy).then_some(source.as_path());
        let Some(dir) = self.paste_folder(beside) else {
            return;
        };
        if dir.starts_with(&source) {
            let name = file_name(&source);
            let message = match clip.kind {
                ClipKind::Cut => tr!("workbench-project-paste-into-itself-cut", name = name),
                ClipKind::Copy => tr!("workbench-project-paste-into-itself-copy", name = name),
            };
            return toast::show(cx, Toast::error(message));
        }
        match clip.kind {
            ClipKind::Copy => self.paste_copy(source, dir, root, window, cx),
            // Where it is already: nothing to move.
            ClipKind::Cut if source.parent() == Some(&dir) => {}
            ClipKind::Cut => self.paste_cut(clip, source, dir, root, window, cx),
        }
    }

    /// The folder a paste goes in: the selected one, unless that's
    /// `beside`, or the selected entry's folder, or the root with no row
    /// selected.
    fn paste_folder(&self, beside: Option<&Path>) -> Option<PathBuf> {
        let tree = self.tree()?;
        let selected = self.selected_index().and(self.selected);
        let Some(id) = selected else {
            return Some(tree.root_path().to_owned());
        };
        let path = tree.path(id)?;
        match tree.row(id)?.kind {
            EntryKind::Dir if Some(path.as_path()) != beside => Some(path),
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
                fs_ops::copy_into(&root, &source, &dir)
            }
        });
        cx.spawn_in(window, async move |panel, cx| {
            let copied = job.await;
            panel
                .update(cx, |panel, cx| match copied {
                    Ok(copy) => {
                        if let Some(copy) = panel.shown(&copy, &root) {
                            panel.changed(&copy, cx);
                            let folder = copy.symlink_metadata().is_ok_and(|meta| meta.is_dir());
                            cx.emit(PanelEvent::Created(vec![(copy.clone(), folder)]));
                            panel.reveal(copy, cx);
                        }
                    }
                    Err(error) => failed(
                        &tr!("workbench-project-paste-failed", name = &name),
                        &error,
                        cx,
                    ),
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Moves `source` into `dir`, both under `root`, as a rename is made
    /// ([`Self::move_now`]), once language servers whose `willRename`
    /// filters match have answered with their edits. The name it takes
    /// there is looked up first, for them to be asked about.
    fn paste_cut(
        &mut self,
        clip: Clip,
        source: PathBuf,
        dir: PathBuf,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = match fs_ops::paste_name(&root, &source, &dir) {
            Ok(name) => name,
            Err(error) => {
                let message = tr!("workbench-project-move-failed", name = file_name(&source));
                return failed(&message, &error, cx);
            }
        };
        let shown = (self.shown(&source, &root)).zip(self.shown(&dir.join(&name), &root));
        let Some((shown_from, shown_to)) = shown else {
            return;
        };
        let moving = Move {
            clip,
            identity: ops::entry_identity(&source),
            root,
            from: source,
            dir,
            name,
            shown_from,
            shown_to,
        };
        if self.move_refused(&moving, window, cx) {
            return;
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

    /// Whether `moving` must wait, because something acts on its old path
    /// or on its new one, in the folder it goes in ([`Workbench::busy_under`]:
    /// a save, a file being read, or another of the panel's operations, such
    /// as that folder being moved to the Trash): then a toast says why.
    fn move_refused(&self, moving: &Move, window: &Window, cx: &mut Context<Self>) -> bool {
        let (busy, name) = match Workbench::busy_under(&moving.shown_from, window, cx) {
            Some(busy) => (busy, moving.entry_name()),
            None => match Workbench::busy_under(&moving.shown_to, window, cx) {
                Some(busy) => (busy, file_name(&moving.dir)),
                None => return false,
            },
        };
        let message = match busy {
            Busy::Saving => tr!("workbench-project-paste-busy-saving", name = &name),
            Busy::Opening => tr!("workbench-project-paste-busy-opening", name = &name),
            Busy::Working => tr!("workbench-project-paste-busy-working", name = &name),
        };
        toast::show(cx, Toast::warning(message));
        true
    }

    /// Moves `moving`'s entry on this thread, then makes `edits`, language
    /// servers' for the move, with its editors taking the new path in the
    /// same turn; then brings the tree up to date and selects it. A folder
    /// expanded before is after.
    ///
    /// Everything is checked again first, as the servers may have taken a
    /// while: nothing is moved if something acts on either path now
    /// ([`Self::move_refused`]), if the entry was moved or replaced since
    /// they were asked about it, or if its new path is taken. The move is
    /// made before the edits, so one that fails makes none of them.
    fn move_now(
        &mut self,
        moving: Move,
        edits: Vec<FileEdits>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_refused(&moving, window, cx) {
            return;
        }
        let entry = moving.entry_name();
        if moving.identity.is_none() || ops::entry_identity(&moving.from) != moving.identity {
            return toast::show(
                cx,
                Toast::error(tr!("workbench-project-paste-raced", name = &entry)),
            );
        }
        // Planned while every spelling of the old path still resolves.
        let moves = Workbench::plan_rename(&moving.shown_from, &moving.shown_to, window, cx);
        // By its folder: a symlink moved is the link, not what it leads to.
        let from_resolved = by_folder(&moving.shown_from);
        let expanded = self.tree().is_some_and(|tree| {
            (tree.find(&moving.from))
                .and_then(|id| tree.row(id))
                .is_some_and(|row| row.kind == EntryKind::Dir && row.expanded)
        });
        let moved = fs_ops::move_into(&moving.root, &moving.from, &moving.dir, &moving.name);
        let path = match moved {
            Ok(path) => path,
            Err(error) => {
                let message = tr!("workbench-project-move-failed", name = &entry);
                return failed(&message, &error, cx);
            }
        };
        let Move {
            clip,
            shown_from,
            shown_to,
            ..
        } = moving;
        let edits = moved_edits(edits, &shown_from, &shown_to, from_resolved.as_deref());
        cx.emit(PanelEvent::Renamed {
            from: shown_from.clone(),
            to: shown_to.clone(),
            moves,
        });
        // Once the editors have taken the new paths, which the event has
        // them do: an open file in a moved folder is found by its new one.
        window.defer(cx, move |window, cx| ops::apply_edits(edits, window, cx));
        if self.clipboard.item.as_ref() == Some(&clip) {
            self.clipboard.item = None;
        }
        self.apply_fs_changes(
            &[FsChange::Path(shown_from), FsChange::Path(shown_to.clone())],
            cx,
        );
        if let Some(id) = (self.tree().and_then(|tree| tree.find(&path))).filter(|_| expanded) {
            self.expand(id, cx);
        }
        self.reveal(shown_to, cx);
    }

    /// Whether a paste now, on the selected row (or the root with none),
    /// would do anything: there's an entry to paste, no cut's paste waits,
    /// and a cut entry wouldn't land where it is. For the tree's menu,
    /// which leaves out Paste otherwise.
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
                let source = tree.root_path().join(&clip.rel);
                (self.paste_folder(None)).is_some_and(|dir| source.parent() != Some(&dir))
            }
        }
    }

    /// The cut entry's node, if it's loaded, for its row to be dimmed.
    pub(super) fn cut_node(&self) -> Option<NodeId> {
        let clip = (self.clipboard.item.as_ref()).filter(|clip| clip.kind == ClipKind::Cut)?;
        let tree = self.tree()?;
        tree.find(tree.root_path().join(&clip.rel))
    }

    /// The tree is opened anew: a look at the disk that began under the old
    /// one no longer counts, and the new one reconciles the entry once it's
    /// listed ([`Self::drop_gone_clip`]).
    pub(super) fn clip_reopened(&mut self) {
        self.clipboard.seen += 1;
    }

    /// Drops the entry to paste once it's gone. When the tree may say so
    /// (the nearest folder above it that's loaded is listed, and up to
    /// date, without it), the disk is asked, off this thread: a tree that
    /// hides ignored entries doesn't list one that's there. The answer
    /// counts only if nothing newer was seen meanwhile ([`Self::clip_looked`]).
    pub(super) fn drop_gone_clip(&mut self, cx: &mut Context<Self>) {
        if self.clipboard.item.is_none() {
            return;
        }
        self.clipboard.seen += 1;
        let (Some(clip), Some(tree)) = (&self.clipboard.item, self.tree()) else {
            return;
        };
        let path = tree.root_path().join(&clip.rel);
        if tree.find(&path).is_some() {
            return;
        }
        let nearest = path.ancestors().skip(1).find_map(|dir| tree.find(dir));
        if !nearest.is_some_and(|dir| tree.listing(dir).is_none()) {
            return;
        }
        let (seen, root) = (self.clipboard.seen, tree.root_path().to_owned());
        let exists = cx.background_spawn(async move { fs_ops::exists(&root, &path) });
        cx.spawn(async move |panel, cx| {
            let exists = exists.await;
            panel
                .update(cx, |panel, cx| panel.clip_looked(seen, exists, cx))
                .ok();
        })
        .detach();
    }

    /// Takes in a look at the disk for the entry to paste, started when
    /// [`Clipboard::seen`] was `seen`: if it's gone, and nothing has been
    /// seen since (another cut or copy, a watcher batch, a listing), it's
    /// dropped. An answer that can't tell keeps it.
    pub(super) fn clip_looked(
        &mut self,
        seen: u64,
        exists: std::io::Result<bool>,
        cx: &mut Context<Self>,
    ) {
        if seen == self.clipboard.seen && exists.is_ok_and(|exists| !exists) {
            self.clipboard.item = None;
            cx.notify();
        }
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
fn file_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    name.to_string_lossy().into_owned()
}

/// Shows `message` and why, `error`, in a toast.
fn failed(message: &str, error: &std::io::Error, cx: &mut gpui::App) {
    let detail = ops::capitalized(&error.to_string());
    toast::show(cx, Toast::error(message).detail(detail));
}

#[cfg(test)]
impl ProjectPanel {
    /// The entry to paste, whether it was cut, and its path from the root.
    pub(crate) fn clipboard_entry(&self) -> Option<(bool, PathBuf)> {
        let clip = self.clipboard.item.as_ref()?;
        Some((clip.kind == ClipKind::Cut, clip.rel.clone()))
    }

    /// What a look at the disk for the entry to paste started now would
    /// take in ([`Self::clip_looked`]).
    pub(crate) fn clip_seen(&self) -> u64 {
        self.clipboard.seen
    }
}
