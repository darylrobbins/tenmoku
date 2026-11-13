//! The project panel's file operations, on the selected row: a new file or
//! folder, rename, move to the Trash, duplicate, and copying its path.
//! With several rows selected, a new entry and a rename act on the primary
//! alone; the rest act on every selected row ([`ProjectPanel::selection`]):
//! one question names how many go to the Trash (a selected folder's
//! selected entries go with it), one background job trashes or duplicates
//! each in turn, and one toast names those that failed and why. Copy Path
//! puts their paths a line apiece.
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

use kiln_l10n::{Text, text, tr};
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
use fe_ui::{
    CHEVRON_WIDTH, ICON_SIZE, IconRef, TREE_INDENT, TextField, TextFieldEvent, text_field,
};
use gpui::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, Focusable as _,
    InteractiveElement as _, IntoElement as _, KeyDownEvent, ParentElement as _, PromptButton,
    PromptLevel, SharedString, Styled as _, Subscription, Window, deferred, div,
    prelude::FluentBuilder as _, px, svg,
};

use super::{
    PanelEvent, PanelOp, ProjectPanel, ROW_HEIGHT, State, op,
    undo::{Made, Moved, Step},
};
use crate::{
    Workbench,
    picker::{self, PaletteOp},
    workbench::{
        Busy,
        toast::{self, Toast},
    },
};

/// The key context the name field sits in, for the `text_input.*`
/// commands' keys.
const FIELD_KEY_CONTEXT: &str = "text_input";

/// The buttons of the question before a move to the Trash. Cancel is the
/// default, as [`fe_gpui::pane_item::DISCARD_BUTTONS`] makes it: GPUI gives
/// Return to the first button, so Cancel is an ok button, which Escape
/// finds by its title.
fn trash_buttons() -> [PromptButton; 2] {
    [
        PromptButton::Ok(tr!("workbench-project-cancel")),
        PromptButton::Other(tr!("workbench-project-move-to-trash")),
    ]
}

/// The answer to [`trash_buttons`] that moves the entry.
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
    error: Option<FieldError>,
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
fn renaming() -> SharedString {
    tr!("workbench-project-renaming")
}

impl NameEdit {
    /// The field the name is typed in.
    pub(super) fn input(&self) -> Entity<TextField> {
        self.input.clone()
    }

    /// Why the last name given was refused, now, in the active locale.
    pub(super) fn error(&self) -> Option<SharedString> {
        self.error.as_ref().map(FieldError::resolve)
    }
}

/// Why a name was refused, kept as what it is until it's drawn, so it's
/// said in the locale active then.
enum FieldError {
    Text(Text),
    /// A name the checks refused, as fe-project words it, capitalised.
    Name(NameError),
}

impl FieldError {
    fn resolve(&self) -> SharedString {
        match self {
            FieldError::Text(text) => text.resolve(),
            FieldError::Name(error) => capitalized(&error.to_string()).into(),
        }
    }
}

