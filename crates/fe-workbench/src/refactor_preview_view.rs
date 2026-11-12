//! A language server's edit that reaches past the file it was asked in (a
//! rename's, a code action's), shown in the AI Changes panel to review
//! before it applies ([`Workbench::preview_or_apply`]).
//!
//! - **Shown.** The preview ([`RefactorPreview`]) takes the panel's place
//!   of a changeset, as a new turn's does: that changeset's proposals stay
//!   on their editors. The header reads `<title>: N files, M edits`, what
//!   Apply would make as decided so far, with Apply and Cancel; each file's
//!   row has its counts and Accept File and Reject File, which keep or
//!   leave out its hunks. A whole-only edit (one making file operations)
//!   has no per-file buttons, and lists its operations as rows too.
//! - **Per hunk.** Clicking a file's row opens a read-only review tab on a
//!   copy of the file's text as the preview has it, its pending hunks shown
//!   as proposal blocks. Their Reject (and an accept through the editor)
//!   decides the preview's own hunk, and the row's counts follow; a
//!   decision made from the panel shows in the tab. A whole-only edit's
//!   hunks refuse it, saying why.
//! - **Apply** closes the review tabs and applies what's kept
//!   ([`Workbench::apply_user_file_edits`]): each open buffer in one
//!   transaction, closed files written. Then what the asker wanted done
//!   after (a rename's caret, a code action's command) runs, and the panel
//!   has `No pending changes`. A refusal (a previewed buffer edited since)
//!   changes nothing, says why, and keeps the preview; a failure after
//!   part of the edit was made (a file that can't be written, an operation
//!   refused) says so, and ends the preview, as applying it again would
//!   make that part twice. **Cancel** closes the review tabs and leaves
//!   `No pending changes`, running nothing.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};

use fe_buffer::{Buffer, BufferId, HunkId, HunkStatus, Origin, ProposedChange, Version};
use fe_editor::{Changeset, Counts};
use fe_gpui::{EditorView, ProposalEvent};
use fe_lsp::{EditTarget, FileEdits};
use gpui::{
    App, Context, Entity, EntityId, Focusable as _, PromptButton, PromptLevel, Subscription, Window,
};
use kiln_l10n::tr;

use super::{AiChanges, ListedFile, Listing, PreviewHead, lines};
use crate::{
    Workbench, loader, lsp,
    workbench::toast::{self, Toast},
    workspace_edit::refactor_preview::{RefactorPreview, origin},
};

/// What runs once the edit is applied, given the edit that was.
pub(crate) type OnApplied =
    Box<dyn FnOnce(&mut Workbench, &[FileEdits], &mut Window, &mut Context<Workbench>)>;

/// A preview the AI Changes panel shows.
pub(crate) struct Review {
    preview: RefactorPreview,
    on_applied: Option<OnApplied>,
    /// The review tabs open.
    tabs: Vec<ReviewTab>,
    /// To the review tabs' proposal events.
    editors: HashMap<EntityId, Subscription>,
    /// Each closed file's text as the preview read it, which Apply checks
    /// is still the file's.
    read: Vec<(PathBuf, String)>,
}

/// A review tab: a copy of a previewed file's text, not to keep.
struct ReviewTab {
    path: PathBuf,
    buffer: BufferId,
    proposal: Rc<RefCell<ProposedChange>>,
    /// Each hunk of the copy's proposal, with the preview's it stands for.
    pairs: Vec<(HunkId, HunkId)>,
}

impl Review {
    /// The buffers of its review tabs.
    pub(super) fn buffers(&self) -> impl Iterator<Item = BufferId> + '_ {
        self.tabs.iter().map(|tab| tab.buffer)
    }
}

/// How many edits `target` makes: its text edits, or one operation.
fn edit_count(target: &EditTarget) -> usize {
    match target {
        EditTarget::Open { edits, .. } => edits.len(),
        EditTarget::Closed { edits, .. } => edits.len(),
        EditTarget::Depends { made, skipped, .. } => match (&**made, &**skipped) {
            (Ok(made), _) => edit_count(made),
            (_, Ok(skipped)) => edit_count(skipped),
            _ => 0,
        },
        _ => 1,
    }
}

