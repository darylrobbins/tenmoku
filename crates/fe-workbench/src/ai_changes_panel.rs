//! The AI Changes panel in the sidebar dock: the files one AI turn proposed
//! edits to (a [`fe_editor::Changeset`]), reviewed together. The header says
//! `N files, M hunks pending` and has Accept All, Reject All and Revert Turn
//! (disabled until a hunk has been accepted); a row per file has its icon,
//! its path from the root, the lines its hunks add and remove, how many
//! hunks are pending, accepted and rejected, and Accept File and Reject File
//! buttons (`ai-changes-accept-<path>`, `ai-changes-reject-<path>`).
//!
//! Clicking a row, or Enter on it, opens the file in the active pane, shows
//! the file's proposal in its editor and moves to its first pending hunk;
//! the editor's own buttons then decide hunks one by one, and the counts
//! follow its [`fe_gpui::ProposalEvent`]s. The header's buttons run the
//! commands `ai.changes_accept_all`, `ai.changes_reject_all` and
//! `ai.changes_revert`; `ai.show_changes` shows the panel, and
//! `ai.changes_next_file` and `ai.changes_previous_file` move the
//! selection while the panel has focus ([`KEY_CONTEXT`]).
//!
//! The workbench owns the changeset ([`Workbench::set_changeset`], one at a
//! time) and lists it again into the panel after every decision. A
//! changeset with everything decided and nothing accepted leaves the panel
//! with `No pending changes`; one with accepted hunks keeps its rows so the
//! turn can still be reverted. Reverting a file edited since asks first,
//! naming the files, and only then overwrites them.
//!
//! The panel is in the sidebar beside Problems, joining the rail once it
//! has shown, as Problems does: the only bottom dock is the terminal's,
//! which draws no other panel.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{BufferId, HunkId, HunkStatus, ProposedChange, TransactionId, Version};
use fe_command::Source;
use fe_editor::{ChangesetError, Counts, FileEntry, RevertOutcome, Summary};
use fe_gpui::{EditorView, ProposalEvent};
use fe_ui::{Button, Icon, ROW_HEIGHT, file_icon};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, EntityId, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, PromptButton,
    PromptLevel, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, UniformListScrollHandle, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, svg, uniform_list,
};

use crate::{
    Pane, Workbench,
    ai_changes_buffers::{Made, with_editors},
    workbench::toast::{self, Toast},
};
use fe_editor::Changeset;

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "ai_changes";

/// The command that shows the panel.
pub(crate) const SHOW: &str = "ai.show_changes";

/// What the panel shows when nothing is pending.
const NO_CHANGES: &str = "No pending changes";

/// The AI changes in the sidebar: on the rail once it has shown; focused
/// when shown, for its keys.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("ai_changes"),
    name: "AI Changes",
    icon: Icon::FileDiff,
    show_command: SHOW,
    show_op: crate::dock::DockOp::ShowAiChanges,
    order: 75,
    always_on_rail: false,
    focus_on_show: true,
};

/// What an AI changes command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiChangesOp {
    AcceptAll,
    RejectAll,
    Revert,
    NextFile,
    PreviousFile,
}

/// A file of the changeset as the panel lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListedFile {
    pub(crate) path: PathBuf,
    pub(crate) is_new: bool,
    pub(crate) counts: Counts,
    /// Lines the hunks add and remove.
    pub(crate) added: usize,
    pub(crate) removed: usize,
}

/// What the workbench lists into the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Listing {
    pub(crate) files: Vec<ListedFile>,
    pub(crate) summary: Summary,
}

/// The workbench's active changeset and what it needs to reach buffers.
pub(crate) struct AiChanges {
    pub(crate) changeset: Changeset,
    /// Which changeset this is, among those the workbench has shown.
    generation: u64,
    made: Made,
    /// Lines added and removed per file, as last read.
    lines: HashMap<PathBuf, (usize, usize)>,
    /// To the open editors' proposal events.
    editors: HashMap<EntityId, Subscription>,
    /// How many reopened files were last told of as not followed.
    told: usize,
    /// A file being read for a row that was clicked.
    opening: Option<Subscription>,
    /// The buffers of tabs showing new files still pending, by file: made
    /// to review them, not to keep. A decision closes them.
    previews: Vec<(PathBuf, BufferId)>,
}

