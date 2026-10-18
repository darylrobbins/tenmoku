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
//! new or renamed row; a new file opens in the active pane, focused.

use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use fe_gpui::commands::RunCommand;
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
    /// Which field this is, of [`ProjectPanel::edits`].
    serial: u64,
    _subscriptions: [Subscription; 2],
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
    fn expand(&mut self, dir: NodeId, cx: &mut Context<Self>) {
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
    /// edit the field and move its caret, and the panel's other keys do
    /// nothing, so typing reaches only the field. Others go on to the
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

    /// Escape cancels the field.
    fn on_field_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.cancel_edit(window, cx);
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
        let (kind, serial, root) = (edit.kind, edit.serial, tree.root_path().to_owned());
        if kind == EditKind::Rename {
            if target.file_name().is_some_and(|old| *old == *name) {
                return self.cancel_edit(window, cx);
            }
            return self.rename(serial, root, target, &name, window, cx);
        }
        edit.running = true;
        edit.error = None;
        // All it makes: the folders the name names, and the entry.
        let running = Running::start(target.join(name.trim_end_matches('/')));
        let job = {
            let name = name.clone();
            let target = target.clone();
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
                    panel.edit_done(serial, kind, target, &name, named, window, cx)
                })
                .ok();
            drop(running);
        })
        .detach();
        cx.notify();
    }

    /// Renames `target`, under the tree's `root`, to `name`, on this thread,
    /// unless something would still act on the old path once it's done
    /// ([`Workbench::busy_under`]: a save, a Save As, a file being read, or
    /// another of the panel's operations), in any window: then the field
    /// says why and stays open. Done here, between saves, so none starts
    /// before the editors take the new path.
    fn rename(
        &mut self,
        serial: u64,
        root: PathBuf,
        target: PathBuf,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = target
            .strip_prefix(&root)
            .ok()
            .and_then(|rel| Some(self.root.as_ref()?.join(rel)));
        // The new path counts too: a copy or Save As may be about to write
        // there.
        let destination = (shown.as_ref()).and_then(|shown| Some(shown.parent()?.join(name)));
        let busy = (shown.as_ref())
            .and_then(|shown| Workbench::busy_under(shown, window, cx))
            .or_else(|| {
                let destination = destination.as_ref()?;
                crate::open::any_reserved().then_some(())?;
                Workbench::busy_under(destination, window, cx)
            });
        if let Some(busy) = busy {
            let old = target.file_name().unwrap_or_default().to_string_lossy();
            let error = match busy {
                Busy::Saving => format!("“{old}” is being saved. Rename it once it’s saved."),
                Busy::Opening => format!("“{old}” is being opened. Rename it once it’s open."),
                Busy::Working => {
                    format!("“{old}” is being worked on. Rename it once that’s done.")
                }
            };
            if let Some(edit) = self.edit.as_mut() {
                edit.error = Some(error);
            }
            return cx.notify();
        }
        // Planned while every spelling of the old path still resolves.
        let moves = shown
            .and_then(|from| Some((from.clone(), from.parent()?.join(name))))
            .map(|(from, to)| (Workbench::plan_rename(&from, &to, window, cx), from, to));
        let renamed = fs_ops::rename(&root, &target, name);
        if let (Ok(_), Some((moves, from, to))) = (&renamed, moves) {
            cx.emit(PanelEvent::Renamed { from, to, moves });
        }
        self.edit_done(serial, EditKind::Rename, target, name, renamed, window, cx);
    }

    /// Takes in what the name field's operation did to `target` (the folder
    /// a new entry went in, or the renamed entry): it shows a failure in
    /// the field, if the field's still open; otherwise closes the field,
    /// brings the tree up to date, and selects the entry, which opens if
    /// it's a new file.
    #[allow(clippy::too_many_arguments)]
    fn edit_done(
        &mut self,
        serial: u64,
        kind: EditKind,
        target: PathBuf,
        name: &str,
        named: Named,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let Some(tree) = self.tree() else {
            return;
        };
        let root = tree.root_path().to_owned();
        let shown = |path: &std::path::Path| {
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
            && tree
                .find(&target)
                .and_then(|id| tree.row(id))
                .is_some_and(|row| row.expanded);
        self.apply_fs_changes(&changes, cx);
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
        // What was asked about: an entry renamed meanwhile, and another put
        // at its path, isn't it.
        let asked = entry_identity(&path);
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
                        panel.changed(&path, cx);
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
                        if let Some(copy) = panel.changed(&copy, cx) {
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

    /// Brings the tree up to date with a change at `path`, under the tree's
    /// root, as the watcher's batch would, and returns the path as the panel
    /// hands it out.
    fn changed(&mut self, path: &std::path::Path, cx: &mut Context<Self>) -> Option<PathBuf> {
        let tree = self.tree()?;
        let rel = path.strip_prefix(tree.root_path()).ok()?;
        let shown = self.root.as_ref()?.join(rel);
        self.apply_fs_changes(&[FsChange::Path(shown.clone())], cx);
        Some(shown)
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
        // The field draws its own border; an error's is red.
        let border = edit.error.is_some().then_some(self.theme.error_text);
        let error = edit.error.clone().map(|error| {
            deferred(
                div()
                    .absolute()
                    .top(ROW_HEIGHT)
                    .left(px(4.))
                    .right(px(4.))
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(self.theme.error_text)
                    .bg(self.theme.panel_background)
                    .text_color(self.theme.error_text)
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
                    .text_color(self.theme.icon),
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
fn entry_identity(path: &std::path::Path) -> Option<(u64, u64)> {
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
    fn start(path: PathBuf) -> Arc<Running> {
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

/// The length of `name`'s stem, which a rename selects: up to its last
/// `.`, unless that's its first character (`.gitignore`) or there's none.
fn stem_len(name: &str) -> usize {
    name.rfind('.').filter(|&dot| dot > 0).unwrap_or(name.len())
}

/// `text` with its first letter in upper case.
fn capitalized(text: &str) -> String {
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

    /// The name field's input, while it's open.
    pub(crate) fn name_input(&self) -> Option<Entity<TextField>> {
        Some(self.edit.as_ref()?.input.clone())
    }
}

#[cfg(test)]
#[path = "project_panel_ops_tests.rs"]
mod project_panel_ops_tests;