/// What a row says of an operation of the edit, if `file` is one.
fn operation(file: &FileEdits, display: impl Fn(&Path) -> String) -> Option<String> {
    match &file.target {
        EditTarget::Create { .. } => Some(tr!("workbench-lsp-refactor-create").into()),
        EditTarget::Rename { to, .. } => {
            Some(tr!("workbench-lsp-refactor-rename-to", path = display(to)).into())
        }
        EditTarget::Delete { .. } => Some(tr!("workbench-lsp-refactor-delete").into()),
        _ => None,
    }
}

/// A file on disk: its inode, modified time and length.
type Stat = Option<(u64, SystemTime, u64)>;

/// What an edit could change, as it stands: each open buffer's version,
/// and each path's entry and the file it leads to, through a symlink.
#[derive(PartialEq, Eq)]
struct Marks {
    buffers: Vec<Option<Version>>,
    files: Vec<(Stat, Stat)>,
}

/// Whether `target` edits a closed file, a dependent one either way.
fn has_closed(target: &EditTarget) -> bool {
    match target {
        EditTarget::Closed { .. } => true,
        EditTarget::Depends { made, skipped, .. } => {
            [&**made, &**skipped].into_iter().flatten().any(has_closed)
        }
        _ => false,
    }
}

/// The text edits of `target` to open buffers, a dependent one's either
/// way: each buffer and the version they're for.
fn open_targets(target: &EditTarget, out: &mut Vec<(BufferId, Version)>) {
    match target {
        EditTarget::Open {
            buffer, version, ..
        } => out.push((*buffer, *version)),
        EditTarget::Depends { made, skipped, .. } => {
            for target in [&**made, &**skipped].into_iter().flatten() {
                open_targets(target, out);
            }
        }
        _ => {}
    }
}

/// Why an edit can't apply: `file`, an open buffer it edits, has closed.
fn no_longer_open(file: &impl std::fmt::Display) -> String {
    tr!(
        "workbench-lsp-refactor-no-longer-open",
        file = file.to_string()
    )
    .into()
}

/// Why an edit can't apply: `file` has changed since it was read.
fn changed_since(file: &impl std::fmt::Display) -> String {
    tr!(
        "workbench-lsp-refactor-changed-since",
        file = file.to_string()
    )
    .into()
}

/// Why an edit can't apply: `file` doesn't exist any more.
fn doesnt_exist(file: &impl std::fmt::Display) -> String {
    tr!(
        "workbench-lsp-refactor-doesnt-exist",
        file = file.to_string()
    )
    .into()
}

/// Why an edit can't apply: `file` can't be read.
fn cant_read(file: &impl std::fmt::Display, error: &impl std::fmt::Display) -> String {
    tr!(
        "workbench-lsp-refactor-cant-read",
        file = file.to_string(),
        error = error.to_string()
    )
    .into()
}

/// An info prompt saying `message`, answered by its one button.
fn say(message: &str, window: &mut Window, cx: &mut App) {
    drop(window.prompt(
        PromptLevel::Info,
        message,
        None,
        &[PromptButton::ok(tr!("workbench-lsp-ok"))],
        cx,
    ));
}

impl Workbench {
    /// Applies `edits`, asked for in `source`, if they change only it,
    /// then runs `on_applied`. Otherwise shows them in the AI Changes panel
    /// under `title`, to apply once reviewed, `on_applied` running only
    /// then. Why not: the edit refused, or the preview couldn't be made.
    pub(crate) fn preview_or_apply(
        &mut self,
        title: impl Into<String>,
        source: BufferId,
        edits: Vec<FileEdits>,
        on_applied: OnApplied,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !RefactorPreview::needs_preview(&edits, source) {
            self.apply_user_file_edits(edits.clone(), cx)?;
            on_applied(self, &edits, window, cx);
            return Ok(());
        }
        let others = Self::other_workbenches(cx);
        let mut read = Vec::new();
        let preview = RefactorPreview::new(title, edits, |path, target| {
            let text = self.preview_text(path, target, &others, cx)?;
            if !matches!(target, EditTarget::Open { .. }) {
                read.push((path.to_owned(), text.clone()));
            }
            Ok(text)
        })?;
        self.discard_ai_reviews(window, cx);
        let mut state = AiChanges::new(Changeset::new(preview.title(), Origin::Lsp));
        state.refactor = Some(Review {
            preview,
            on_applied: Some(on_applied),
            tabs: Vec::new(),
            editors: HashMap::new(),
            read,
        });
        self.ai_changes = Some(state);
        self.ai_changes_panel(cx);
        self.sync_ai_changes(cx);
        self.run_dock_op(crate::dock::DockOp::ShowAiChanges, window, cx);
        Ok(())
    }