impl AiChanges {
    pub(crate) fn new(changeset: Changeset) -> Self {
        static GENERATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            changeset,
            generation: GENERATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            made: Made::default(),
            lines: HashMap::new(),
            editors: HashMap::new(),
            told: 0,
            opening: None,
            previews: Vec::new(),
        }
    }
}

pub(crate) struct AiChangesPanel {
    workbench: WeakEntity<Workbench>,
    focus: FocusHandle,
    root: Option<PathBuf>,
    listing: Listing,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
}

impl AiChangesPanel {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut App) -> Self {
        Self {
            workbench,
            focus: cx.focus_handle(),
            root: None,
            listing: Listing::default(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// Lists `listing`, with paths relative to `root`, keeping the selected
    /// file selected if it's still listed.
    pub(crate) fn set_listing(
        &mut self,
        root: Option<PathBuf>,
        listing: Listing,
        cx: &mut Context<Self>,
    ) {
        if self.listing == listing && self.root == root {
            return;
        }
        let selected = self.selected_path().map(Path::to_owned);
        self.selected = selected
            .and_then(|path| listing.files.iter().position(|file| file.path == path))
            .or_else(|| (self.selected.is_some() && !listing.files.is_empty()).then_some(0));
        self.root = root;
        self.listing = listing;
        cx.notify();
    }

    fn selected_path(&self) -> Option<&Path> {
        let file = self.listing.files.get(self.selected?)?;
        Some(&file.path)
    }

    /// Selects the next or previous file, wrapping; with none selected, the
    /// first or the last.
    pub(crate) fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.listing.files.len();
        if count == 0 {
            return;
        }
        let to = match self.selected {
            None if forward => 0,
            None => count - 1,
            Some(selected) if forward => (selected + 1) % count,
            Some(selected) => (selected + count - 1) % count,
        };
        self.select(to, cx);
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Selects file `ix` and opens it.
    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.listing.files.get(ix) else {
            return;
        };
        let path = file.path.clone();
        self.select(ix, cx);
        // The workbench lists into this panel as it opens the file.
        let workbench = self.workbench.clone();
        window.defer(cx, move |window, cx| {
            (workbench.update(cx, |workbench, cx| workbench.open_ai_file(path, window, cx))).ok();
        });
    }

    fn key_context(&self) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        context
    }

    /// `path` as a file row shows it: from the root, if it's under it.
    fn display_path(&self, path: &Path) -> String {
        let relative = (self.root.as_deref()).and_then(|root| path.strip_prefix(root).ok());
        relative.unwrap_or(path).display().to_string()
    }