impl From<Text> for FieldError {
    fn from(text: Text) -> Self {
        FieldError::Text(text)
    }
}

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
    /// For a new entry once it's made, the first entry the create made
    /// (the entry, or the first folder above it it made), to undo it by.
    first: Option<Made>,
    /// For a rename, what the entry was as it was renamed.
    identity: Option<fs_ops::EntryId>,
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
            EditKind::NewFile => text!("workbench-project-file-name"),
            EditKind::NewFolder => text!("workbench-project-folder-name"),
            EditKind::Rename => text!("workbench-project-new-name"),
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
            edit.error = Some(FieldError::Name(error));
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
            first: None,
            identity: None,
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
                let folder = kind == EditKind::NewFolder;
                fs_ops::create_recorded(&root, &target, &name, folder)
            })
        };
        cx.spawn_in(window, async move |panel, cx| {
            let mut naming = naming;
            let named = job.await.map(|created| {
                naming.first = Some(created.first.into());
                created.path
            });
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
            let old = old.to_string_lossy();
            let error = text!("workbench-project-rename-raced", name = old.into_owned());
            match (self.edit.as_mut()).filter(|edit| edit.serial == naming.serial) {
                Some(edit) => edit.error = Some(error.into()),
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
        let old = old.into_owned();
        let error = match busy {
            Busy::Saving => text!("workbench-project-rename-busy-saving", name = old),
            Busy::Opening => text!("workbench-project-rename-busy-opening", name = old),
            Busy::Working => text!("workbench-project-rename-busy-working", name = old),
        };
        match self
            .edit
            .as_mut()
            .filter(|edit| edit.serial == naming.serial)
        {
            Some(edit) => edit.error = Some(error.into()),
            None => log::warn!("not renamed: {error}"),
        }
        cx.notify();
        true
    }

    /// Renames `naming`'s entry on this thread, unless [`Self::refused`].
    /// Done here, between saves, so none starts before the editors take the
    /// new path.
    fn rename_now(&mut self, mut naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        if self.refused(&naming, window, cx) {
            return;
        }
        // What's renamed, for the history: a rename keeps it.
        naming.identity = entry_identity(&naming.target);
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
            first,
            identity,
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
        let step = match kind {
            EditKind::Rename => Step::Moved(
                (identity.into_iter())
                    .map(|identity| Moved {
                        from: target.clone(),
                        to: path.clone(),
                        identity,
                    })
                    .collect(),
            ),
            EditKind::NewFile | EditKind::NewFolder => Step::Made(first.into_iter().collect()),
        };
        self.landed_new(&root, step, true);
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
        let Some((root, entries)) = self.selected_entries() else {
            return;
        };
        let mut entries = topmost(entries);
        if entries.len() > 1 {
            return self.delete_many(root, entries, window, cx);
        }
        let Some((path, name)) = entries.pop() else {
            return;
        };
        let Some(shown) = self.shown(&path, &root) else {
            return;
        };
        // What was asked about: an entry renamed meanwhile, and another put
        // at its path, isn't it.
        let asked = entry_identity(&path);
        let folder = path.symlink_metadata().is_ok_and(|meta| meta.is_dir());
        let question = tr!("workbench-project-trash-question", name = &name);
        let answer = window.prompt(PromptLevel::Warning, &question, None, &trash_buttons(), cx);
        let moved = tr!("workbench-project-trash-moved-since");
        cx.spawn_in(window, async move |panel, cx| {
            if answer.await != Ok(MOVE_TO_TRASH) {
                return;
            }
            let running = Running::start(path.clone());
            let trashed = cx
                .background_spawn({
                    let path = path.clone();
                    let running = running.clone();
                    let root = root.clone();
                    async move {
                        let _running = running;
                        if asked.is_none() || entry_identity(&path) != asked {
                            let moved = moved.to_string();
                            return Err(io::Error::new(io::ErrorKind::NotFound, moved));
                        }
                        fs_ops::trash_recorded(&root, &[path])
                    }
                })
                .await;
            panel
                .update_in(cx, |panel, window, cx| match trashed {
                    Ok(trashed) => {
                        panel.landed_new(&root, Step::Trashed(restorable(trashed)), true);
                        panel.changed(&shown, cx);
                        cx.emit(PanelEvent::Deleted(vec![(shown, folder)]));
                    }
                    Err(error) => {
                        let message = tr!("workbench-project-trash-failed", name = &name);
                        failed(&message, &error, window, cx);
                    }
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Asks once whether to move `entries` (paths under the tree's `root`,
    /// and names), none inside another, to the Trash, then moves each in
    /// turn in one background job, unless the entry at its path is no
    /// longer the one asked about. Those moved leave the tree together;
    /// a toast names those that weren't, and why.
    fn delete_many(
        &mut self,
        root: PathBuf,
        entries: Vec<(PathBuf, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked: Vec<Asked> = (entries.into_iter())
            .filter_map(|(path, name)| {
                Some(Asked {
                    shown: self.shown(&path, &root)?,
                    identity: entry_identity(&path),
                    folder: path.symlink_metadata().is_ok_and(|meta| meta.is_dir()),
                    path,
                    name,
                })
            })
            .collect();
        let count = asked.len();
        let question = tr!("workbench-project-trash-question-many", count = count);
        let answer = window.prompt(PromptLevel::Warning, &question, None, &trash_buttons(), cx);
        let moved = tr!("workbench-project-trash-moved-since").to_string();
        cx.spawn_in(window, async move |panel, cx| {
            if answer.await != Ok(MOVE_TO_TRASH) {
                return;
            }
            let running: Vec<_> = (asked.iter())
                .map(|entry| Running::start(entry.path.clone()))
                .collect();
            let jobs: Vec<_> = (asked.iter())
                .map(|entry| (entry.path.clone(), entry.identity))
                .collect();
            let trashed = cx
                .background_spawn({
                    let running = running.clone();
                    let root = root.clone();
                    async move {
                        let _running = running;
                        (jobs.into_iter())
                            .map(|(path, asked)| {
                                if asked.is_none() || entry_identity(&path) != asked {
                                    let moved = moved.clone();
                                    return Err(io::Error::new(io::ErrorKind::NotFound, moved));
                                }
                                fs_ops::trash_recorded(&root, &[path])
                            })
                            .collect::<Vec<_>>()
                    }
                })
                .await;
            panel
                .update(cx, |panel, cx| {
                    let mut deleted = Vec::new();
                    let mut recorded = Vec::new();
                    let mut failures = Vec::new();
                    for (entry, trashed) in asked.into_iter().zip(trashed) {
                        match trashed {
                            Ok(trashed) => {
                                recorded.extend(restorable(trashed));
                                deleted.push((entry.shown, entry.folder));
                            }
                            Err(error) => failures.push(Failure::error(
                                tr!("workbench-project-trash-failed", name = &entry.name),
                                &error,
                            )),
                        }
                    }
                    let changed = !deleted.is_empty();
                    panel.landed_new(&root, Step::Trashed(recorded), changed);
                    if !deleted.is_empty() {
                        let changes: Vec<FsChange> = (deleted.iter())
                            .map(|(shown, _)| FsChange::Path(shown.clone()))
                            .collect();
                        panel.apply_fs_changes(&changes, cx);
                        cx.emit(PanelEvent::Deleted(deleted));
                    }
                    report_failures(failures, Many::Trash, cx);
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Copies the selected entry next to itself (`name copy.ext`), in the
    /// background, and selects the copy. With several selected, each is
    /// copied in turn ([`Self::duplicate_many`]).
    pub(super) fn duplicate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((root, mut entries)) = self.selected_entries() else {
            return;
        };
        if entries.len() > 1 {
            return self.duplicate_many(root, entries, window, cx);
        }
        let Some((path, name)) = entries.pop() else {
            return;
        };
        // The copy goes beside it, named once it's made.
        let running = Running::start(path.parent().unwrap_or(&path).to_owned());
        let job = cx.background_spawn({
            let running = running.clone();
            let root = root.clone();
            async move {
                let _running = running;
                fs_ops::duplicate_recorded(&root, &path)
            }
        });
        cx.spawn_in(window, async move |panel, cx| {
            let copied = job.await;
            panel
                .update_in(cx, |panel, window, cx| match copied {
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
                        let message = tr!("workbench-project-duplicate-failed", name = &name);
                        failed(&message, &error, window, cx);
                    }
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Copies each of `entries` (paths under the tree's `root`, and names)
    /// next to itself, in turn, in one background job; the copies made
    /// show together, the first selected, and a toast names those that
    /// weren't made, and why.
    fn duplicate_many(
        &mut self,
        root: PathBuf,
        entries: Vec<(PathBuf, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Each copy goes beside its entry, named once it's made.
        let running: Vec<_> = (entries.iter())
            .map(|(path, _)| Running::start(path.parent().unwrap_or(path).to_owned()))
            .collect();
        let paths: Vec<PathBuf> = entries.iter().map(|(path, _)| path.clone()).collect();
        let job = cx.background_spawn({
            let running = running.clone();
            let root = root.clone();
            async move {
                let _running = running;
                (paths.iter())
                    .map(|path| fs_ops::duplicate_recorded(&root, path))
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
                    for ((_, name), copied) in entries.iter().zip(copied) {
                        match copied {
                            Ok(copy) => {
                                made.extend(panel.shown(&copy.path, &root));
                                copies.push(Made::from(copy));
                            }
                            Err(error) => failures.push(Failure::error(
                                tr!("workbench-project-duplicate-failed", name = name),
                                &error,
                            )),
                        }
                    }
                    let changed = !copies.is_empty();
                    panel.landed_new(&root, Step::Made(copies), changed);
                    panel.made(made, cx);
                    report_failures(failures, Many::Duplicate, cx);
                })
                .ok();
            drop(running);
        })
        .detach();
    }

    /// Takes in entries an operation made, as the panel hands paths out:
    /// brings the tree up to date with them, says they were made, and
    /// selects the first.
    pub(super) fn made(&mut self, made: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(first) = made.first().cloned() else {
            return;
        };
        let changes: Vec<FsChange> = made.iter().cloned().map(FsChange::Path).collect();
        self.apply_fs_changes(&changes, cx);
        let created = (made.into_iter())
            .map(|path| {
                let folder = path.symlink_metadata().is_ok_and(|meta| meta.is_dir());
                (path, folder)
            })
            .collect();
        cx.emit(PanelEvent::Created(created));
        self.reveal(first, cx);
    }

    /// Puts the selected entries' paths on the clipboard, a line apiece in
    /// the list's order: absolute, or from the root with `/`s.
    pub(super) fn copy_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let lines: Vec<String> = (self.selection().into_iter())
            .filter_map(|(_, id)| self.shown_path(id))
            .map(|path| match (relative, self.root.as_deref()) {
                (true, Some(root)) => fs_ops::relative_path(root, &path),
                _ => path.to_string_lossy().into_owned(),
            })
            .collect();
        if lines.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
    }

    /// The tree's root, and each selected entry's path under it and name,
    /// in the list's order; `None` with none selected.
    pub(super) fn selected_entries(&self) -> Option<(PathBuf, Vec<(PathBuf, String)>)> {
        let tree = self.tree()?;
        let entries: Vec<(PathBuf, String)> = (self.selection().into_iter())
            .filter_map(|(_, id)| {
                let path = tree.path(id)?;
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some((path, name))
            })
            .collect();
        (!entries.is_empty()).then(|| (tree.root_path().to_owned(), entries))
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
        icon: IconRef,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(edit) = &self.edit else {
            return div().into_any_element();
        };
        let theme = fe_ui::ui_theme(cx);
        // The field draws its own border; an error's is red.
        let border = edit.error.is_some().then_some(theme.error_text);
        let note = (edit.waiting.as_ref()).map(|_| (renaming().to_string(), theme.text));
        let error = (edit.error.as_ref())
            .map(|error| (error.resolve().to_string(), theme.error_text))
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

/// An entry asked about before a move to the Trash: its path under the
/// tree's root, its name, its path as the panel hands it out, its device
/// and inode ([`entry_identity`]), and whether it's a folder.
struct Asked {
    path: PathBuf,
    name: String,
    shown: PathBuf,
    identity: Option<(u64, u64)>,
    folder: bool,
}

/// `entries` (paths and names) less those inside another of them: what's
/// in a folder goes with it.
pub(super) fn topmost(entries: Vec<(PathBuf, String)>) -> Vec<(PathBuf, String)> {
    let paths: Vec<PathBuf> = entries.iter().map(|(path, _)| path.clone()).collect();
    (entries.into_iter())
        .filter(|(path, _)| !(paths.iter()).any(|other| other != path && path.starts_with(other)))
        .collect()
}

/// What failed of an operation on several entries.
pub(super) struct Failure {
    /// What's said of the entry, as for it alone.
    message: SharedString,
    /// Why, if the message doesn't say.
    why: Option<String>,
    /// Whether it waits for something else, which is a warning, not an
    /// error.
    busy: bool,
}

impl Failure {
    /// `message`, and why, `error`.
    pub(super) fn error(message: SharedString, error: &io::Error) -> Self {
        Self {
            message,
            why: Some(capitalized(&error.to_string())),
            busy: false,
        }
    }

    /// `message`, which says why.
    pub(super) fn said(message: SharedString) -> Self {
        Self {
            message,
            why: None,
            busy: false,
        }
    }

    /// `message`, of an entry something else is busy with.
    pub(super) fn busy(message: SharedString) -> Self {
        Self {
            message,
            why: None,
            busy: true,
        }
    }

    /// Shows it in a toast, as for an operation on it alone.
    pub(super) fn show(self, cx: &mut gpui::App) {
        let toast = match (self.busy, self.why) {
            (true, _) => Toast::warning(self.message),
            (false, Some(why)) => Toast::error(self.message).detail(why),
            (false, None) => Toast::error(self.message),
        };
        toast::show(cx, toast);
    }

    /// Its line in the detail of a toast about several.
    fn line(&self) -> String {
        match &self.why {
            Some(why) => tr!(
                "workbench-project-failed-why",
                message = &self.message,
                error = why
            )
            .to_string(),
            None => self.message.to_string(),
        }
    }
}

/// Which operation on several entries failed for some.
#[derive(Clone, Copy)]
pub(super) enum Many {
    Trash,
    Duplicate,
    Paste,
    /// An undo or a redo.
    Undo,
}

/// Shows what failed of an operation on several entries: one failure as
/// for its entry alone, several in one toast, counted, its detail naming
/// each and why, a line apiece.
pub(super) fn report_failures(failures: Vec<Failure>, many: Many, cx: &mut gpui::App) {
    if failures.len() <= 1 {
        failures.into_iter().for_each(|failure| failure.show(cx));
        return;
    }
    let count = failures.len();
    let message = match many {
        Many::Trash => tr!("workbench-project-trash-failed-many", count = count),
        Many::Duplicate => tr!("workbench-project-duplicate-failed-many", count = count),
        Many::Paste => tr!("workbench-project-paste-failed-many", count = count),
        Many::Undo => tr!("workbench-project-undo-failed-many", count = count),
    };
    let lines: Vec<String> = failures.iter().map(Failure::line).collect();
    toast::show(cx, Toast::error(message).detail(lines.join("\n")));
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
/// typed: the name taken, or the name's own fault as the checks word it,
/// each said in the locale active when it's drawn, or else the system's
/// words as they came.
fn failure(error: &io::Error, name: &str) -> FieldError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        return text!("workbench-project-name-taken", name = name.to_owned()).into();
    }
    let refused = (error.get_ref()).and_then(|inner| inner.downcast_ref::<NameError>());
    match refused {
        Some(refused) => FieldError::Name(*refused),
        None => Text::verbatim(capitalized(&error.to_string())).into(),
    }
}

/// Those of `trashed` that can be put back, which an undo offers.
fn restorable(trashed: Vec<fs_ops::Trashed>) -> Vec<fs_ops::Trashed> {
    (trashed.into_iter())
        .filter(|trashed| trashed.location().is_some())
        .collect()
}

/// The device and inode of the entry at `path`, a symlink itself rather
/// than its target; `None` if there's none.
pub(super) fn entry_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    kiln::core::platform::path_identity(path, false).ok()
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
        &[PromptButton::ok(tr!("workbench-project-ok"))],
        cx,
    ));
}

#[cfg(test)]
impl ProjectPanel {
    /// The name field's text and error, while it's open.
    pub(crate) fn name_field(&self, cx: &gpui::App) -> Option<(String, Option<String>)> {
        let edit = self.edit.as_ref()?;
        let error = edit.error.as_ref().map(|error| error.resolve().to_string());
        Some((edit.input.read(cx).value().to_owned(), error))
    }

    /// The note under the name field, while it's open and a rename waits
    /// for language servers.
    pub(crate) fn name_field_note(&self) -> Option<SharedString> {
        self.edit.as_ref()?.waiting.as_ref().map(|_| renaming())
    }

    /// The name field's input, while it's open.
    pub(crate) fn name_input(&self) -> Option<Entity<TextField>> {
        Some(self.edit.as_ref()?.input.clone())
    }
}

#[cfg(test)]
#[path = "project_panel_ops_tests.rs"]
mod project_panel_ops_tests;