    /// `path`'s text now, for the preview of an edit to it: its open
    /// buffer's, refused if that changed since the server's edits, or the
    /// file's as it reads.
    fn preview_text(
        &self,
        path: &Path,
        target: &EditTarget,
        others: &[Entity<Workbench>],
        cx: &App,
    ) -> Result<String, String> {
        let display = path.display();
        match target {
            EditTarget::Open {
                buffer, version, ..
            } => {
                let editor = (self.and_others(others, cx))
                    .find_map(|workbench| workbench.editors_of(*buffer, cx).into_iter().next())
                    .ok_or_else(|| no_longer_open(&display))?;
                let buffer = editor.read(cx).model().buffer();
                if buffer.version() != *version {
                    return Err(changed_since(&display));
                }
                Ok(buffer.text().to_string())
            }
            _ => match loader::read(path.to_owned()) {
                Ok(Some(buffer)) => Ok(buffer.text().to_string()),
                Ok(None) => Err(doesnt_exist(&display)),
                Err(error) => Err(cant_read(&display, &error)),
            },
        }
    }

    /// The review being shown, if the panel shows one.
    fn review(&mut self) -> Option<&mut Review> {
        self.ai_changes.as_mut()?.refactor.as_mut()
    }

    pub(super) fn reviewing(&self) -> bool {
        (self.ai_changes.as_ref()).is_some_and(|state| state.refactor.is_some())
    }

    /// Closes the tabs made to review the panel's changes: a turn's new
    /// files' and a preview's.
    pub(super) fn discard_ai_reviews(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let buffers: Vec<BufferId> = (self.ai_changes.as_ref())
            .map(|state| {
                let previews = state.previews.iter().map(|(_, buffer)| *buffer);
                let tabs = state.refactor.iter().flat_map(Review::buffers);
                previews.chain(tabs).collect()
            })
            .unwrap_or_default();
        if buffers.is_empty() {
            return;
        }
        let panes: Vec<_> = self.pane_group(cx).panes().into_iter().cloned().collect();
        for pane in panes {
            pane.update(cx, |pane, cx| pane.discard_buffers(&buffers, window, cx));
        }
        if let Some(review) = self.review() {
            review.tabs.clear();
            review.editors.clear();
        }
    }

    /// Why `edits`, the preview's to apply, can't be applied as reviewed,
    /// checked before any of it is made, as applying would refuse a target
    /// once it got there: an open buffer they edit changed, closed or was
    /// saved under another name since the server's edits; a file they edit
    /// closed was opened since, or a closed file's text isn't what the
    /// preview read (`read`, those of a whole-only preview all checked, as
    /// all of it applies). A path the edit's own renames move is checked
    /// where it is now.
    fn preflight(
        &self,
        edits: &[FileEdits],
        read: &[(PathBuf, String)],
        whole: bool,
        others: &[Entity<Workbench>],
        cx: &App,
    ) -> Result<(), String> {
        let opened: HashSet<PathBuf> = (self.and_others(others, cx))
            .flat_map(|workbench| workbench.opened_files(cx))
            .collect();
        let mut renames: Vec<(PathBuf, PathBuf)> = Vec::new();
        for file in edits {
            let display = file.path.display();
            let now = origin(&file.path, &renames);
            if let EditTarget::Rename { to, .. } = &file.target {
                renames.push((file.path.clone(), to.clone()));
            }
            let mut targets = Vec::new();
            open_targets(&file.target, &mut targets);
            for (buffer, version) in targets {
                let editor = (self.and_others(others, cx))
                    .find_map(|workbench| workbench.editors_of(buffer, cx).into_iter().next())
                    .ok_or_else(|| no_longer_open(&display))?;
                let model = editor.read(cx).model();
                if model.buffer().version() != version {
                    return Err(changed_since(&display));
                }
                let shown = model.path().and_then(lsp::resolve);
                if shown.is_none() || shown != lsp::resolve(&now) {
                    return Err(tr!(
                        "workbench-lsp-refactor-saved-under-another-name",
                        file = display.to_string()
                    )
                    .into());
                }
            }
            // Where it is now, and where the edit's renames have it then.
            let open = [now.as_path(), file.path.as_path()]
                .into_iter()
                .any(|path| {
                    opened.contains(path) || lsp::resolve(path).is_some_and(|p| opened.contains(&p))
                });
            if has_closed(&file.target) && open {
                return Err(tr!(
                    "workbench-lsp-refactor-opened-since",
                    file = display.to_string()
                )
                .into());
            }
        }
        let applied: HashSet<&Path> = edits.iter().map(|file| file.path.as_path()).collect();
        for (path, text) in read {
            if !whole && !applied.contains(path.as_path()) {
                continue;
            }
            let display = path.display();
            match loader::read(path.clone()) {
                Ok(Some(buffer)) if buffer.text().to_string() == *text => {}
                Ok(Some(_)) => return Err(changed_since(&display)),
                Ok(None) => return Err(doesnt_exist(&display)),
                Err(error) => return Err(cant_read(&display, &error)),
            }
        }
        Ok(())
    }

