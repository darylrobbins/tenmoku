//! The project panel's file operations, on the selected row: a new file or
//! folder, rename, move to the Trash, duplicate, and copying its path.
//!
//! A new entry and a rename are named in an inline field, VS Code style: a
//! row of its own as the first in the folder (the selected one, or the
//! selected file's), or in place of the renamed row's name, its stem
//! selected. The field is kiln-ui's [`TextField`] (it has a caret and a
//! selection) in a `text_input` key context: the `text_input.*` commands
//! edit it, its own keys do the rest, and vim never sees what's typed
//! there. Enter (the panel's `project_panel.open`, or `text_input.submit`)
//! runs the operation, and escape or the field losing focus cancels it; a
//! refused name shows its error under the field, which stays open. A new
//! entry's name may hold `/`s, which name folders to create above it, and
//! one ending in `/` names a folder.
//!
//! Every operation goes through `fe_project`'s `fs_ops`, under the tree's
//! (canonical) root. Creating, duplicating and trashing run off the main
//! thread. A rename runs on it, refused while a buffer it would move is
//! being saved: so no save can start between the rename and the editors
//! taking the new path, which [`PanelEvent::Renamed`] has them do in the
//! same turn, before any watcher batch naming the old path is taken in.
//! Once an operation is done the panel brings the tree up to date as it
//! would for the watcher's batch (which may come too), then selects the
//! new or renamed row; a new file opens in the active pane, focused. What
//! it made, renamed or trashed is announced ([`PanelEvent`]) even if the
//! tree is being opened again meanwhile.
//!
//! Before a rename, language servers whose `willRename` filters match are
//! asked for the edits it needs ([`Workbench::will_rename`]), as VS Code's
//! explorer asks its rename participants. While they're asked the field
//! says "Renaming…" and takes no other name; escape stops waiting and
//! renames at once. The edits they give are made
//! ([`Workbench::apply_file_edits`]), then the rename is checked again and
//! made; edits that can't be made, or a server that fails, don't stop it.
//! If the entry was moved or replaced meanwhile (by its device and inode),
//! neither the edits nor the rename are made. With no server to ask, the
//! rename is made at once, as before.

use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    task::{Poll, Waker},
};

use fe_gpui::commands::RunCommand;
use fe_lsp::FileEdits;
use fe_project::{FsChange, NameError, NodeId, fs_ops};
use fe_ui::{CHEVRON_WIDTH, ICON_SIZE, Icon, TREE_INDENT, TextField, TextFieldEvent, text_field};
use gpui::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, Focusable as _,
    InteractiveElement as _, IntoElement as _, KeyDownEvent, ParentElement as _, PromptButton,
    PromptLevel, SharedString, Styled as _, Subscription, Window, deferred, div,
    prelude::FluentBuilder as _, px, svg,
};

use super::{PanelEvent, PanelOp, ProjectPanel, ROW_HEIGHT, State, op};
use crate::{
    Workbench,
    picker::{self, PaletteOp},
    workbench::Busy,
};

/// The key context the name field sits in, for the `text_input.*`
/// commands' keys.
const FIELD_KEY_CONTEXT: &str = "text_input";

/// The buttons of the question before a move to the Trash. Cancel is the
/// default, as [`fe_gpui::pane_item::DISCARD_BUTTONS`] makes it: GPUI gives
/// Return to the first button, so Cancel is an ok button, which Escape
/// finds by its title.
const TRASH_BUTTONS: [PromptButton; 2] = [
    PromptButton::Ok(SharedString::new_static("Cancel")),
    PromptButton::Other(SharedString::new_static("Move to Trash")),
];

/// The answer to [`TRASH_BUTTONS`] that moves the entry.
const MOVE_TO_TRASH: usize = 1;

/// What the name field names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditKind {
    NewFile,
    NewFolder,
    Rename,
}