    /// What shows in place of the rows, if anything.
    fn message(&self) -> Option<&'static str> {
        let counts = &self.listing.summary.counts;
        (self.listing.files.is_empty()
            || (counts.pending == 0 && counts.accepted == 0 && counts.stale == 0))
            .then_some(NO_CHANGES)
    }

    fn header(&self) -> String {
        let summary = &self.listing.summary;
        let files = match summary.files {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        let hunks = match summary.counts.pending {
            1 => "1 hunk".to_owned(),
            n => format!("{n} hunks"),
        };
        format!("{files}, {hunks} pending")
    }

    fn row_counts(counts: &Counts) -> String {
        let parts: Vec<String> = [
            (counts.pending, "pending"),
            (counts.accepted, "accepted"),
            (counts.rejected, "rejected"),
            (counts.stale, "stale"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        parts.join(", ")
    }

    fn header_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let can_revert = self.listing.summary.counts.accepted > 0;
        let button = |id: &'static str, label: &'static str, command: &'static str, on: bool| {
            div().flex_none().debug_selector(move || id.into()).child(
                Button::new(id, label)
                    .small()
                    .disabled(!on)
                    .on_click(move |_, window, cx| {
                        crate::pane_toolbar::dispatch(command, Source::Mouse, window, cx)
                    }),
            )
        };
        let _ = cx;
        div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(button(
                "ai-changes-accept-all",
                "Accept All",
                "ai.changes_accept_all",
                true,
            ))
            .child(button(
                "ai-changes-reject-all",
                "Reject All",
                "ai.changes_reject_all",
                true,
            ))
            .child(button(
                "ai-changes-revert",
                "Revert Turn",
                "ai.changes_revert",
                can_revert,
            ))
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ui = fe_ui::ui_theme(cx);
        let selected_bg = if self.focus.contains_focused(window, cx) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        range
            .filter_map(|ix| {
                let file = self.listing.files.get(ix)?;
                let shown = self.display_path(&file.path);
                let name = file.path.file_name().unwrap_or_default().to_string_lossy();
                let icon = svg()
                    .path(file_icon(&name).path())
                    .size(px(14.))
                    .flex_none()
                    .text_color(ui.icon);
                let decided = |counts: &Counts| counts.pending == 0 && counts.stale == 0;
                let can_decide = !decided(&file.counts);
                let buttons = [("accept", "Accept", true), ("reject", "Reject", false)]
                    .into_iter()
                    .map(|(what, label, accept)| {
                        let selector = format!("ai-changes-{what}-{shown}");
                        let id = SharedString::from(selector.clone());
                        let workbench = self.workbench.clone();
                        let path = file.path.clone();
                        div().flex_none().debug_selector(move || selector).child(
                            Button::new(id, label)
                                .small()
                                .disabled(!can_decide && what == "accept")
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    (workbench.update(cx, |workbench, cx| {
                                        workbench.decide_ai_file(&path, accept, window, cx)
                                    }))
                                    .ok();
                                }),
                        )
                    });
                Some(
                    div()
                        .id(ElementId::Name(format!("ai-changes-row-{shown}").into()))
                        .debug_selector({
                            let selector = format!("ai-changes-row-{shown}");
                            move || selector
                        })
                        .h(ROW_HEIGHT * 2.)
                        .w_full()
                        .px_3()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when(self.selected == Some(ix), |row| row.bg(selected_bg))
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(
                            cx.listener(move |panel, _, window, cx| panel.open(ix, window, cx)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(icon)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(shown.clone()),
                                )
                                .children(buttons),
                        )
                        .child(
                            div()
                                .pl(px(22.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(ui.muted_text)
                                .child(format!(
                                    "+{} -{}  {}",
                                    file.added,
                                    file.removed,
                                    Self::row_counts(&file.counts)
                                )),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl Focusable for AiChangesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AiChangesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let message = self.message();
        let count = if message.is_some() {
            0
        } else {
            self.listing.files.len()
        };
        let list = uniform_list(
            "ai-changes-rows",
            count,
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
            .track_focus(&self.focus)
            .key_context(self.key_context())
            .on_key_down(
                cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter"
                        && event.keystroke.modifiers.number_of_modifiers() == 0
                        && let Some(ix) = panel.selected
                    {
                        panel.open(ix, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .debug_selector(|| "sidebar-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(ui.border)
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(ui.muted_text).child(self.header()))
                    .child(self.header_buttons(cx)),
            )
            .when_some(message, |panel, message| {
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_color(ui.muted_text)
                        .child(message),
                )
            })
            .child(list)
    }
}

/// How many lines `text` has.
fn lines(text: &str) -> usize {
    text.lines().count()
}

impl Workbench {
    /// Shows `changeset`, a turn's proposals, in the AI Changes panel, made
    /// and shown now, in place of any before it: the old changeset's
    /// proposals stay on their editors, whatever was pending, and only the
    /// list is replaced.
    #[allow(dead_code)] // Card 13's chat turns call it.
    pub(crate) fn set_changeset(
        &mut self,
        changeset: Changeset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The turn it replaces leaves no review tabs behind.
        let previews: Vec<BufferId> = (self.ai_changes.as_ref())
            .map(|state| state.previews.iter().map(|(_, buffer)| *buffer).collect())
            .unwrap_or_default();
        if !previews.is_empty() {
            for pane in self
                .pane_group(cx)
                .panes()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                pane.update(cx, |pane, cx| pane.discard_buffers(&previews, window, cx));
            }
        }
        self.ai_changes = Some(AiChanges::new(changeset));
        self.ai_changes_panel(cx);
        self.sync_ai_changes(cx);
        self.run_dock_op(crate::dock::DockOp::ShowAiChanges, window, cx);
    }

    /// The panel, made and registered in the dock the first time.
    pub(crate) fn ai_changes_panel(&mut self, cx: &mut Context<Self>) -> Entity<AiChangesPanel> {
        if let Some(panel) = self.dock().read(cx).ai_changes().cloned() {
            return panel;
        }
        let workbench = cx.weak_entity();
        let panel = cx.new(|cx| AiChangesPanel::new(workbench, cx));
        self.dock()
            .clone()
            .update(cx, |dock, cx| dock.set_ai_changes(panel.clone(), cx));
        panel
    }

    /// The open editors, one per buffer.
    fn ai_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
        self.open_editors(cx)
    }

    /// Lists the changeset into the panel again, and follows the proposal
    /// events of the open editors. Each editor showing a file of the
    /// changeset gets its proposal, and a file reopened after its tab was
    /// closed is followed to its new buffer.
    pub(crate) fn sync_ai_changes(&mut self, cx: &mut Context<Self>) {
        let panel = self.ai_changes_panel(cx);
        let root = self.root.clone();
        let editors = self.ai_editors(cx);
        let Some(state) = self.ai_changes.as_mut() else {
            panel.update(cx, |panel, cx| {
                panel.set_listing(root, Listing::default(), cx)
            });
            return;
        };
        let live: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        state.editors.retain(|id, _| live.contains(id));
        for editor in &editors {
            state.editors.entry(editor.entity_id()).or_insert_with(|| {
                cx.subscribe(editor, |workbench, editor, event: &ProposalEvent, cx| {
                    // The accept is filed under the checkpoint before
                    // anything is armed again.
                    match event {
                        ProposalEvent::Accepted { hunk, transaction } => {
                            workbench.adopt_ai_accept(&editor, *hunk, Some(*transaction), cx)
                        }
                        ProposalEvent::AcceptedNoChange { hunk } => {
                            workbench.adopt_ai_accept(&editor, *hunk, None, cx)
                        }
                        ProposalEvent::Rejected { .. } => {}
                    }
                    workbench.sync_ai_changes(cx)
                })
            });
        }
        // Each editor with the file it shows, if it has one: tabs without a
        // path stay in step with the editors.
        let shown: Vec<Option<(PathBuf, BufferId)>> = (editors.iter())
            .map(|editor| {
                let model = editor.read(cx).model();
                Some((model.path()?.to_owned(), model.buffer().id()))
            })
            .collect();
        let listing = with_editors(&editors, &mut state.made, cx, |set| {
            let mut conflicts = Vec::new();
            for (path, id) in shown.iter().flatten() {
                let Some((held, _)) = state.changeset.proposal(path) else {
                    continue;
                };
                if held != *id
                    && fe_editor::BufferSet::buffer_mut(set, held).is_none()
                    && let Err(error) = state.changeset.rebind(set, path, *id)
                {
                    conflicts.push(error);
                }
            }
            state.changeset.observe(set);
            let entries: Vec<FileEntry> = state.changeset.files(set);
            for entry in &entries {
                state.changeset.arm(set, &entry.path);
            }
            let summary = state.changeset.summary(set);
            let files = entries
                .into_iter()
                .map(|entry| {
                    let read = state.changeset.proposal(&entry.path).and_then(
                        |(buffer, proposal): (BufferId, Rc<RefCell<ProposedChange>>)| {
                            let buffer = fe_editor::BufferSet::buffer_mut(set, buffer)?;
                            let hunks = proposal.borrow().hunks(buffer.current());
                            Some((
                                hunks
                                    .iter()
                                    .map(|hunk| lines(&hunk.new_text))
                                    .sum::<usize>(),
                                hunks
                                    .iter()
                                    .map(|hunk| lines(&hunk.old_text))
                                    .sum::<usize>(),
                            ))
                        },
                    );
                    let read = read.or_else(|| {
                        (state.changeset.new_file_text(&entry.path)).map(|text| (lines(text), 0))
                    });
                    if let Some(read) = read {
                        state.lines.insert(entry.path.clone(), read);
                    }
                    let (added, removed) = state.lines.get(&entry.path).copied().unwrap_or((0, 0));
                    ListedFile {
                        path: entry.path,
                        is_new: entry.is_new,
                        counts: entry.counts,
                        added,
                        removed,
                    }
                })
                .collect();
            (Listing { files, summary }, conflicts)
        });
        let (listing, conflicts) = listing;
        let attach: Vec<(Entity<EditorView>, Rc<RefCell<ProposedChange>>)> = (editors.iter())
            .zip(&shown)
            .filter_map(|(editor, shown)| {
                let (path, id) = shown.as_ref()?;
                let (held, proposal) = state.changeset.proposal(path)?;
                let shown = editor.read(cx).proposals();
                let missing = !shown.iter().any(|shown| Rc::ptr_eq(shown, &proposal));
                (held == *id && missing).then(|| (editor.clone(), proposal))
            })
            .collect();
        panel.update(cx, |panel, cx| panel.set_listing(root, listing, cx));
        // A reopened file whose text is not the one the turn proposed to is
        // not followed: it keeps no proposal, and says so once per sync.
        if !conflicts.is_empty() && state.told != conflicts.len() {
            state.told = conflicts.len();
            Self::ai_conflicts(conflicts, cx);
        }
        for (editor, proposal) in attach {
            editor.update(cx, |editor, cx| {
                let mut proposals = editor.proposals().to_vec();
                proposals.push(proposal);
                editor.set_proposals(proposals, cx);
            });
        }
    }

    /// Files the accept `editor` made under the changeset's checkpoint, so
    /// Revert Turn covers it.
    fn adopt_ai_accept(
        &mut self,
        editor: &Entity<EditorView>,
        hunk: HunkId,
        transaction: Option<TransactionId>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = editor.read(cx).model().path().map(Path::to_owned) else {
            return;
        };
        let editors = self.ai_editors(cx);
        let Some(state) = self.ai_changes.as_mut() else {
            return;
        };
        with_editors(&editors, &mut state.made, cx, |set| {
            state.changeset.adopt_accept(set, &path, hunk, transaction)
        });
    }

    /// Opens the buffers made for new files in the active pane, closes the
    /// tabs of those removed, and has every editor drawn again after a
    /// decision changed their text or proposals.
    fn land_ai_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editors = self.ai_editors(cx);
        let (made, mut removed): (Vec<_>, Vec<_>) = match self.ai_changes.as_mut() {
            Some(state) => (
                std::mem::take(&mut state.made.buffers),
                std::mem::take(&mut state.made.removed),
            ),
            None => return,
        };
        if let Some(state) = self.ai_changes.as_mut() {
            // Previews of files no longer pending go: a file accepted gets
            // its own tab, one rejected has none.
            let pending: Vec<PathBuf> = with_editors(&editors, &mut state.made, cx, |set| {
                (state.changeset.files(set).into_iter())
                    .filter(|entry| entry.is_new && entry.counts.pending > 0)
                    .map(|entry| entry.path)
                    .collect()
            });
            state.previews.retain(|(path, buffer)| {
                let keep = pending.contains(path);
                if !keep {
                    removed.push(*buffer);
                }
                keep
            });
        }
        let pane = self.pane().clone();
        for (path, buffer) in made {
            pane.update(cx, |pane, cx| {
                pane.add(fe_editor::EditorModel::new(buffer, Some(path)), cx)
            });
        }
        if !removed.is_empty() {
            for pane in self
                .pane_group(cx)
                .panes()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                pane.update(cx, |pane, cx| pane.discard_buffers(&removed, window, cx));
            }
        }
        for editor in self.ai_editors(cx) {
            editor.update(cx, |_, cx| cx.notify());
        }
        cx.refresh_windows();
        self.sync_problems(cx);
        self.sync_ai_changes(cx);
    }

    /// Tells of the files `errors` name, in a toast.
    fn ai_conflicts(errors: impl IntoIterator<Item = ChangesetError>, cx: &mut App) {
        let errors: Vec<String> = errors.into_iter().map(|error| error.to_string()).collect();
        if !errors.is_empty() {
            toast::show(cx, Toast::info(errors.join("\n")));
        }
    }

    /// Accepts or rejects every pending hunk of `path`.
    pub(crate) fn decide_ai_file(
        &mut self,
        path: &Path,
        accept: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors = self.ai_editors(cx);
        let Some(state) = self.ai_changes.as_mut() else {
            return;
        };
        let result = with_editors(&editors, &mut state.made, cx, |set| match accept {
            true => state.changeset.accept_file(set, path).map(drop),
            false => state.changeset.reject_file(set, path),
        });
        Self::ai_conflicts(result.err(), cx);
        self.land_ai_changes(window, cx);
    }

    /// Does an AI changes command's op.
    pub(crate) fn run_ai_changes_op(
        &mut self,
        op: AiChangesOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match op {
            AiChangesOp::NextFile | AiChangesOp::PreviousFile => {
                let forward = op == AiChangesOp::NextFile;
                let panel = self.ai_changes_panel(cx);
                panel.update(cx, |panel, cx| panel.move_selection(forward, cx));
            }
            AiChangesOp::AcceptAll | AiChangesOp::RejectAll => {
                let editors = self.ai_editors(cx);
                let Some(state) = self.ai_changes.as_mut() else {
                    return;
                };
                let errors = with_editors(&editors, &mut state.made, cx, |set| match op {
                    AiChangesOp::AcceptAll => (state.changeset.accept_all(set).into_iter())
                        .filter_map(|(_, result)| result.err())
                        .collect::<Vec<_>>(),
                    _ => (state.changeset.reject_all(set).into_iter())
                        .filter_map(|(_, result)| result.err())
                        .collect(),
                });
                Self::ai_conflicts(errors, cx);
                self.land_ai_changes(window, cx);
            }
            AiChangesOp::Revert => self.revert_ai_turn(Vec::new(), window, cx),
        }
    }

    /// Reverts what the turn accepted. A file edited since is restored only
    /// after asking, naming the files; the answer holds for those files of
    /// this changeset and no others, as their buffers were when asked, so a
    /// turn that replaced it meanwhile, or a file edited or reopened since
    /// the question, asks again.
    fn revert_ai_turn(
        &mut self,
        confirmed: Vec<(PathBuf, Option<(BufferId, Version)>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors = self.ai_editors(cx);
        let Some(state) = self.ai_changes.as_mut() else {
            return;
        };
        let generation = state.generation;
        let outcomes = with_editors(&editors, &mut state.made, cx, |set| {
            let accepted = state.changeset.summary(set).counts.accepted;
            (accepted > 0).then(|| state.changeset.revert(set))
        });
        self.land_ai_changes(window, cx);
        let edited: Vec<PathBuf> = outcomes
            .into_iter()
            .flatten()
            .filter_map(|outcome| match outcome {
                RevertOutcome::NeedsConfirm { path } => Some(path),
                _ => None,
            })
            .collect();
        if edited.is_empty() {
            return;
        }
        let marks = self.buffer_marks(&edited, cx);
        if marks.iter().all(|mark| confirmed.contains(mark)) {
            if let Some(state) = self.ai_changes.as_mut() {
                with_editors(&editors, &mut state.made, cx, |set| {
                    state.changeset.revert_forced(set)
                });
            }
            return self.land_ai_changes(window, cx);
        }
        let root = self.root.clone();
        let names: Vec<String> = (edited.iter())
            .map(|path| {
                let relative = root
                    .as_deref()
                    .and_then(|root| path.strip_prefix(root).ok());
                relative.unwrap_or(path).display().to_string()
            })
            .collect();
        let detail = format!(
            "These files were edited since the turn accepted its changes:\n{}\n\nRestoring them loses those edits.",
            names.join("\n")
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            "Revert the turn's changes?",
            Some(&detail),
            &[PromptButton::ok("Revert"), PromptButton::cancel("Cancel")],
            cx,
        );
        cx.spawn_in(window, async move |workbench, cx| {
            if answer.await != Ok(0) {
                return;
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let same = (workbench.ai_changes.as_ref())
                        .is_some_and(|state| state.generation == generation);
                    if same {
                        workbench.revert_ai_turn(marks, window, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Each of `paths` with the identity and version of the buffer an open
    /// editor shows for it, `None` if none does.
    fn buffer_marks(
        &self,
        paths: &[PathBuf],
        cx: &App,
    ) -> Vec<(PathBuf, Option<(BufferId, Version)>)> {
        let editors = self.ai_editors(cx);
        (paths.iter())
            .map(|path| {
                let mark = editors.iter().find_map(|editor| {
                    let model = editor.read(cx).model();
                    (model.path() == Some(path.as_path()))
                        .then(|| (model.buffer().id(), model.buffer().version()))
                });
                (path.clone(), mark)
            })
            .collect()
    }

    /// Opens `path`, a file of the changeset, in the active pane, shows its
    /// proposal in its editor and moves to its first pending hunk. A new
    /// file not yet accepted is accepted, which makes its buffer in a tab to
    /// review; an accepted one shows its tab.
    pub(crate) fn open_ai_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.ai_changes.as_mut() else {
            return;
        };
        state.opening = None;
        if state.changeset.proposal(&path).is_none() {
            if state.changeset.new_file_text(&path).is_some() {
                self.open_ai_new_file(path, window, cx);
            }
            return;
        }
        let pane = self.pane().clone();
        if crate::loader::is_svg(&path) {
            // Text, not the viewer ordinary routing opens for an SVG.
            pane.update(cx, |pane, cx| pane.open_as_text(path.clone(), window, cx));
            if Self::shows_text(&pane, &path, cx) {
                return self.land_ai_file(&pane, &path, window, cx);
            }
            let watched = path.clone();
            let (serial, navigations) = {
                let read = pane.read(cx);
                (read.open_serial(), read.navigations())
            };
            let opening = cx.observe_in(&pane, window, move |workbench, pane, window, cx| {
                let read = pane.read(cx);
                let current = read.open_serial() == serial && read.navigations() == navigations;
                if current && !Self::shows_text(&pane, &watched, cx) {
                    return;
                }
                if let Some(state) = workbench.ai_changes.as_mut() {
                    state.opening = None;
                }
                // The user moved on meanwhile: show nothing.
                if current {
                    workbench.land_ai_file(&pane, &watched, window, cx);
                }
            });
            if let Some(state) = self.ai_changes.as_mut() {
                state.opening = Some(opening);
            }
            return;
        }
        pane.update(cx, |pane, cx| pane.open_at(path.clone(), None, window, cx));
        if !pane.read(cx).is_opening(&path) {
            return self.land_ai_file(&pane, &path, window, cx);
        }
        let serial = pane.read(cx).open_serial();
        let watched = path.clone();
        let opening = cx.observe_in(&pane, window, move |workbench, pane, window, cx| {
            let read = pane.read(cx);
            if read.open_serial() == serial && read.is_opening(&watched) {
                return;
            }
            if let Some(state) = workbench.ai_changes.as_mut() {
                state.opening = None;
            }
            if read.open_serial() == serial {
                workbench.land_ai_file(&pane, &watched, window, cx);
            }
        });
        if let Some(state) = self.ai_changes.as_mut() {
            state.opening = Some(opening);
        }
    }

    /// Whether `pane` has an editor tab on `path`.
    fn shows_text(pane: &Entity<Pane>, path: &Path, cx: &App) -> bool {
        (pane.read(cx).tabs().iter())
            .filter_map(|tab| tab.editor())
            .any(|editor| editor.read(cx).model().path() == Some(path))
    }

    /// Reviews the new file `path`: a pending one opens a tab on a copy of
    /// its text, deciding nothing; an accepted one has its tab shown. Either
    /// is shown in whichever pane has it.
    fn open_ai_new_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let panes: Vec<Entity<Pane>> = self.pane_group(cx).panes().into_iter().cloned().collect();
        let tab = |panes: &[Entity<Pane>], cx: &App| {
            panes.iter().find_map(|pane| {
                let at = pane.read(cx).tabs().iter().position(|tab| {
                    (tab.editor()).is_some_and(|e| e.read(cx).model().path() == Some(&path))
                })?;
                Some((pane.clone(), at))
            })
        };
        let editors = self.ai_editors(cx);
        let pending = self.ai_changes.as_mut().is_some_and(|state| {
            with_editors(&editors, &mut state.made, cx, |set| {
                (state.changeset.files(set).into_iter())
                    .any(|entry| entry.path == path && entry.counts.pending > 0)
            })
        });
        if pending
            && tab(&panes, cx).is_none()
            && let Some(state) = self.ai_changes.as_mut()
            && let Some(text) = state.changeset.new_file_text(&path)
        {
            // Not yet accepted: its text to look at, read-only, for the
            // decision's own buttons to replace or close.
            let buffer = fe_buffer::Buffer::new(text);
            state.previews.push((path.clone(), buffer.id()));
            let model = fe_editor::EditorModel::new(buffer, Some(path.clone()));
            let pane = self.pane().clone();
            pane.update(cx, |pane, cx| pane.add(model, cx));
            let editor = pane.read(cx).active_editor().cloned();
            if let Some(editor) = editor {
                editor.update(cx, |editor, cx| editor.set_read_only(true, cx));
            }
        }
        if let Some((pane, at)) = tab(&panes, cx) {
            pane.update(cx, |pane, cx| pane.activate(at, window, cx));
            let editor = pane.read(cx).active_editor().cloned();
            if let Some(editor) = editor {
                window.focus(&editor.focus_handle(cx), cx);
            }
        }
    }

    /// Shows the proposal of `path` in the editor of `pane`'s on it, at its
    /// first pending hunk, focused. A buffer reopened since its tab closed
    /// is followed first.
    fn land_ai_file(
        &mut self,
        pane: &Entity<Pane>,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_ai_changes(cx);
        let Some((buffer, proposal)) =
            (self.ai_changes.as_ref()).and_then(|state| state.changeset.proposal(path))
        else {
            return;
        };
        let (editor, at) = {
            let pane = pane.read(cx);
            let at = (pane.tabs().iter()).position(|tab| {
                (tab.editor()).is_some_and(|editor| editor.read(cx).model().path() == Some(path))
            });
            (at.and_then(|at| pane.tabs()[at].editor().cloned()), at)
        };
        let (Some(editor), Some(at)) = (editor, at) else {
            return;
        };
        // A tab in the background (an SVG viewer replaced where it was) is
        // brought forward.
        pane.update(cx, |pane, cx| pane.activate(at, window, cx));
        if editor.read(cx).model().buffer().id() != buffer {
            return;
        }
        editor.update(cx, |editor, cx| {
            let hunks = proposal.borrow().hunks(editor.model().buffer().current());
            let first = |status| {
                (hunks.iter())
                    .filter(|hunk| hunk.status == status)
                    .map(|hunk| hunk.range.start)
                    .min()
            };
            if let Some(at) = first(HunkStatus::Pending).or_else(|| first(HunkStatus::Stale)) {
                editor.select_and_reveal(at..at, cx);
            }
        });
        window.focus(&editor.focus_handle(cx), cx);
    }
}

#[cfg(test)]
impl AiChangesPanel {
    /// A row as shown: path, lines added and removed, and the counts.
    pub(crate) fn shown_rows(&self) -> Vec<(String, usize, usize, Counts)> {
        (self.listing.files.iter())
            .map(|file| {
                (
                    self.display_path(&file.path),
                    file.added,
                    file.removed,
                    file.counts,
                )
            })
            .collect()
    }

    pub(crate) fn header_text(&self) -> String {
        self.header()
    }

    pub(crate) fn message_text(&self) -> Option<&'static str> {
        self.message()
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn test_key_context(&self) -> KeyContext {
        self.key_context()
    }
}