    /// What `edits` could change, as it stands now.
    fn marks(&self, edits: &[FileEdits], others: &[Entity<Workbench>], cx: &App) -> Marks {
        let mut buffers = Vec::new();
        let mut paths = Vec::new();
        for file in edits {
            let mut targets = Vec::new();
            open_targets(&file.target, &mut targets);
            buffers.extend(targets.into_iter().map(|(buffer, _)| buffer));
            paths.push(file.path.as_path());
            if let EditTarget::Rename { to, .. } = &file.target {
                paths.push(to);
            }
        }
        let version = |buffer: BufferId| {
            (self.and_others(others, cx))
                .find_map(|workbench| workbench.editors_of(buffer, cx).into_iter().next())
                .map(|editor| editor.read(cx).model().buffer().version())
        };
        let stat = |path: &Path, follow: bool| {
            let meta = if follow {
                fs::metadata(path)
            } else {
                fs::symlink_metadata(path)
            };
            let meta = meta.ok()?;
            let (_, file) = kiln::core::platform::path_identity(path, follow).ok()?;
            Some((file, meta.modified().ok()?, meta.len()))
        };
        // A closed file is written where its symlinks lead.
        let file = |path: &Path| (stat(path, false), stat(path, true));
        Marks {
            buffers: buffers.into_iter().map(version).collect(),
            files: paths.into_iter().map(file).collect(),
        }
    }