/// The name field, open.
pub(super) struct NameEdit {
    pub(super) kind: EditKind,
    /// For a new entry the folder it goes in (the tree's root, maybe); for
    /// a rename the entry.
    pub(super) node: NodeId,
    input: Entity<TextField>,
    /// Why the last name given was refused, shown under the field.
    error: Option<String>,
    /// Whether the operation is running: Enter waits for it.
    running: bool,
    /// The rename waiting for language servers' edits, which escape stops
    /// waiting for.
    waiting: Option<Waiting>,
    /// Which field this is, of [`ProjectPanel::edits`].
    serial: u64,
    _subscriptions: [Subscription; 2],
}

/// The text under the field while a rename waits for language servers.
const RENAMING: &str = "Renaming…";

/// A rename waiting for language servers' edits.
struct Waiting {
    naming: Naming,
    /// The entry asked about ([`entry_identity`]).
    entry: Option<(u64, u64)>,
    /// Sent to stop waiting: the rename is made without the edits.
    skip: async_channel::Sender<()>,
}

/// What the name field's operation names: `name` in `target`, the folder a
/// new entry goes in or the renamed entry, under the tree's (canonical)
/// `root`, for field `serial`.
#[derive(Clone)]
struct Naming {
    serial: u64,
    kind: EditKind,
    root: PathBuf,
    target: PathBuf,
    name: String,
}

/// What an operation that names an entry did: the entry's path, under the
/// tree's root.
type Named = io::Result<PathBuf>;