    /// Applies what the preview keeps, then what was to run after. A
    /// refusal that changed nothing (a previewed file or buffer edited
    /// since, checked first) is said, and keeps the preview; one that came
    /// after part of the edit was made closes it too, as applying it again
    /// would make that part twice.
    pub(crate) fn apply_refactor_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = self.review() else {
            return;
        };
        let edits = review.preview.selected();
        let (read, whole) = (
            std::mem::take(&mut review.read),
            !review.preview.is_decidable(),
        );
        let others = Self::other_workbenches(cx);
        let checked = self.preflight(&edits, &read, whole, &others, cx);
        if let Some(review) = self.review() {
            review.read = read;
        }
        if let Err(message) = checked {
            return say(&message, window, cx);
        }
        let before = self.marks(&edits, &others, cx);
        match self.apply_user_file_edits(edits.clone(), cx) {
            Ok(_) => {
                let on_applied = self.review().and_then(|review| review.on_applied.take());
                self.discard_ai_reviews(window, cx);
                self.ai_changes = None;
                self.sync_ai_changes(cx);
                if let Some(on_applied) = on_applied {
                    on_applied(self, &edits, window, cx);
                }
            }
            Err(message) if self.marks(&edits, &others, cx) == before => {
                self.sync_ai_changes(cx);
                say(&message, window, cx);
            }
            Err(message) => {
                self.discard_ai_reviews(window, cx);
                self.ai_changes = None;
                self.sync_ai_changes(cx);
                say(
                    &tr!("workbench-lsp-refactor-partly-made", message = message),
                    window,
                    cx,
                );
            }
        }
    }

    /// Leaves the preview unapplied, running nothing.
    pub(crate) fn cancel_refactor_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.reviewing() {
            return;
        }
        self.discard_ai_reviews(window, cx);
        self.ai_changes = None;
        self.sync_ai_changes(cx);
    }

    /// Keeps or leaves out every pending hunk of `path`.
    pub(super) fn decide_refactor_file(
        &mut self,
        path: &Path,
        accept: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(review) = self.review() else {
            return;
        };
        let decided = match accept {
            true => review.preview.accept_file(path),
            false => review.preview.reject_file(path),
        };
        if let Err(error) = decided {
            toast::show(cx, Toast::info(error.to_string()));
        }
        self.sync_ai_changes(cx);
    }

    /// Keeps or leaves out every pending hunk of the preview.
    pub(super) fn decide_refactor_all(&mut self, accept: bool, cx: &mut Context<Self>) {
        let Some(review) = self.review() else {
            return;
        };
        let decided = match accept {
            true => review.preview.accept_all(),
            false => review.preview.reject_all(),
        };
        let errors: Vec<String> = match decided {
            Ok(results) => (results.into_iter())
                .filter_map(|(_, result)| result.err())
                .map(|error| error.to_string())
                .collect(),
            Err(error) => vec![error.to_string()],
        };
        if !errors.is_empty() {
            toast::show(cx, Toast::info(errors.join("\n")));
        }
        self.sync_ai_changes(cx);
    }

    /// Opens a review tab on `path`, a previewed file, at its first pending
    /// hunk, or shows the one open.
    pub(super) fn open_review_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors = self.open_editors(cx);
        let Some(review) = self.review() else {
            return;
        };
        let open = (review.tabs.iter())
            .filter(|tab| tab.path == path)
            .find_map(|tab| {
                (editors.iter()).find(|editor| editor.read(cx).model().buffer().id() == tab.buffer)
            })
            .cloned();
        let editor = match open {
            Some(editor) => {
                let panes: Vec<_> = self.pane_group(cx).panes().into_iter().cloned().collect();
                for pane in panes {
                    let at =
                        (pane.read(cx).tabs().iter()).position(|tab| tab.editor() == Some(&editor));
                    if let Some(at) = at {
                        pane.update(cx, |pane, cx| pane.activate(at, window, cx));
                        break;
                    }
                }
                editor
            }
            None => {
                let Some(text) = (review.preview.buffer(&path)).map(|b| b.text().to_string())
                else {
                    return;
                };
                let hunks: Vec<_> = (review.preview.hunks(&path).into_iter())
                    .filter(|hunk| hunk.status != HunkStatus::Accepted)
                    .collect();
                let copy = Buffer::new(&text);
                let mut proposal = ProposedChange::from_edits_keeping_noops(
                    copy.current(),
                    (hunks.iter()).map(|hunk| (hunk.range.clone(), &*hunk.new_text)),
                    Origin::Lsp,
                );
                let ids: Vec<HunkId> = (proposal.hunks(copy.current()).into_iter())
                    .map(|hunk| hunk.id)
                    .collect();
                // Made from the preview's hunks, apart and in order, it has
                // as many; if not, it isn't shown, rather than mispaired.
                let pairs: Vec<(HunkId, HunkId)> = match ids.len() == hunks.len() {
                    true => ids.into_iter().zip(hunks.iter().map(|h| h.id)).collect(),
                    false => Vec::new(),
                };
                for ((copied, _), hunk) in pairs.iter().zip(&hunks) {
                    if hunk.status == HunkStatus::Rejected {
                        proposal.reject(*copied).ok();
                    }
                }
                let proposal = Rc::new(RefCell::new(proposal));
                review.tabs.push(ReviewTab {
                    path: path.clone(),
                    buffer: copy.id(),
                    proposal: proposal.clone(),
                    pairs: pairs.clone(),
                });
                // No file, so nothing saves it there or opens it on a
                // server; named and highlighted as the file is.
                let start = text
                    .get(..text.floor_char_boundary(512))
                    .unwrap_or_default();
                let language = fe_syntax::detect_language_name(Some(&path), start);
                let name = fe_gpui::pane_item::file_name(Some(&path));
                let model = fe_editor::EditorModel::new(copy, None);
                let pane = self.pane().clone();
                pane.update(cx, |pane, cx| pane.add(model, cx));
                let Some(editor) = pane.read(cx).active_editor().cloned() else {
                    return;
                };
                editor.update(cx, |editor, cx| {
                    editor.set_unsavable(true);
                    editor.set_tab_title(
                        tr!("workbench-lsp-refactor-tab-title", name = &name),
                        name,
                        cx,
                    );
                    editor.set_language(language, cx);
                });
                editor
            }
        };
        self.sync_ai_changes(cx);
        editor.update(cx, |editor, cx| {
            let hunks = (editor.proposals().first())
                .map(|proposal| proposal.borrow().hunks(editor.model().buffer().current()))
                .unwrap_or_default();
            let first = (hunks.iter())
                .filter(|hunk| hunk.status == HunkStatus::Pending)
                .map(|hunk| hunk.range.start)
                .min();
            if let Some(at) = first {
                editor.select_and_reveal(at..at, cx);
            }
        });
        window.focus(&editor.focus_handle(cx), cx);
    }

    /// A review tab's hunk decided in its editor: the preview's hunk it
    /// stands for decided alike. One the preview refuses is put back.
    fn review_decided(
        &mut self,
        editor: &Entity<EditorView>,
        event: &ProposalEvent,
        cx: &mut Context<Self>,
    ) {
        let buffer = editor.read(cx).model().buffer().id();
        let (copied, accept) = match event {
            ProposalEvent::Accepted { hunk, .. } | ProposalEvent::AcceptedNoChange { hunk } => {
                (*hunk, true)
            }
            ProposalEvent::Rejected { hunk } => (*hunk, false),
        };
        let Some(review) = self.review() else {
            return;
        };
        let Some(tab) = review.tabs.iter().find(|tab| tab.buffer == buffer) else {
            return;
        };
        let Some(&(_, theirs)) = tab.pairs.iter().find(|(copy, _)| *copy == copied) else {
            return;
        };
        let (path, proposal) = (tab.path.clone(), tab.proposal.clone());
        let wanted = if accept {
            HunkStatus::Accepted
        } else {
            HunkStatus::Rejected
        };
        let status = (review.preview.hunks(&path).into_iter())
            .find(|hunk| hunk.id == theirs)
            .map(|hunk| hunk.status);
        if status != Some(wanted) {
            let decided = match accept {
                true => review.preview.accept_hunk(&path, theirs),
                false => review.preview.reject_hunk(&path, theirs),
            };
            if let Err(error) = decided {
                if !accept {
                    proposal.borrow_mut().reset_to_pending(copied).ok();
                    editor.update(cx, |_, cx| cx.notify());
                }
                toast::show(cx, Toast::info(error.to_string()));
            }
        }
        self.sync_ai_changes(cx);
    }

    /// Lists the preview into the panel again: its review tabs closed
    /// since forgotten, those open kept read-only, followed, and showing
    /// the preview's decisions.
    pub(super) fn sync_refactor_preview(&mut self, cx: &mut Context<Self>) {
        let panel = self.ai_changes_panel(cx);
        let root = self.root.clone();
        let editors = self.open_editors(cx);
        let Some(review) = self.review() else {
            return;
        };
        let shown: HashSet<BufferId> = (editors.iter())
            .map(|editor| editor.read(cx).model().buffer().id())
            .collect();
        review.tabs.retain(|tab| shown.contains(&tab.buffer));
        let tabs: HashSet<BufferId> = review.buffers().collect();
        let live: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        review.editors.retain(|id, _| live.contains(id));
        let mut accepts = Vec::new();
        for editor in &editors {
            let buffer = editor.read(cx).model().buffer().id();
            if !tabs.contains(&buffer) {
                continue;
            }
            let Some(tab) = review.tabs.iter().find(|tab| tab.buffer == buffer) else {
                continue;
            };
            // Every view of it, a split too: read-only but for its hunks'
            // decisions (accepted only if the preview can be decided hunk
            // by hunk), its proposal shown, never saved anywhere, and
            // never dirty, so nothing asks to save it or backs it up.
            let decidable = review.preview.is_decidable();
            editor.update(cx, |editor, cx| {
                if !editor.is_read_only() {
                    editor.set_read_only(true, cx);
                }
                editor.set_accepts_when_read_only(decidable, cx);
                editor.set_unsavable(true);
                let shown = (editor.proposals().iter()).any(|p| Rc::ptr_eq(p, &tab.proposal));
                if !shown && !tab.pairs.is_empty() {
                    editor.set_proposals(vec![tab.proposal.clone()], cx);
                }
                if editor.model().is_dirty() {
                    editor.model_mut().buffer_mut().did_save();
                }
            });
            review.editors.entry(editor.entity_id()).or_insert_with(|| {
                cx.subscribe(editor, |workbench, editor, event: &ProposalEvent, cx| {
                    workbench.review_decided(&editor, event, cx)
                })
            });
            // A decision made from the panel shows in the tab.
            let theirs: HashMap<HunkId, HunkStatus> = (review.preview.hunks(&tab.path))
                .into_iter()
                .map(|hunk| (hunk.id, hunk.status))
                .collect();
            let copied: HashMap<HunkId, HunkStatus> = (tab.proposal.borrow())
                .hunks(editor.read(cx).model().buffer().current())
                .into_iter()
                .map(|hunk| (hunk.id, hunk.status))
                .collect();
            let mut changed = false;
            for (copy, preview) in &tab.pairs {
                let open = matches!(
                    copied.get(copy),
                    Some(HunkStatus::Pending | HunkStatus::Stale)
                );
                match theirs.get(preview) {
                    Some(HunkStatus::Rejected) if open => {
                        changed |= tab.proposal.borrow_mut().reject(*copy).is_ok();
                    }
                    Some(HunkStatus::Accepted) if open => accepts.push((editor.clone(), *copy)),
                    _ => {}
                }
            }
            if changed {
                editor.update(cx, |_, cx| cx.notify());
            }
        }
        let display = |path: &Path| {
            let relative = (root.as_deref()).and_then(|root| path.strip_prefix(root).ok());
            relative.unwrap_or(path).display().to_string()
        };
        let preview = &mut review.preview;
        let entries = preview.files();
        let summary = preview.summary();
        let selected = preview.selected();
        let head = PreviewHead {
            title: preview.title().to_owned(),
            files: (selected.iter())
                .map(|file| file.path.as_path())
                .collect::<HashSet<_>>()
                .len(),
            edits: selected.iter().map(|file| edit_count(&file.target)).sum(),
            decidable: preview.is_decidable(),
        };
        let mut by_path: HashMap<PathBuf, ListedFile> = (entries.into_iter())
            .map(|entry| {
                let hunks = preview.hunks(&entry.path);
                let added = hunks.iter().map(|hunk| lines(&hunk.new_text)).sum();
                let removed = hunks.iter().map(|hunk| lines(&hunk.old_text)).sum();
                let file = ListedFile {
                    path: entry.path.clone(),
                    is_new: entry.is_new,
                    counts: entry.counts,
                    added,
                    removed,
                    operation: None,
                };
                (entry.path, file)
            })
            .collect();
        // In the edit's order: its files where first edited, and its
        // operations.
        let mut files = Vec::new();
        for file in preview.edits() {
            match operation(file, display) {
                Some(operation) => files.push(ListedFile {
                    path: file.path.clone(),
                    is_new: false,
                    counts: Counts::default(),
                    added: 0,
                    removed: 0,
                    operation: Some(operation),
                }),
                None => files.extend(by_path.remove(&file.path)),
            }
        }
        let listing = Listing {
            files,
            summary,
            preview: Some(head),
        };
        panel.update(cx, |panel, cx| panel.set_listing(root, listing, cx));
        for (editor, hunk) in accepts {
            editor.update(cx, |editor, cx| editor.accept_hunk(hunk, cx).ok());
        }
    }
}

#[cfg(test)]
#[path = "refactor_preview_view_tests.rs"]
mod tests;