impl ProjectPanel {
    /// Opens the name field for `kind` on the selected row: a new entry goes
    /// in the selected folder, or in the selected file's folder, or in the
    /// root with no row selected; a folder it goes in is expanded. A rename
    /// needs a selected row, and starts from its name. The field takes
    /// focus.
    pub(super) fn start_edit(
        &mut self,
        kind: EditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tree) = self.tree() else {
            return;
        };
        let selected = self.selected_index().and(self.selected);
        let (node, text) = match kind {
            EditKind::Rename => {
                let Some(id) = selected else {
                    return;
                };
                let name = tree.row(id).expect("the selection is shown").name;
                (id, name.to_string_lossy().into_owned())
            }
            EditKind::NewFile | EditKind::NewFolder => {
                let dir = match selected.and_then(|id| Some((id, tree.row(id)?.kind))) {
                    Some((id, fe_project::EntryKind::Dir)) => id,
                    Some((id, _)) => tree
                        .path(id)
                        .and_then(|path| tree.find(path.parent()?))
                        .unwrap_or(tree.root()),
                    None => tree.root(),
                };
                (dir, String::new())
            }
        };
        if kind != EditKind::Rename {
            self.expand(node, cx);
        }
        let placeholder = match kind {
            EditKind::NewFile => "File name",
            EditKind::NewFolder => "Folder name",
            EditKind::Rename => "New name",
        };
        let input = cx.new(|cx| {
            let mut input = TextField::new(window, cx)
                .placeholder(placeholder)
                .text(&text);
            // A rename replaces the stem as it's typed, keeping the
            // extension.
            input.set_selection(0, stem_len(&text), cx);
            input
        });
        let focus = input.focus_handle(cx);
        self.edits += 1;
        let serial = self.edits;
        // Enter where the field's own binding takes it.
        let submitted = cx.subscribe_in(&input, window, |panel, _, event, window, cx| {
            if *event == TextFieldEvent::Submitted {
                panel.commit_edit(window, cx);
            }
        });
        let blur = cx.on_blur(&focus, window, move |panel, _, cx| {
            if panel
                .edit
                .as_ref()
                .is_some_and(|edit| edit.serial == serial)
            {
                panel.edit = None;
                cx.notify();
            }
        });
        self.edit = Some(NameEdit {
            kind,
            node,
            input,
            error: None,
            running: false,
            waiting: None,
            serial,
            _subscriptions: [submitted, blur],
        });
        window.focus(&focus, cx);
        if let Some((at, _)) = self.field_row() {
            self.scroll
                .scroll_to_item(at, gpui::ScrollStrategy::Nearest);
        } else if let Some(ix) = self.selected_index() {
            self.scroll
                .scroll_to_item(ix, gpui::ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Expands directory `dir` once it's listed, unless it's the root or
    /// expanded already.
    pub(super) fn expand(&mut self, dir: NodeId, cx: &mut Context<Self>) {
        let Some(row) = self.tree().and_then(|tree| tree.row(dir)) else {
            return;
        };
        if !row.expanded && self.expanding.insert(dir) {
            // Retries a failed listing.
            self.epoch += 1;
            self.reconcile(cx);
        }
    }

    /// Whether the name field has focus.
    pub(super) fn field_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.edit
            .as_ref()
            .is_some_and(|edit| edit.input.focus_handle(cx).is_focused(window))
    }

    /// Runs a command while the name field has focus: Enter runs the
    /// operation, the `text_input` edits and the panel's left and right
    /// edit the field and move its caret, the panel's cut, copy and paste
    /// (cmd/ctrl-x, -c and -v) are the field's, and the panel's other keys
    /// do nothing, so typing reaches only the field. Others go on to the
    /// workbench.
    pub(super) fn run_in_field(
        &mut self,
        run: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if run.0.command.as_str() == "text_input.submit" {
            return self.commit_edit(window, cx);
        }
        let action: Box<dyn gpui::Action> = match op(run, cx) {
            Some(PanelOp::Open) => return self.commit_edit(window, cx),
            Some(PanelOp::CollapseOrParent) => Box::new(text_field::Left),
            Some(PanelOp::ExpandOrFirstChild) => Box::new(text_field::Right),
            Some(PanelOp::Cut) => Box::new(text_field::Cut),
            Some(PanelOp::Copy) => Box::new(text_field::Copy),
            Some(PanelOp::Paste) => Box::new(text_field::Paste),
            Some(_) => return,
            None => match picker::op(run, cx) {
                Some(PaletteOp::Backspace) => Box::new(text_field::Backspace),
                Some(PaletteOp::DeleteWordLeft) => Box::new(text_field::DeleteWordLeft),
                Some(PaletteOp::Paste) => Box::new(text_field::Paste),
                Some(PaletteOp::Clear) => {
                    if let Some(edit) = &self.edit {
                        edit.input.update(cx, |input, cx| input.set_value("", cx));
                    }
                    return;
                }
                _ => return cx.propagate(),
            },
        };
        // To the field, which has focus.
        window.dispatch_action(action, cx);
    }

    /// Escape cancels the field, or, while a rename waits for language
    /// servers, stops waiting and renames.
    fn on_field_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            match self.edit.as_mut().and_then(|edit| edit.waiting.take()) {
                Some(waiting) => {
                    // The task waiting sees this first, and drops the
                    // requests.
                    waiting.skip.try_send(()).ok();
                    self.renaming_done(waiting.naming.serial);
                    self.rename_answered(waiting.naming, waiting.entry, Vec::new(), window, cx);
                }
                None => self.cancel_edit(window, cx),
            }
            cx.stop_propagation();
        }
    }

    /// Closes the name field, doing nothing, and focuses the panel.
    pub(super) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Runs the name field's operation with the name typed: a create in
    /// the background, a rename here ([`Self::rename`]). A name refused
    /// before it starts shows its error, as one the operation fails with
    /// does once it's over; renaming to the same name only closes the
    /// field.
    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Open(tree) = &self.state else {
            return;
        };
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        if edit.running {
            return;
        }
        let name = edit.input.read(cx).value().to_owned();
        let valid = fs_ops::validate_name(&name).and_then(|()| match edit.kind {
            EditKind::Rename if name.contains('/') => Err(NameError::Separator),
            _ => Ok(()),
        });
        if let Err(error) = valid {
            edit.error = Some(capitalized(&error.to_string()));
            return cx.notify();
        }
        let Some(target) = tree.path(edit.node) else {
            return;
        };
        let (kind, root) = (edit.kind, tree.root_path().to_owned());
        let naming = Naming {
            serial: edit.serial,
            kind,
            root: root.clone(),
            target: target.clone(),
            name: name.clone(),
        };
        if kind == EditKind::Rename {
            if target.file_name().is_some_and(|old| *old == *name) {
                return self.cancel_edit(window, cx);
            }
            return self.rename(naming, window, cx);
        }
        edit.running = true;
        edit.error = None;
        // All it makes: the folders the name names, and the entry.
        let running = Running::start(target.join(name.trim_end_matches('/')));
        let job = {
            let running = running.clone();
            cx.background_spawn(async move {
                let _running = running;
                match kind {
                    EditKind::NewFolder => fs_ops::create_dir(&root, &target, &name),
                    EditKind::NewFile | EditKind::Rename => {
                        fs_ops::create_file(&root, &target, &name)
                    }
                }
            })
        };
        cx.spawn_in(window, async move |panel, cx| {
            let named = job.await;
            panel
                .update_in(cx, |panel, window, cx| {
                    panel.edit_done(naming, named, window, cx)
                })
                .ok();
            drop(running);
        })
        .detach();
        cx.notify();
    }

    /// Renames `naming`'s entry, unless something would still act on its
    /// path once it's done ([`Self::refused`]). Language servers whose
    /// `willRename` filters match are asked for their edits first: while
    /// they're asked the field says so and takes no other name, and once
    /// they answer the edits are made and the rename is made
    /// ([`Self::rename_now`]). With none to ask, the rename is made here.
    fn rename(&mut self, naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        if self.refused(&naming, window, cx) {
            return;
        }
        let asked = self.renaming_paths(&naming).and_then(|(from, to)| {
            let workbench = window.root::<Workbench>().flatten()?;
            Some(workbench.read(cx).will_rename(&from, &to, cx))
        });
        let Some(mut asked) = asked else {
            return self.rename_now(naming, window, cx);
        };
        // Ready at once when no server's filters match.
        if let Some(edits) = ready_now(&mut asked) {
            apply_edits(edits, window, cx);
            return self.rename_now(naming, window, cx);
        }
        let Some(edit) = self
            .edit
            .as_mut()
            .filter(|edit| edit.serial == naming.serial)
        else {
            return;
        };
        let (skip, skipped) = async_channel::bounded(1);
        // What's renamed once they answer must be what they were asked
        // about.
        let entry = entry_identity(&naming.target);
        edit.running = true;
        edit.error = None;
        edit.waiting = Some(Waiting {
            naming: naming.clone(),
            entry,
            skip,
        });
        cx.spawn_in(window, async move |panel, cx| {
            // A field closed meanwhile keeps waiting: the rename goes on.
            let mut skipped = std::pin::pin!(async move {
                if skipped.recv().await.is_err() {
                    std::future::pending::<()>().await;
                }
            });
            // Skipped first: escape renamed already, and an answer that
            // came with it isn't made.
            let edits = std::future::poll_fn(|cx| {
                if skipped.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                Pin::new(&mut asked).poll(cx).map(Some)
            });
            let Some(edits) = edits.await else {
                return;
            };
            panel
                .update_in(cx, |panel, window, cx| {
                    panel.renaming_done(naming.serial);
                    panel.rename_answered(naming, entry, edits, window, cx);
                })
                .ok();
        })
        .detach();
        cx.notify();
    }

    /// Makes `edits`, language servers' for `naming`'s rename, then the
    /// rename, unless its entry is no longer `entry`, the one they were
    /// asked about: then neither, and the field, if it's open, says why.
    fn rename_answered(
        &mut self,
        naming: Naming,
        entry: Option<(u64, u64)>,
        edits: Vec<FileEdits>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.is_none() || entry_identity(&naming.target) != entry {
            let old = naming.target.file_name().unwrap_or_default();
            let error = format!(
                "“{}” was moved or replaced while it was being renamed. Nothing was renamed.",
                old.to_string_lossy()
            );
            match (self.edit.as_mut()).filter(|edit| edit.serial == naming.serial) {
                Some(edit) => edit.error = Some(error),
                None => log::warn!("{error}"),
            }
            return cx.notify();
        }
        apply_edits(edits, window, cx);
        self.rename_now(naming, window, cx);
    }

    /// Field `serial`, if it's still open, has its rename's wait over.
    fn renaming_done(&mut self, serial: u64) {
        if let Some(edit) = self.edit.as_mut().filter(|edit| edit.serial == serial) {
            edit.waiting = None;
            edit.running = false;
        }
    }

    /// The paths `naming`'s rename moves its entry from and to, as the panel
    /// hands them out.
    fn renaming_paths(&self, naming: &Naming) -> Option<(PathBuf, PathBuf)> {
        let rel = naming.target.strip_prefix(&naming.root).ok()?;
        let from = self.root.as_ref()?.join(rel);
        let to = from.parent()?.join(&naming.name);
        Some((from, to))
    }

    /// Whether `naming`'s rename must wait, because something would still
    /// act on the old path once it's done ([`Workbench::busy_under`]: a
    /// save, a Save As, a file being read, or another of the panel's
    /// operations), in any window: then the field, if it's open, says why
    /// and stays open.
    fn refused(&mut self, naming: &Naming, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let paths = self.renaming_paths(naming);
        // The new path counts too: a copy or Save As may be about to write
        // there.
        let busy = (paths.as_ref())
            .and_then(|(from, _)| Workbench::busy_under(from, window, cx))
            .or_else(|| {
                let (_, to) = paths.as_ref()?;
                crate::open::any_reserved().then_some(())?;
                Workbench::busy_under(to, window, cx)
            });
        let Some(busy) = busy else {
            return false;
        };
        let old = naming
            .target
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let error = match busy {
            Busy::Saving => format!("“{old}” is being saved. Rename it once it’s saved."),
            Busy::Opening => format!("“{old}” is being opened. Rename it once it’s open."),
            Busy::Working => {
                format!("“{old}” is being worked on. Rename it once that’s done.")
            }
        };
        match self
            .edit
            .as_mut()
            .filter(|edit| edit.serial == naming.serial)
        {
            Some(edit) => edit.error = Some(error),
            None => log::warn!("not renamed: {error}"),
        }
        cx.notify();
        true
    }

    /// Renames `naming`'s entry on this thread, unless [`Self::refused`].
    /// Done here, between saves, so none starts before the editors take the
    /// new path.
    fn rename_now(&mut self, naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        if self.refused(&naming, window, cx) {
            return;
        }
        // Planned while every spelling of the old path still resolves.
        let moves = (self.renaming_paths(&naming))
            .map(|(from, to)| (Workbench::plan_rename(&from, &to, window, cx), from, to));
        let renamed = fs_ops::rename(&naming.root, &naming.target, &naming.name);
        if let (Ok(_), Some((moves, from, to))) = (&renamed, moves) {
            cx.emit(PanelEvent::Renamed { from, to, moves });
        }
        self.edit_done(naming, renamed, window, cx);
    }

    /// Takes in what the name field's operation did to `naming`'s target
    /// (the folder a new entry went in, or the renamed entry): it shows a
    /// failure in the field, if the field's still open; otherwise closes the
    /// field, brings the tree up to date, says what it made, and selects
    /// the entry, which opens if it's a new file.
    fn edit_done(
        &mut self,
        naming: Naming,
        named: Named,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Naming {
            serial,
            kind,
            root,
            target,
            name,
        } = naming;
        let name = name.as_str();
        let open = self.edit.as_mut().filter(|edit| edit.serial == serial);
        let path = match (named, open) {
            (Err(error), Some(edit)) => {
                edit.running = false;
                edit.error = Some(failure(&error, name));
                return cx.notify();
            }
            (Err(error), None) => {
                return log::warn!("cannot name {name} in {}: {error}", target.display());
            }
            (Ok(path), open) => {
                if open.is_some() {
                    self.cancel_edit(window, cx);
                }
                path
            }
        };
        // By the root the operation ran under: the tree may be being opened
        // again.
        let shown = |path: &Path| {
            let rel = path.strip_prefix(&root).ok()?;
            Some(self.root.as_ref()?.join(rel))
        };
        let Some(new) = shown(&path) else {
            return;
        };
        let mut changes = Vec::new();
        match kind {
            EditKind::Rename => {
                let Some(old) = shown(&target) else {
                    return;
                };
                changes.push(FsChange::Path(old));
                changes.push(FsChange::Path(new.clone()));
            }
            EditKind::NewFile | EditKind::NewFolder => {
                // Each folder the name made, down to the entry.
                let mut at = target.clone();
                for component in path
                    .strip_prefix(&target)
                    .into_iter()
                    .flat_map(|rel| rel.components())
                {
                    at.push(component);
                    changes.extend(shown(&at).map(FsChange::Path));
                }
            }
        }
        // A folder expanded before its rename is after it.
        let expanded = kind == EditKind::Rename
            && self.tree().is_some_and(|tree| {
                (tree.find(&target))
                    .and_then(|id| tree.row(id))
                    .is_some_and(|row| row.expanded)
            });
        self.apply_fs_changes(&changes, cx);
        if kind != EditKind::Rename {
            let folder = kind == EditKind::NewFolder || name.ends_with('/');
            cx.emit(PanelEvent::Created(vec![(new.clone(), folder)]));
        }
        if let Some(id) = (self.tree().and_then(|tree| tree.find(&path))).filter(|_| expanded) {
            self.expand(id, cx);
        }
        self.reveal(new.clone(), cx);
        // A new file's name ending in `/` made a folder, which opens nothing.
        if kind == EditKind::NewFile && !name.ends_with('/') {
            cx.emit(PanelEvent::OpenFile(new));
        }
    }

    /// Asks whether to move the selected entry to the Trash, then does it,
    /// in the background, unless the entry at its path is no longer the one
    /// asked about (by its device and inode, not followed through a
    /// symlink). Editors of it stay open with their buffers.
    pub(super) fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, root, name)) = self.selected_entry() else {
            return;
        };
        let Some(shown) = self.shown(&path, &root) else {
            return;
        };
        // What was asked about: an entry renamed meanwhile, and another put
        // at its path, isn't it.
        let asked = entry_identity(&path);
        let folder = path.symlink_metadata().is_ok_and(|meta| meta.is_dir());
        let question = format!("Move “{name}” to the Trash?");
        let answer = window.prompt(PromptLevel::Warning, &question, None, &TRASH_BUTTONS, cx);
        cx.spawn_in(window, async move |panel, cx| {
            if answer.await != Ok(MOVE_TO_TRASH) {
                return;
            }
            let running = Running::start(path.clone());
            let trashed = cx
                .background_spawn({
                    let path = path.clone();
                    let running = running.clone();
                    async move {
                        let _running = running;
                        if asked.is_none() || entry_identity(&path) != asked {
                            let moved = "It was moved or replaced since you were asked.";
                            return Err(io::Error::new(io::ErrorKind::NotFound, moved));
                        }
                        fs_ops::trash(&root, &[path])
                    }
                })
                .await;
            panel
                .update_in(cx, |panel, window, cx| match trashed {
                    Ok(()) => {
                        panel.changed(&shown, cx);
                        cx.emit(PanelEvent::Deleted(vec![(shown, folder)]));
                    }
                    Err(error) => {
                        let message = format!("“{name}” couldn’t be moved to the Trash.");
                        failed(&message, &error, window, cx);
                    }
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Copies the selected entry next to itself (`name copy.ext`), in the
    /// background, and selects the copy.
    pub(super) fn duplicate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, root, name)) = self.selected_entry() else {
            return;
        };
        // The copy goes beside it, named once it's made.
        let running = Running::start(path.parent().unwrap_or(&path).to_owned());
        let job = cx.background_spawn({
            let running = running.clone();
            let root = root.clone();
            async move {
                let _running = running;
                fs_ops::duplicate(&root, &path)
            }
        });
        cx.spawn_in(window, async move |panel, cx| {
            let copied = job.await;
            panel
                .update_in(cx, |panel, window, cx| match copied {
                    Ok(copy) => {
                        if let Some(copy) = panel.shown(&copy, &root) {
                            panel.changed(&copy, cx);
                            let folder = copy.symlink_metadata().is_ok_and(|meta| meta.is_dir());
                            cx.emit(PanelEvent::Created(vec![(copy.clone(), folder)]));
                            panel.reveal(copy, cx);
                        }
                    }
                    Err(error) => {
                        let message = format!("“{name}” couldn’t be duplicated.");
                        failed(&message, &error, window, cx);
                    }
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Puts the selected entry's path on the clipboard: absolute, or from
    /// the root with `/`s.
    pub(super) fn copy_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected_index().and(self.selected) else {
            return;
        };
        let Some(path) = self.shown_path(id) else {
            return;
        };
        let text = match (relative, self.root.as_deref()) {
            (true, Some(root)) => fs_ops::relative_path(root, &path),
            _ => path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// The selected entry's path under the tree's root, that root, and the
    /// entry's name.
    fn selected_entry(&self) -> Option<(PathBuf, PathBuf, String)> {
        let id = self.selected_index().and(self.selected)?;
        let tree = self.tree()?;
        let path = tree.path(id)?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some((path, tree.root_path().to_owned(), name))
    }

    /// `path`, under the tree's (canonical) `root`, as the panel hands it
    /// out.
    pub(super) fn shown(&self, path: &Path, root: &Path) -> Option<PathBuf> {
        let rel = path.strip_prefix(root).ok()?;
        Some(self.root.as_ref()?.join(rel))
    }

    /// Brings the tree, if it's open, up to date with a change at `shown`,
    /// as the watcher's batch would.
    pub(super) fn changed(&mut self, shown: &Path, cx: &mut Context<Self>) {
        self.apply_fs_changes(&[FsChange::Path(shown.to_owned())], cx);
    }

    /// Where a new entry's field sits in the list, and its depth: the first
    /// row in its folder. `None` unless a new entry is being named, in a
    /// folder that's shown.
    pub(super) fn field_row(&self) -> Option<(usize, usize)> {
        let edit = self
            .edit
            .as_ref()
            .filter(|edit| edit.kind != EditKind::Rename)?;
        let tree = self.tree()?;
        if edit.node == tree.root() {
            return Some((0, 0));
        }
        let depth = tree.row(edit.node)?.depth;
        let ix = Self::index_of(tree, Some(edit.node), self.field_hint.get())?;
        self.field_hint.set(ix);
        Some((ix + 1, depth + 1))
    }

    /// Where tree row `ix` is in the list: one further down from a new
    /// entry's field on.
    pub(super) fn list_ix(&self, ix: usize) -> usize {
        match self.field_row() {
            Some((at, _)) if ix >= at => ix + 1,
            _ => ix,
        }
    }

    /// The name field's row, at `depth` with `icon`, its error under it.
    pub(super) fn render_field(
        &self,
        depth: usize,
        icon: Icon,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(edit) = &self.edit else {
            return div().into_any_element();
        };
        let theme = fe_ui::ui_theme(cx);
        // The field draws its own border; an error's is red.
        let border = edit.error.is_some().then_some(theme.error_text);
        let note = (edit.waiting.as_ref()).map(|_| (RENAMING.to_owned(), theme.text));
        let error = (edit.error.clone())
            .map(|error| (error, theme.error_text))
            .or(note);
        let error = error.map(|(error, color)| {
            deferred(
                div()
                    .absolute()
                    .top(ROW_HEIGHT)
                    .left(px(4.))
                    .right(px(4.))
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(color)
                    .bg(theme.panel_background)
                    .text_color(color)
                    .child(error),
            )
            .with_priority(1)
        });
        div()
            .id("name-field")
            .relative()
            .h(ROW_HEIGHT)
            .w_full()
            .flex()
            .items_center()
            .pl(px(6.) + TREE_INDENT * depth as f32 + CHEVRON_WIDTH + px(2.))
            .pr(px(8.))
            .on_key_down(cx.listener(Self::on_field_key))
            .child(
                svg()
                    .path(icon.path())
                    .size(ICON_SIZE)
                    .flex_none()
                    .mr(px(6.))
                    .text_color(theme.icon),
            )
            .child(
                div()
                    .key_context(FIELD_KEY_CONTEXT)
                    .flex_1()
                    .min_w_0()
                    .h(ROW_HEIGHT - px(2.))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .when_some(border, |field, border| {
                        field.border_1().border_color(border)
                    })
                    .child(edit.input.clone()),
            )
            .children(error)
            .into_any_element()
    }
}

/// Makes `edits`, language servers' for a rename, in `window`'s workbench;
/// edits that can't be made are logged, and the rename goes on.
pub(super) fn apply_edits(edits: Vec<FileEdits>, window: &Window, cx: &mut gpui::App) {
    if edits.is_empty() {
        return;
    }
    let Some(workbench) = window.root::<Workbench>().flatten() else {
        return;
    };
    let applied = workbench.update(cx, |workbench, cx| workbench.apply_file_edits(edits, cx));
    if let Err(error) = applied {
        log::warn!("the rename's edits weren't made: {error}");
    }
}

/// The message for a name the operation refused, `name` being the name
/// typed.
fn failure(error: &io::Error, name: &str) -> String {
    match error.kind() {
        io::ErrorKind::AlreadyExists => {
            format!("A file or folder “{name}” already exists here. Choose another name.")
        }
        _ => capitalized(&error.to_string()),
    }
}

/// The device and inode of the entry at `path`, a symlink itself rather
/// than its target; `None` if there's none.
pub(super) fn entry_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::symlink_metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

/// The paths the panel's operations running in the background work in, in
/// every window: a create's entry (with the folders its name makes), the
/// folder a duplicate's copy goes in, a move to the Trash's entry. Each is
/// spelled as the tree spells it, under its canonical root, so its folders
/// match any spelling of the project ([`crate::workbench::overlap`]). A
/// rename overlapping one waits ([`Workbench::busy_under`]).
static RUNNING: Mutex<Vec<(u64, PathBuf)>> = Mutex::new(Vec::new());

/// Counts the operations started, to key [`RUNNING`].
static STARTED: AtomicU64 = AtomicU64::new(0);

/// An operation's entry in [`RUNNING`], held by its background job and by
/// what takes its outcome in: there until both are over, whether or not the
/// panel or its window still are.
pub(super) struct Running(u64);

impl Running {
    pub(super) fn start(path: PathBuf) -> Arc<Running> {
        let id = STARTED.fetch_add(1, Ordering::Relaxed);
        RUNNING.lock().unwrap().push((id, path));
        Arc::new(Running(id))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        running.retain(|(id, _)| *id != self.0);
    }
}

/// [`RUNNING`]'s paths.
pub(crate) fn running() -> Vec<PathBuf> {
    let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    running.iter().map(|(_, path)| path.clone()).collect()
}

/// `future`'s output, if it has it on its first poll.
pub(crate) fn ready_now<F: Future + Unpin>(future: &mut F) -> Option<F::Output> {
    let mut cx = std::task::Context::from_waker(Waker::noop());
    match Pin::new(future).poll(&mut cx) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

/// The length of `name`'s stem, which a rename selects: up to its last
/// `.`, unless that's its first character (`.gitignore`) or there's none.
fn stem_len(name: &str) -> usize {
    name.rfind('.').filter(|&dot| dot > 0).unwrap_or(name.len())
}

/// `text` with its first letter in upper case.
pub(super) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Shows `message` and why, `error`, over the window.
fn failed(message: &str, error: &io::Error, window: &mut Window, cx: &mut gpui::App) {
    let detail = capitalized(&error.to_string());
    drop(window.prompt(
        PromptLevel::Critical,
        message,
        Some(&detail),
        &[PromptButton::ok("OK")],
        cx,
    ));
}

#[cfg(test)]
impl ProjectPanel {
    /// The name field's text and error, while it's open.
    pub(crate) fn name_field(&self, cx: &gpui::App) -> Option<(String, Option<String>)> {
        let edit = self.edit.as_ref()?;
        Some((edit.input.read(cx).value().to_owned(), edit.error.clone()))
    }

    /// The note under the name field, while it's open and a rename waits
    /// for language servers.
    pub(crate) fn name_field_note(&self) -> Option<&'static str> {
        self.edit.as_ref()?.waiting.as_ref().map(|_| RENAMING)
    }

    /// The name field's input, while it's open.
    pub(crate) fn name_input(&self) -> Option<Entity<TextField>> {
        Some(self.edit.as_ref()?.input.clone())
    }
}

#[cfg(test)]
#[path = "project_panel_ops_tests.rs"]
mod project_panel_ops_tests;
