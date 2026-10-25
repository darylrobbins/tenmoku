//! The editor pane: a tab per open file, the active one's editor below.
//! Its tabs, their strip and their drag and drop are kiln-shell's
//! ([`PaneState`], whose items are the editors, and the viewers of files
//! too large to edit); opening files, revealing a place in them, and what
//! shows while a file is read or failed to be, are fastedit's.
//! [`Pane::tabs`] keeps a [`Tab`] per item in step.

use std::{
    cell::RefCell,
    collections::HashMap,
    io,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_buffer::{Bias, ResolvedSelection};
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_config::{EditorConfig, EndOfLine, IndentKind, IndentSize};
use fe_editor::{EditorModel, IndentStyle};
use fe_fileview::{FileView, Freshness, LineIndex};
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView, FileViewer, FileViewerEvent, SettingsEvent,
    commands::RunCommand,
};
use fe_keymap::{KeyOutcome, Keymap, Matcher};
use fe_ui::{
    Breadcrumbs, Crumb, CrumbKind, Icon, IconButton, TAB_HEIGHT, TOOLBAR_HEIGHT, TabBar, Tooltip,
    UiTheme, file_icon,
};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, DragMoveEvent, Entity, EntityId,
    EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, PromptLevel, Render, Styled as _, Subscription, Task, WeakEntity,
    Window, div,
};
use kiln::ViewCommands as _;
pub use kiln_shell::PaneEvent;
use kiln_shell::{ItemHandle, ItemId, PaneState, PaneView};

use crate::{
    closed_editors::{ClosedEditor, SharedClosedEditors, ViewState},
    find_bar::{FindBar, FindOp},
    loader::{self, Opened},
    nav::Place,
    pane_drag::{DraggedTab, TabDropped},
    pane_toolbar::{self, NavButtons, Toolbar, ToolbarRequest},
    workbench::toast::{self, Toast},
    workbench_commands::markdown_preview::{
        self, MARKDOWN_PREVIEW_CONTEXT, MarkdownPreview, TOGGLE_PREVIEW, is_markdown,
    },
};

/// A tab's ID, which stays with it as tabs open, close and move: its
/// item's.
pub(crate) type TabId = ItemId;

/// An open file: the pane's item, as fastedit reads it: an editor, or a
/// [`FileViewer`] of a file too large to edit. The item is the tab's
/// [`PaneItem`](kiln_shell::PaneItem) ([`Tab::item`]), which the tab strip
/// and the window's title read.
#[derive(Clone)]
pub struct Tab {
    /// `None` for a buffer that was never saved, such as the welcome text.
    pub path: Option<PathBuf>,
    item: Box<dyn ItemHandle>,
    /// The editor's Markdown preview, while it's shown in the editor's
    /// place: `markdown.toggle_preview`. The editor stays the tab's item,
    /// so its title, saves, closes and saved layouts are the editor's, and
    /// the preview isn't kept in a layout. A copy of what
    /// [`markdown_preview::in_place`] holds for the editor, which keeps it
    /// as the tab moves to another pane.
    preview: Option<Entity<MarkdownPreview>>,
}

impl Tab {
    /// The tab for `item`, its path and in-place preview as they are now.
    fn of(item: &dyn ItemHandle, cx: &App) -> Self {
        let editor = item.downcast_ref::<EditorView>();
        Self {
            path: item.path(cx),
            item: item.boxed_clone(),
            preview: editor.and_then(|editor| markdown_preview::in_place(editor, cx).cloned()),
        }
    }

    /// The editor's Markdown preview, while it's shown in its place.
    pub(crate) fn preview(&self) -> Option<&Entity<MarkdownPreview>> {
        self.preview.as_ref()
    }

    pub(crate) fn id(&self) -> TabId {
        self.item().id()
    }

    /// The tab's item, as kiln-shell holds items.
    pub fn item(&self) -> &dyn ItemHandle {
        &*self.item
    }

    /// The tab's editor; `None` for a viewer.
    pub fn editor(&self) -> Option<&Entity<EditorView>> {
        self.item.downcast_ref()
    }

    /// The tab's viewer, if it shows a file too large to edit.
    pub fn viewer(&self) -> Option<&Entity<FileViewer>> {
        self.item.downcast_ref()
    }
}

pub struct Pane {
    /// The tabs' items, the active one, focus and layout: kiln-shell's.
    state: PaneState<Pane>,
    /// A tab per item, in the same order, kept in step as items come, go
    /// and move.
    tabs: Vec<Tab>,
    /// The file opened last, while it's read: shown in the active tab's
    /// place until it lands.
    opening: Option<PathBuf>,
    /// Reads running, so a second click on a file being read doesn't read it
    /// again. Dropping one cancels it.
    reads: HashMap<PathBuf, Task<()>>,
    /// Why each file being read, or waiting behind a Save As, was opened:
    /// for a look, to land in the preview tab, or to stay. An open to stay
    /// is for good: a later open for a look doesn't undo it.
    intents: HashMap<PathBuf, Intent>,
    /// The file opened last and why it couldn't be read, shown in the
    /// active tab's place until another tab is shown.
    error: Option<(PathBuf, String)>,
    /// What to select in the file opened last once its editor exists.
    reveal: Option<(PathBuf, Reveal)>,
    /// Bumped by every [`Pane::open_at`], so the open last made can be told
    /// from the ones it superseded.
    open_serial: u64,
    /// Bumped by every tab shown or closed other than by a file's read
    /// landing: the user's navigation, which a pending open mustn't undo.
    navigations: u64,
    /// Set while a file's read lands, whose tab shown isn't a navigation.
    landing: bool,
    /// The editor shown in the active tab's place, as [`Pane::note_shown`]
    /// last saw it, and how many times it has changed: see
    /// [`Revealed::shown`].
    shown: Option<EntityId>,
    shown_serial: u64,
    /// The last reveal applied to an editor, and the open it was for.
    revealed: Option<Revealed>,
    /// The window's closed file editors, which every pane of it shares.
    closed: SharedClosedEditors,
    /// The selections and scroll each file being reopened gets back once
    /// its editor is in the pane ([`Pane::reopen`]).
    restores: HashMap<PathBuf, ViewState>,
    /// Set while the preview tab is replaced, which doesn't close it for
    /// [`Pane::closed_editors`].
    replacing: bool,
    /// Pushes the tabs of a pane closed with them, as Close Pane closes it,
    /// as the pane is released: the workspace holds the only strong handle.
    _released: Subscription,
    theme: EditorTheme,
    /// For editors yet to open.
    settings: EditorSettings,
    /// To [`fe_gpui::ActivePalette`]: recolours the open editors and the
    /// theme later ones open with.
    _palette: Subscription,
    toolbar: Toolbar,
    /// Made by the first `find.show`.
    find: Option<FindBar>,
    /// The Peek Definition widget open in one of the pane's editors, drawn
    /// while that editor shows ([`crate::peek`]).
    pub(crate) peek: Option<crate::peek::Peek>,
    /// The window the last frame drew the pane in, which a viewer's
    /// request to edit its file prompts in.
    window: Option<AnyWindowHandle>,
    this: WeakEntity<Pane>,
    /// The keys typed in a Markdown preview shown in a tab, toward a Save.
    save_keys: Rc<RefCell<SaveKeys>>,
    /// Hears each viewer tab's [`FileViewerEvent::EditRequested`].
    viewer_events: HashMap<TabId, Subscription>,
    /// Reads of the files of viewer tabs being converted to editors.
    conversions: HashMap<TabId, Task<()>>,
    /// The tab bar's buttons' laid-out bounds.
    #[cfg(test)]
    actions: gpui::ScrollHandle,
    /// The tab bar's buttons the last frame drew: ID and icon.
    #[cfg(test)]
    drawn_actions: Vec<(&'static str, Icon)>,
    /// The crumbs the last frame drew: label, kind and icon.
    #[cfg(test)]
    drawn_crumbs: Vec<(String, CrumbKind, Option<Icon>)>,
    #[cfg(test)]
    last_tooltip: Rc<std::cell::RefCell<Option<String>>>,
}

/// Saves the file, in the editor and in its preview shown in its place.
const SAVE: &str = "editor.save";

/// The keys typed in a Markdown preview shown in an editor's place,
/// matched as the workbench's editor would match them, with the keymap
/// they're matched against: for [`SaveKeys::saves`].
#[derive(Default)]
struct SaveKeys(Option<(Arc<Keymap>, Matcher)>);

impl SaveKeys {
    /// Whether `keystroke` ends a key or chord that runs [`SAVE`] in the
    /// workbench's editor, as the keymap has it now. No binding names the
    /// preview, so its keys go on unmatched to its key listener; any other
    /// command they'd run in the editor doesn't apply.
    fn saves(&mut self, keystroke: &gpui::Keystroke, cx: &App) -> bool {
        let keymap = fe_gpui::current_keymap(cx);
        if (self.0.as_ref()).is_none_or(|(matched, _)| !Arc::ptr_eq(matched, &keymap)) {
            self.0 = Some((keymap.clone(), Matcher::new(keymap)));
        }
        let (_, matcher) = self.0.as_mut().expect("just made");
        let Some(keystroke) = kiln_platform::keys::keystroke(keystroke) else {
            matcher.flush();
            return false;
        };
        let in_editor: ContextStack = [crate::KEY_CONTEXT, fe_editor::commands::CONTEXT]
            .into_iter()
            .map(ContextFrame::new)
            .collect();
        let mut outcome = matcher.push(keystroke, &in_editor);
        // A keystroke that ends a chord matching nothing starts afresh, as
        // the keymap's own matching has it.
        if matches!(&outcome, KeyOutcome::Unmatched(keys) if keys.len() > 1) {
            outcome = matcher.push(keystroke, &in_editor);
        }
        matches!(outcome, KeyOutcome::Dispatch(run) if run.command.as_str() == SAVE)
    }
}

/// What a Markdown tab shows: see [`Pane::markdown_shown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkdownShown {
    Source,
    Preview,
}

/// Dispatches `command` as [`pane_toolbar::dispatch`] does, from the mouse,
/// but invoked in `frame` within the workbench.
fn dispatch_in(command: &str, frame: &str, window: &mut Window, cx: &mut App) {
    let Ok(invocation) = Invocation::of(command) else {
        return;
    };
    let context = [
        ContextFrame::new(crate::KEY_CONTEXT),
        ContextFrame::new(frame),
    ]
    .into_iter()
    .collect();
    let invocation = invocation.source(Source::Mouse).context(context);
    window.dispatch_action(Box::new(RunCommand(invocation)), cx);
}

/// Why a file being read was opened: see [`Pane::open_preview`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Intent {
    /// For a look, in the preview tab.
    Preview,
    /// To stay.
    Keep,
}

impl Pane {
    pub fn new(theme: EditorTheme, settings: EditorSettings, cx: &mut Context<Self>) -> Self {
        Self {
            state: PaneState::new(cx),
            tabs: Vec::new(),
            opening: None,
            reads: HashMap::new(),
            intents: HashMap::new(),
            error: None,
            reveal: None,
            open_serial: 0,
            navigations: 0,
            landing: false,
            shown: None,
            shown_serial: 0,
            revealed: None,
            closed: SharedClosedEditors::default(),
            restores: HashMap::new(),
            replacing: false,
            _released: cx.on_release(|pane, cx| {
                for tab in &pane.tabs {
                    pane.note_closed(tab, cx);
                }
            }),
            theme,
            settings,
            _palette: cx.observe_global::<fe_gpui::ActivePalette>(Self::follow_palette),
            toolbar: Toolbar::default(),
            find: None,
            peek: None,
            window: None,
            this: cx.weak_entity(),
            save_keys: Rc::default(),
            viewer_events: HashMap::new(),
            conversions: HashMap::new(),
            #[cfg(test)]
            actions: gpui::ScrollHandle::new(),
            #[cfg(test)]
            drawn_actions: Vec::new(),
            #[cfg(test)]
            drawn_crumbs: Vec::new(),
            #[cfg(test)]
            last_tooltip: Rc::default(),
        }
    }

    /// A pane like this one, empty, opening editors as it does.
    pub(crate) fn new_peer(&self, cx: &mut Context<Self>) -> Self {
        let mut pane = Self::new(self.theme.clone(), self.settings.clone(), cx);
        pane.state.set_peers(self.state.peers().clone());
        pane.closed = self.closed.clone();
        pane
    }

    /// Recolours the pane's editors, and those it opens later, with the
    /// active palette.
    fn follow_palette(&mut self, cx: &mut Context<Self>) {
        let palette = fe_gpui::editor_palette(cx);
        self.theme.set_colors(&palette);
        for editor in self.tabs.iter().filter_map(Tab::editor) {
            editor.update(cx, |editor, cx| editor.set_colors(&palette, cx));
        }
        cx.notify();
    }

    pub(crate) fn toolbar(&self) -> &Toolbar {
        &self.toolbar
    }

    /// Changes the toolbar's state: see [`crate::pane_toolbar`].
    pub(crate) fn update_toolbar(
        &mut self,
        update: impl FnOnce(&mut Toolbar),
        cx: &mut Context<Self>,
    ) {
        update(&mut self.toolbar);
        cx.notify();
    }

    /// Where the last frame laid the pane out; zero-sized before that.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.state.bounds()
    }

    /// Whether the pane is reading `path` to show it.
    pub(crate) fn is_opening(&self, path: &Path) -> bool {
        self.opening.as_deref() == Some(path)
    }

    /// Every file the pane is reading to show, or waiting for the host to
    /// read: each lands in a tab, older reads too.
    pub(crate) fn reading(&self) -> impl Iterator<Item = &Path> {
        (self.reads.keys().map(PathBuf::as_path)).chain(self.opening.as_deref())
    }

    /// Where the caret will be once the file being read lands: at the
    /// revealed offset, else the start of a file opened plainly. `None` if
    /// no file is being read, or the reveal is by row and column, which
    /// can't be an offset before the text exists.
    pub(crate) fn pending_place(&self) -> Option<Place> {
        let path = self.opening.as_ref()?;
        let offset = match self.reveal.as_ref().filter(|(at, _)| at == path) {
            None => 0,
            Some((_, reveal)) if reveal.row == 0 => reveal.columns.end,
            Some(_) => return None,
        };
        Some(Place {
            path: path.clone(),
            offset,
        })
    }

    /// Whether the pane is reading a file whose caret place isn't known yet:
    /// the reveal is by row and column, which can't be an offset before the
    /// text exists.
    pub(crate) fn place_pending(&self) -> bool {
        self.opening.is_some() && self.pending_place().is_none()
    }

    /// The theme editors open with.
    pub fn theme(&self) -> &EditorTheme {
        &self.theme
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The active tab's index; `None` with no tabs.
    pub fn active(&self) -> Option<usize> {
        self.state.active()
    }

    /// The active tab, whatever shows in its place. By its item, which
    /// kiln-shell has changed before [`PaneView::items_changed`] brings the
    /// tabs in step.
    pub(crate) fn active_tab(&self) -> Option<&Tab> {
        let id = self.state.active_item()?.id();
        self.tabs.iter().find(|tab| tab.id() == id)
    }

    /// The Markdown preview shown in the active tab's place, unless a file
    /// being opened or a read error shows.
    pub(crate) fn shown_preview(&self) -> Option<&Entity<MarkdownPreview>> {
        self.active_item()?;
        self.active_tab()?.preview()
    }

    /// Whether the active tab is a Markdown editor's showing its text, or
    /// its preview in the text's place; `None` for any other tab, or while
    /// a file being opened or a read error shows.
    fn markdown_shown(&self, cx: &App) -> Option<MarkdownShown> {
        self.active_item()?;
        let tab = self.active_tab()?;
        if tab.preview.is_some() {
            return Some(MarkdownShown::Preview);
        }
        is_markdown(tab.editor()?.read(cx)).then_some(MarkdownShown::Source)
    }

    /// Shows `preview` in the place of tab `ix`'s editor, focused if the
    /// tab is active; with `None`, the editor again, focused if the tab is.
    pub(crate) fn show_in_place(
        &mut self,
        ix: usize,
        preview: Option<Entity<MarkdownPreview>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        let Some(editor) = tab.editor().cloned() else {
            return;
        };
        tab.preview = preview.clone();
        markdown_preview::set_in_place(&editor, preview.clone(), cx);
        if self.state.active() == Some(ix) {
            let focus = match &preview {
                Some(preview) => preview.focus_handle(cx),
                None => editor.focus_handle(cx),
            };
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// The active tab's editor, unless a file being opened or a read error
    /// shows in its place.
    pub fn active_editor(&self) -> Option<&Entity<EditorView>> {
        if self.opening.is_some() || self.error.is_some() {
            return None;
        }
        self.state.active_item()?.downcast_ref::<EditorView>()
    }

    /// The active tab's item, its editor or viewer, unless a file being
    /// opened or a read error shows in its place: what the status bar
    /// follows.
    pub fn active_item(&self) -> Option<&dyn ItemHandle> {
        if self.opening.is_some() || self.error.is_some() {
            return None;
        }
        self.state.active_item()
    }

    /// What gets focus when focus leaves a hidden panel: the active editor
    /// or viewer, or the preview shown in its place, or else the pane.
    pub(crate) fn focus_target(&self, cx: &App) -> FocusHandle {
        match self.shown_item() {
            Some(item) => item.item_focus_handle(cx),
            None => self.state.focus().clone(),
        }
    }

    /// The window title: the active tab's file name, marked if it has
    /// unsaved changes.
    pub fn title(&self, cx: &App) -> String {
        if let Some(path) = &self.opening {
            return format!("{} — loading", file_name(Some(path)));
        }
        match self.state.active_item() {
            Some(item) => item.window_title(cx).into(),
            None => "fastedit".into(),
        }
    }

    /// Opens `model` in a new tab and shows it, without focusing it.
    pub fn add(&mut self, model: EditorModel, cx: &mut Context<Self>) {
        let editor = self.new_editor(model, cx);
        self.add_item(Box::new(editor), cx);
    }

    /// Opens a viewer of `view`, a file too large to edit, in a new tab and
    /// shows it, without focusing it.
    pub(crate) fn add_viewer(
        &mut self,
        view: Arc<FileView>,
        index: Arc<LineIndex>,
        cx: &mut Context<Self>,
    ) {
        let viewer = self.new_viewer(view, index, cx);
        self.add_item(Box::new(viewer), cx);
    }

    /// Opens `item`, an editor or a viewer, in a new tab and shows it,
    /// without focusing it.
    pub(crate) fn add_item(&mut self, item: Box<dyn ItemHandle>, cx: &mut Context<Self>) {
        let ix = self.state.push(item, cx);
        self.state.set_active(ix);
        self.error = None;
        self.sync_tabs(cx);
        self.note_shown(cx);
        cx.notify();
    }

    /// A tab per item, reusing the tab of an item still open. Each editor,
    /// however it came (opened, split or restored), shows the minimap as
    /// the settings say.
    fn sync_tabs(&mut self, cx: &mut App) {
        let old = std::mem::take(&mut self.tabs);
        self.tabs = (self.state.items().iter())
            .map(|item| {
                let id = item.id();
                old.iter()
                    .find(|tab| tab.id() == id)
                    .cloned()
                    .unwrap_or_else(|| Tab::of(&**item, cx))
            })
            .collect();
        let minimap = fe_gpui::minimap_options(&self.settings);
        for editor in self.tabs.iter().filter_map(Tab::editor) {
            configs::read(editor, cx);
            editor.update(cx, |editor, cx| editor.set_minimap(minimap, cx));
        }
        let open: Vec<TabId> = self.tabs.iter().map(Tab::id).collect();
        self.viewer_events.retain(|id, _| open.contains(id));
        if !self.replacing {
            for tab in old.iter().filter(|tab| !open.contains(&tab.id())) {
                self.note_closed(tab, cx);
            }
        }
        // A converting viewer that left the pane isn't converted by it.
        for tab in old.iter().filter(|tab| !open.contains(&tab.id())) {
            if let (Some(_), Some(viewer)) = (self.conversions.remove(&tab.id()), tab.viewer()) {
                viewer.update(cx, |viewer, cx| viewer.set_converting(false, cx));
            }
        }
        for tab in &self.tabs {
            let Some(viewer) = tab.viewer() else {
                continue;
            };
            if !self.viewer_events.contains_key(&tab.id()) {
                let subscription = self.hear_edit_requests(viewer, cx);
                self.viewer_events.insert(tab.id(), subscription);
            }
        }
    }

    /// Pushes `tab`, gone from the pane, to [`Pane::closed_editors`] as it
    /// is now, if it's a file's editor, once the effect cycle is over: a tab
    /// moved to another pane is in it by then, and isn't pushed. A copy of
    /// it, another editor of its file, doesn't stop it.
    fn note_closed(&self, tab: &Tab, cx: &mut App) {
        let Some(closed) =
            (tab.editor()).and_then(|editor| ClosedEditor::of(editor, self.this.clone(), cx))
        else {
            return;
        };
        let (id, stack) = (tab.id(), self.closed.clone());
        // The group as it is then: a pane split off for a move joins it
        // after the tab has left this one.
        let (peers, this) = (self.state.peers().clone(), self.this.clone());
        cx.defer(move |cx| {
            let peers = peers.borrow();
            let panes = peers.iter().chain([&this]).filter_map(WeakEntity::upgrade);
            let moved = (panes.into_iter()).any(|pane| pane.read(cx).state.position(id).is_some());
            if !moved {
                stack.borrow_mut().push(closed);
            }
        });
    }

    /// The window's closed file editors, newest on top, which Reopen
    /// Closed Editor takes from: see [`crate::closed_editors`].
    pub(crate) fn closed_editors(&self) -> &SharedClosedEditors {
        &self.closed
    }

    /// [`PaneView::add_preview`], whose preview tab it replaces isn't
    /// closed for [`Pane::closed_editors`].
    fn replace_preview(&mut self, item: Box<dyn ItemHandle>, cx: &mut App) -> usize {
        self.replacing = true;
        let ix = self.add_preview(item, cx);
        self.replacing = false;
        ix
    }

    /// Hears `viewer` ask to be edited, and handles it in the window the
    /// pane was last drawn in.
    fn hear_edit_requests(&self, viewer: &Entity<FileViewer>, cx: &mut App) -> Subscription {
        let pane = self.this.clone();
        cx.subscribe(viewer, move |viewer, event: &FileViewerEvent, cx| {
            if *event != FileViewerEvent::EditRequested {
                return;
            }
            let Some(pane) = pane.upgrade() else {
                return;
            };
            let Some(window) = pane.read(cx).window else {
                return;
            };
            cx.spawn(async move |cx| {
                cx.update_window(window, |_, window, cx| {
                    pane.update(cx, |pane, cx| pane.edit_requested(&viewer, window, cx))
                })
                .ok();
            })
            .detach();
        })
    }

    /// Converts the viewer tab `viewer` to an editor of its file, after
    /// asking if the file is over [`edit_warning`].
    fn edit_requested(
        &mut self,
        viewer: &Entity<FileViewer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if viewer.read(cx).is_converting() || self.viewer_position(viewer).is_none() {
            return;
        }
        let view = viewer.read(cx).view().clone();
        if view.len() <= edit_warning(view.path()) {
            self.convert(viewer.clone(), window, cx);
            return;
        }
        viewer.update(cx, |viewer, cx| viewer.set_converting(true, cx));
        let name = file_name(Some(view.path()));
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Edit {name}?"),
            Some(&format!(
                "It loads all {} into memory, which takes several seconds and about 1.5 times its size.",
                size_text(view.len())
            )),
            &["Edit", "Cancel"],
            cx,
        );
        let viewer = viewer.clone();
        cx.spawn_in(window, async move |pane, cx| {
            let confirmed = answer.await.ok() == Some(0);
            pane.update_in(cx, |pane, window, cx| {
                viewer.update(cx, |viewer, cx| viewer.set_converting(false, cx));
                if confirmed {
                    pane.convert(viewer, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn viewer_position(&self, viewer: &Entity<FileViewer>) -> Option<usize> {
        let id = viewer.entity_id();
        (self.tabs.iter()).position(|tab| tab.viewer().is_some_and(|v| v.entity_id() == id))
    }

    /// Reads `viewer`'s file on the background executor, unless it has
    /// changed on disk, and then replaces the tab with an editor.
    fn convert(&mut self, viewer: Entity<FileViewer>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.viewer_position(&viewer) else {
            return;
        };
        let view = viewer.read(cx).view().clone();
        if view.check() == Freshness::Changed {
            viewer.update(cx, |viewer, cx| viewer.refuse_changed(cx));
            return;
        }
        let name = file_name(Some(view.path()));
        viewer.update(cx, |viewer, cx| {
            viewer.set_converting(true, cx);
            viewer.set_banner(format!("Loading {name} for editing…"), cx);
        });
        let read = cx.background_spawn({
            let path = view.path().to_owned();
            async move { loader::read(path) }
        });
        let task = cx.spawn_in(window, async move |pane, cx| {
            let result = read.await;
            pane.update_in(cx, |pane, window, cx| {
                pane.converted(viewer, view, result, window, cx)
            })
            .ok();
        });
        self.conversions.insert(self.tabs[ix].id(), task);
    }

    /// Replaces the tab of `viewer`, at the same index, with an editor on
    /// `result`, unless the file changed since the viewer opened it or
    /// couldn't be read: the viewer stays and its banner says why.
    fn converted(
        &mut self,
        viewer: Entity<FileViewer>,
        view: Arc<FileView>,
        result: io::Result<Option<fe_buffer::Buffer>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.viewer_position(&viewer) else {
            return;
        };
        self.conversions.remove(&self.tabs[ix].id());
        // The viewer was reloaded meanwhile: it shows another view of the
        // file now, which this read isn't of.
        if !Arc::ptr_eq(viewer.read(cx).view(), &view) {
            viewer.update(cx, |viewer, cx| viewer.set_converting(false, cx));
            return;
        }
        let buffer = match result {
            Ok(Some(buffer)) if view.check() == Freshness::Unchanged => buffer,
            Ok(_) => {
                viewer.update(cx, |viewer, cx| viewer.refuse_changed(cx));
                return;
            }
            Err(error) => {
                let name = file_name(Some(view.path()));
                viewer.update(cx, |viewer, cx| {
                    viewer.set_converting(false, cx);
                    viewer.set_banner(format!("Cannot read {name}: {error}"), cx);
                });
                return;
            }
        };
        let head = viewer.read(cx).model().head();
        let model = EditorModel::new(buffer, Some(view.path().to_owned()));
        let editor = self.new_editor(model, cx);
        editor.update(cx, |editor, cx| {
            let text = editor.model().text();
            let at = text.clip_offset(head.min(text.len()), Bias::Left);
            editor.select_and_reveal(at..at, cx);
        });
        // The editor goes in before the viewer, which then goes. The
        // active index is put back before the viewer's removal so that
        // `take_item` activates no tab: a newer open pending, and the
        // window's focus, stay as they are.
        let was_active = self.state.active() == Some(ix);
        let had_focus = viewer.focus_handle(cx).contains_focused(window, cx);
        let focus = editor.focus_handle(cx);
        self.state.insert(ix, Box::new(editor), cx);
        if was_active {
            self.state.set_active(ix);
        }
        self.take_item(ix + 1, window, cx);
        if was_active && had_focus {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Shows the tab for `path` if it's open; otherwise reads the file in the
    /// background and opens it in a new tab, focused, once read. Meanwhile
    /// the pane is focused, as the editor it showed is hidden.
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_at(path, None, window, cx);
    }

    /// [`Pane::open`] for a look: a new tab is the pane's preview, in
    /// italics, in the place of the preview there is, which closes
    /// ([`PaneView::add_preview`]). A tab open already just shows. Once
    /// read, it opens only if it's still the file opened last, or the pane
    /// shows nothing else: an open since, or a tab shown, supersedes it.
    pub fn open_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(path, None, true, window, cx);
    }

    /// [`Pane::open`], then selects `reveal` in the file's editor, or puts a
    /// viewer's caret at its start: at once if it's open, else once it's
    /// read, if it's still the file opened last. A file of
    /// [`loader::LARGE_FILE_BYTES`] or more opens in a [`FileViewer`]. A
    /// preview tab of the file is kept, as an ordinary tab, as is a preview
    /// read of it still running.
    pub(crate) fn open_at(
        &mut self,
        path: PathBuf,
        reveal: Option<Reveal>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_tab(path, reveal, false, window, cx);
    }

    /// [`Pane::open_at`] `closed`'s file, revealing its primary selection,
    /// and gives its editor back all its selections and its scroll once
    /// it's in the pane, shown or not: at once if it's open, else once its
    /// read lands, after any wait behind a Save As of it. A read that fails
    /// or is dropped drops them too.
    pub(crate) fn reopen(
        &mut self,
        closed: ClosedEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reveal = (closed.view.primary()).map(|primary| Reveal::bytes(primary.range.clone()));
        self.restores.insert(closed.path.clone(), closed.view);
        self.open_at(closed.path, reveal, window, cx);
    }

    /// Gives tab `ix`'s editor the selections and scroll its reopen saved
    /// ([`Pane::reopen`]), once.
    fn restore_view(&mut self, ix: usize, cx: &mut App) {
        let tab = &self.tabs[ix];
        let Some(view) = (tab.path.as_ref()).and_then(|path| self.restores.remove(path)) else {
            return;
        };
        if let Some(editor) = tab.editor() {
            view.apply(editor, cx);
        }
    }

    fn open_tab(
        &mut self,
        path: PathBuf,
        reveal: Option<Reveal>,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reveal = None;
        self.open_serial += 1;
        // An open to stay still pending is for good.
        let preview = preview && self.intents.get(&path) != Some(&Intent::Keep);
        if crate::open::any_reserved() {
            let identity = crate::open::file_identity(&path);
            if let Some((_, over)) = crate::open::reserved(&path, identity.as_deref()) {
                self.note_intent(&path, preview);
                return self.open_once_saved(path, reveal, over, window, cx);
            }
        }
        if let Some(ix) = self.position(&path) {
            self.activate(ix, window, cx);
            if !preview {
                self.pin_item(ix, cx);
            }
            if let Some(reveal) = reveal {
                self.apply(&reveal, ix, window, cx);
            }
            self.restore_view(ix, cx);
            // Activating the active tab says nothing: an open that landed
            // on it, such as one waiting behind a Save As, is told still.
            cx.notify();
            return;
        }
        if let Some(model) = self.peer_model(&path, cx) {
            let ix = if preview {
                let editor = self.new_editor(model, cx);
                let ix = self.replace_preview(Box::new(editor), cx);
                self.error = None;
                ix
            } else {
                self.add(model, cx);
                self.tabs.len() - 1
            };
            self.activate(ix, window, cx);
            if let Some(reveal) = reveal {
                self.apply(&reveal, ix, window, cx);
            }
            self.restore_view(ix, cx);
            return;
        }
        self.reveal = reveal.map(|reveal| (path.clone(), reveal));
        self.opening = Some(path.clone());
        self.error = None;
        self.note_shown(cx);
        window.focus(self.state.focus(), cx);
        cx.notify();
        self.note_intent(&path, preview);
        if self.reads.contains_key(&path) {
            return;
        }
        let read = cx.background_spawn({
            let path = path.clone();
            async move { loader::open(path) }
        });
        let task = cx.spawn_in(window, {
            let path = path.clone();
            async move |pane, cx| {
                let result = read.await;
                pane.update_in(cx, |pane, window, cx| {
                    pane.file_read(path, result, window, cx)
                })
                .ok();
            }
        });
        self.reads.insert(path, task);
    }

    /// Notes why `path` is being opened, unless it's to stay already.
    fn note_intent(&mut self, path: &Path, preview: bool) {
        if preview {
            self.intents
                .entry(path.to_owned())
                .or_insert(Intent::Preview);
        } else {
            self.intents.insert(path.to_owned(), Intent::Keep);
        }
    }

    /// Shows `path` being opened until the Save As holding it is over
    /// (`over` closes), then opens it, if it's still the file opened last:
    /// in the tab saved there, if the save went through, and for a look or
    /// to stay as it was last opened.
    fn open_once_saved(
        &mut self,
        path: PathBuf,
        reveal: Option<Reveal>,
        over: async_channel::Receiver<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.opening = Some(path.clone());
        self.error = None;
        self.note_shown(cx);
        window.focus(self.state.focus(), cx);
        cx.notify();
        let task = cx.spawn_in(window, {
            let path = path.clone();
            async move |pane, cx| {
                over.recv().await.ok();
                pane.update_in(cx, |pane, window, cx| {
                    pane.reads.remove(&path);
                    let intent = pane.intents.remove(&path);
                    if pane.opening.as_ref() == Some(&path) {
                        pane.opening = None;
                        let preview = intent == Some(Intent::Preview);
                        pane.open_tab(path, reveal, preview, window, cx);
                    } else {
                        pane.restores.remove(&path);
                    }
                })
                .ok();
            }
        });
        self.reads.insert(path, task);
    }

    /// Shows `path` as being read, by the host, which hands the result to
    /// [`Pane::file_read`].
    pub fn opening(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.intents.insert(path.clone(), Intent::Keep);
        self.opening = Some(path);
        self.note_shown(cx);
        cx.notify();
    }

    /// Opens a finished read of `path` (`Ok(Opened::Text(None))` is a new
    /// file) in a new tab, unless it's open already: an editor, or for a
    /// large file a [`FileViewer`] under a read-only banner. If another
    /// pane has the file in an editor by now, the tab shares that pane's
    /// buffer instead. The tab is shown and focused if it's
    /// the file opened last, or if the pane shows nothing else (no tab,
    /// read or error); an error is shown in the tab's place, with the pane
    /// focused, only for the file opened last. So an older read never hides
    /// a newer one or its error, and never shows without focus. A read
    /// opened for a look ([`Pane::open_preview`]) that's neither is
    /// superseded, and dropped.
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: io::Result<Opened>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.landing = true;
        self.file_landed(path, result, window, cx);
        self.landing = false;
    }

    fn file_landed(
        &mut self,
        path: PathBuf,
        result: io::Result<Opened>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reads.remove(&path);
        let intent = self.intents.remove(&path);
        let preview = intent == Some(Intent::Preview);
        let last = self.opening.as_ref() == Some(&path);
        if last {
            self.opening = None;
        }
        let empty = self.active().is_none() && self.opening.is_none() && self.error.is_none();
        let reveal = self.reveal.take_if(|(at, _)| last && *at == path);
        match result {
            Ok(_) if preview && !(last || empty) && self.position(&path).is_none() => {
                self.restores.remove(&path);
            }
            Ok(opened) => {
                let open = self.position(&path);
                let ix = open.unwrap_or_else(|| {
                    let item: Box<dyn ItemHandle> = match opened {
                        Opened::Text(buffer) => {
                            let model = self.peer_model(&path, cx).unwrap_or_else(|| {
                                EditorModel::new(loader::buffer_or_new(buffer), Some(path))
                            });
                            Box::new(self.new_editor(model, cx))
                        }
                        Opened::Large { view, index } => Box::new(self.new_viewer(view, index, cx)),
                    };
                    if preview {
                        return self.replace_preview(item, cx);
                    }
                    let ix = self.state.push(item, cx);
                    self.sync_tabs(cx);
                    ix
                });
                if intent == Some(Intent::Keep) {
                    self.pin_item(ix, cx);
                }
                if last || empty {
                    self.activate(ix, window, cx);
                    cx.emit(PaneEvent::Activated);
                }
                if let Some((_, reveal)) = reveal {
                    self.apply(&reveal, ix, window, cx);
                }
                self.restore_view(ix, cx);
            }
            Err(error) => {
                self.restores.remove(&path);
                log::error!("cannot open {}: {error}", path.display());
                let name = path.file_name().unwrap_or(path.as_os_str());
                toast::show(
                    cx,
                    Toast::error(format!("Can't open {}", name.to_string_lossy()))
                        .detail(error.to_string()),
                );
                if last {
                    let message = format!("Cannot open {}: {error}", path.display());
                    self.error = Some((path.clone(), message));
                    window.focus(self.state.focus(), cx);
                    cx.emit(PaneEvent::Activated);
                }
            }
        }
        self.note_shown(cx);
        cx.notify();
    }

    /// Selects `reveal` in tab `ix`'s editor, for the open last made; in a
    /// viewer, puts the caret at its start.
    fn apply(&mut self, reveal: &Reveal, ix: usize, window: &mut Window, cx: &mut App) {
        let tab = &self.tabs[ix];
        if tab.viewer().is_some() {
            // The viewer puts it at the place as its index reaches it:
            // clipping to the rows found so far would lose it.
            let state = fe_command::Value::from_iter([
                ("row", fe_command::Value::from(reveal.row)),
                ("column", fe_command::Value::from(reveal.columns.start)),
                ("within_row", fe_command::Value::from(reveal.within_row)),
            ]);
            tab.item().apply_state(&state, window, cx);
            return;
        }
        let Some(editor) = tab.editor().cloned() else {
            return;
        };
        // A place in the text shows the text.
        if let Some(preview) = self.tabs[ix].preview.take() {
            markdown_preview::set_in_place(&editor, None, cx);
            if preview.focus_handle(cx).contains_focused(window, cx) {
                window.focus(&editor.focus_handle(cx), cx);
            }
            cx.notify(self.this.entity_id());
        }
        reveal.select(&editor, cx);
        self.revealed = Some(Revealed {
            serial: self.open_serial,
            editor: editor.entity_id(),
            selection: editor.read(cx).model().primary_selection(),
            shown: self.shown_serial,
        });
    }

    /// The serial of the open last made: see [`Pane::revealed`].
    pub(crate) fn open_serial(&self) -> u64 {
        self.open_serial
    }

    /// How many times the user has navigated the tabs: a tab shown by
    /// click, cycling, an open or a reorder, or a tab closed. Not counted:
    /// a file's read landing ([`Pane::file_read`]).
    pub(crate) fn navigations(&self) -> u64 {
        self.navigations
    }

    /// What [`Revealed::shown`] is compared with.
    pub(crate) fn shown_serial(&self) -> u64 {
        self.shown_serial
    }

    /// Counts a change to the editor shown in the active tab's place, by
    /// identity: called after anything that may change it. An open find
    /// bar follows the change.
    fn note_shown(&mut self, cx: &mut App) {
        let shown = self.active_editor().map(Entity::entity_id);
        if shown != self.shown {
            self.shown = shown;
            self.shown_serial += 1;
            self.update_find(|find, editor, cx| find.follow(editor, cx), cx);
        }
    }

    /// The last reveal applied to an editor. It's for the open last made if
    /// its serial is [`Pane::open_serial`]: an open's reveal is applied at
    /// most once, right away or when its file is read, and never once
    /// another open supersedes it or a tab is shown in its read's place.
    pub(crate) fn revealed(&self) -> Option<&Revealed> {
        self.revealed.as_ref()
    }

    /// Shows tab `ix` and focuses its editor. A file still being read then
    /// opens in a tab without being shown.
    pub fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut App) {
        self.activate_item(ix, window, cx);
    }

    /// Pins the active tab: a preview tab is kept, as an ordinary tab that
    /// the next preview doesn't replace.
    pub fn keep_active(&mut self, cx: &mut App) {
        if let Some(ix) = self.active() {
            self.pin_item(ix, cx);
        }
    }

    /// Activates the next tab, or with `back` the previous one, wrapping. A
    /// sole tab activates itself, so it too replaces a file being read or a
    /// read error.
    pub fn cycle(&mut self, back: bool, window: &mut Window, cx: &mut App) {
        self.cycle_items(back, window, cx);
    }

    /// Closes the active tab and activates its right neighbour, or the left
    /// one if it was last. A tab with unsaved changes stays open (until
    /// there are save prompts), unless another tab, in this pane or
    /// another, shows its buffer: the changes live on there.
    pub fn close_active(&mut self, window: &mut Window, cx: &mut App) {
        // The intent counts now, not once the tab is gone: a discard prompt
        // may keep it open while a pending open lands.
        self.navigations += 1;
        self.close_active_item(window, cx);
    }

    /// Moves tab `ix` to before the tab now at `index`, or else to the end,
    /// and shows and focuses it.
    #[cfg(test)]
    pub(crate) fn reorder(
        &mut self,
        ix: usize,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reorder_item(ix, index, window, cx);
        cx.notify();
    }

    /// A model over the buffer of another pane's editor for `path`, if one
    /// has it open; or else over the working buffer of a diff view of the
    /// file, found by its real path, which `git_diff` may have read for the
    /// diff alone. A viewer has no buffer to share.
    fn peer_model(&self, path: &Path, cx: &App) -> Option<EditorModel> {
        let item = self.state.peer_item(cx, |item| {
            item.downcast_ref::<EditorView>().is_some() && item.path(cx).as_deref() == Some(path)
        });
        match item.and_then(|item| item.downcast::<EditorView>()) {
            Some(editor) => Some(editor.read(cx).model().split()),
            None => crate::Workbench::diff_source(path, cx),
        }
    }

    /// Applies reloaded settings to the open editors and to editors yet to
    /// open.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        self.settings = event.settings.editor.clone();
        self.theme.set_metrics(&self.settings, cx);
        let minimap = fe_gpui::minimap_options(&self.settings);
        for editor in self.tabs.iter().filter_map(Tab::editor) {
            editor.update(cx, |editor, cx| {
                editor.reload_settings(event, cx);
                editor.set_minimap(minimap, cx);
                editor
                    .model_mut()
                    .set_auto_close(self.settings.auto_close_pairs);
            });
            if let Some(width) = configs::get(editor.entity_id(), cx).and_then(|c| c.tab_width) {
                editor.update(cx, |editor, _| editor.model_mut().set_tab_size(width));
            }
        }
        cx.notify();
    }

    /// Has each tab take its item's path as it is now: after a save under
    /// another name. An editor whose path changed drops the old file's
    /// `.editorconfig` properties for the settings' tab size and the
    /// indentation its text shows, then reads the new file's.
    pub(crate) fn sync_paths(&mut self, cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            let path = tab.item.path(cx);
            if tab.path == path {
                continue;
            }
            tab.path = path;
            let Some(editor) = tab.editor() else {
                continue;
            };
            let tab_size = self.settings.tab_size;
            editor.update(cx, |editor, cx| {
                let model = editor.model_mut();
                model.set_tab_size(tab_size);
                let detected = fe_editor::detect_indent_style(model.text());
                model.set_indent_style(detected);
                cx.notify();
            });
            configs::read(editor, cx);
        }
        cx.notify();
    }

    pub(crate) fn position(&self, path: &Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.path.as_deref() == Some(path))
    }

    fn new_editor(&mut self, mut model: EditorModel, cx: &mut Context<Self>) -> Entity<EditorView> {
        if model.tab_size() != self.settings.tab_size {
            model.set_tab_size(self.settings.tab_size);
        }
        if model.soft_wrap() != self.settings.soft_wrap {
            model.set_soft_wrap(self.settings.soft_wrap);
        }
        model.set_auto_close(self.settings.auto_close_pairs);
        let theme = self.theme.clone();
        cx.new(|cx| EditorView::new(model, theme, cx))
    }

    /// A viewer of `view`, a file too large to edit, whose rows `index`
    /// finds, under a banner saying so.
    fn new_viewer(
        &mut self,
        view: Arc<FileView>,
        index: Arc<LineIndex>,
        cx: &mut Context<Self>,
    ) -> Entity<FileViewer> {
        let banner = format!(
            "Read-only: {} is over {}.",
            size_text(view.len()),
            size_text(loader::LARGE_FILE_BYTES)
        );
        let theme = self.theme.clone();
        cx.new(|cx| FileViewer::new(view, index, banner.into(), theme, cx))
    }

    fn message(&self, text: String, cx: &App) -> gpui::AnyElement {
        div()
            .size_full()
            .px_3()
            .py_2()
            .text_color(fe_ui::ui_theme(cx).muted_text)
            .child(text)
            .into_any_element()
    }
}

/// The `.editorconfig` properties of the editors open, read once per
/// editor whichever pane it's in, so they follow it as its tab moves.
mod configs {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
    };

    use fe_config::{EditorConfig, editorconfig_for};
    use fe_gpui::EditorView;
    use gpui::{App, AppContext as _, Entity, EntityId, Global, WeakEntity};

    use super::apply_config;

    /// Kept for the settings reloads and the save card.
    #[derive(Default)]
    struct Configs {
        /// Each editor's, of the file it was read for; `None` while the
        /// read runs.
        editors: HashMap<EntityId, (WeakEntity<EditorView>, PathBuf, Option<EditorConfig>)>,
    }

    impl Global for Configs {}

    /// `editor`'s, if read for the file it has now.
    pub(super) fn get(editor: EntityId, cx: &App) -> Option<EditorConfig> {
        let configs = cx.try_global::<Configs>()?;
        let (weak, path, config) = configs.editors.get(&editor)?;
        let now = weak.upgrade()?.read(cx).model().path().map(Path::to_owned);
        (now.as_ref() == Some(path)).then_some(*config)?
    }

    /// Starts reading `editor`'s file's properties in the background, unless
    /// they're read or being read for that file, and applies them once read,
    /// if the editor is still open, at that file.
    pub(super) fn read(editor: &Entity<EditorView>, cx: &mut App) {
        let Some(path) = editor.read(cx).model().path().map(ToOwned::to_owned) else {
            return;
        };
        let id = editor.entity_id();
        let configs = cx.default_global::<Configs>();
        configs
            .editors
            .retain(|_, (editor, _, _)| editor.upgrade().is_some());
        if configs
            .editors
            .get(&id)
            .is_some_and(|(_, read, _)| *read == path)
        {
            return;
        }
        configs
            .editors
            .insert(id, (editor.downgrade(), path.clone(), None));
        let read = cx.background_spawn({
            let path = path.clone();
            async move { editorconfig_for(&path) }
        });
        let weak = editor.downgrade();
        cx.spawn(async move |cx| {
            let config = read.await;
            cx.update(|cx| {
                let Some(editor) = weak.upgrade() else {
                    return;
                };
                // Read for a file the editor has left since.
                if editor.read(cx).model().path() != Some(path.as_path()) {
                    return;
                }
                cx.default_global::<Configs>()
                    .editors
                    .insert(id, (weak, path, Some(config)));
                editor.update(cx, |editor, cx| {
                    apply_config(editor.model_mut(), &config);
                    editor.set_save_fixes(fe_gpui::SaveFixes {
                        trim_trailing_whitespace: config.trim_trailing_whitespace == Some(true),
                        insert_final_newline: config.insert_final_newline == Some(true),
                    });
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

/// The line ending `config` has its file written with, if it sets one
/// fastedit writes.
pub(crate) fn config_line_ending(config: &EditorConfig) -> Option<fe_buffer::LineEnding> {
    match config.end_of_line {
        Some(EndOfLine::Lf) => Some(fe_buffer::LineEnding::Lf),
        Some(EndOfLine::Crlf) => Some(fe_buffer::LineEnding::CrLf),
        Some(EndOfLine::Cr) | None => None,
    }
}

/// Applies `config` to `model`: EditorConfig wins over the indentation
/// detected and over the `editor.tab_size` setting.
fn apply_config(model: &mut EditorModel, config: &EditorConfig) {
    let spaces = |n: usize| IndentStyle::Spaces(u8::try_from(n).unwrap_or(u8::MAX).max(1));
    let size = match config.indent_size {
        Some(IndentSize::Columns(n)) => Some(n),
        Some(IndentSize::Tab) => config.tab_width,
        None => None,
    };
    match (config.indent_style, size) {
        (Some(IndentKind::Tab), _) => model.set_indent_style(Some(IndentStyle::Tabs)),
        (Some(IndentKind::Space), Some(n)) => model.set_indent_style(Some(spaces(n))),
        (None, Some(n)) if matches!(model.indent_style(), Some(IndentStyle::Spaces(_))) => {
            model.set_indent_style(Some(spaces(n)));
        }
        _ => {}
    }
    if let Some(width) = config.tab_width {
        model.set_tab_size(width);
    }
    if let Some(ending) = config_line_ending(config)
        && model.buffer().line_ending() != ending
    {
        model.buffer_mut().set_line_ending(ending);
    }
}

/// A reveal applied to an editor: see [`Pane::revealed`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Revealed {
    /// The [`Pane::open_serial`] of the open it was for.
    pub serial: u64,
    pub editor: EntityId,
    /// The editor's primary selection right after, clamped to its text.
    pub selection: ResolvedSelection,
    /// [`Pane::shown_serial`] right after: while it's unchanged, the pane
    /// has shown nothing else in the editor's place.
    pub shown: u64,
}

/// A range to select in a file, by row, as a search match gives it: so it
/// lands right whatever the file's line endings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reveal {
    /// 0-based.
    pub row: u32,
    /// Byte range in the row's text as the editor decodes it, where an
    /// invalid UTF-8 sequence is a U+FFFD. The search splits lines at `\n`
    /// only, and the editor at a lone `\r` too, so after one of those the
    /// row is off.
    pub columns: Range<usize>,
    /// Clamp `columns` to the row's text, so a column past its end stops at
    /// the end rather than running into the next row.
    pub within_row: bool,
}

impl Reveal {
    /// `range`, in bytes of the whole text as the editor loads it, as
    /// navigation gives one: row 0's columns run on past its end.
    pub(crate) fn bytes(range: Range<usize>) -> Self {
        Self {
            row: 0,
            columns: range,
            within_row: false,
        }
    }

    /// The range in `text`, widened to char boundaries (a byte match can
    /// split a char).
    pub(crate) fn range(&self, text: &fe_text::Text) -> Range<usize> {
        let start = text.row_start(self.row);
        let limit = if self.within_row {
            text.row_len(self.row) as usize
        } else {
            usize::MAX
        };
        let column = |column: usize| start + column.min(limit);
        text.clip_offset(column(self.columns.start), Bias::Left)
            ..text.clip_offset(column(self.columns.end), Bias::Right)
    }

    /// Selects the range ([`Reveal::range`]) against the editor's text as
    /// it is now, and scrolls to it; `select_and_reveal` clamps a stale one.
    fn select(&self, editor: &Entity<EditorView>, cx: &mut App) {
        editor.update(cx, |editor, cx| {
            let range = self.range(editor.model().text());
            editor.select_and_reveal(range, cx);
        });
    }
}

/// Converting a viewer's file to an editor asks first above this size.
const EDIT_WARNING_BYTES: u64 = 1024 * 1024 * 1024;

#[cfg(not(test))]
fn edit_warning(_: &Path) -> u64 {
    EDIT_WARNING_BYTES
}

/// The warning threshold for the files under `dir`, which a test lowers.
#[cfg(test)]
static EDIT_WARNINGS: std::sync::Mutex<Vec<(PathBuf, u64)>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn edit_warning(path: &Path) -> u64 {
    let lowered = EDIT_WARNINGS.lock().unwrap();
    (lowered.iter())
        .find(|(dir, _)| path.starts_with(dir))
        .map_or(EDIT_WARNING_BYTES, |(_, bytes)| *bytes)
}

/// Asks before converting files of more than `bytes` under `dir`, in a
/// test.
#[cfg(test)]
pub(crate) fn lower_edit_warning(dir: &Path, bytes: u64) {
    let mut lowered = EDIT_WARNINGS.lock().unwrap();
    lowered.retain(|(at, _)| at != dir);
    lowered.push((dir.to_owned(), bytes));
}

/// `bytes` for a banner: `1.5 GiB`, `512 MiB`, `12 KiB`.
fn size_text(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    match bytes {
        GIB.. => format!("{:.1} GiB", bytes as f64 / GIB as f64),
        MIB.. => format!("{} MiB", bytes / MIB),
        KIB.. => format!("{} KiB", bytes / KIB),
        _ => format!("{bytes} bytes"),
    }
}

pub(crate) fn file_name(path: Option<&Path>) -> String {
    fe_gpui::pane_item::file_name(path)
}

#[cfg(test)]
impl Pane {
    /// Where tab `ix` was laid out in the last frame.
    pub(crate) fn tab_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.state.tab_bounds(ix)
    }

    /// Scrolls the tab bar left by `by`.
    pub(crate) fn scroll_tabs(&self, by: Pixels) {
        self.state.scroll_tabs(by);
    }

    /// Where the tab bar, left of its buttons, was laid out in the last
    /// frame.
    pub(crate) fn tab_bar_bounds(&self) -> Bounds<Pixels> {
        self.state.tab_bar_bounds()
    }

    /// Where the content below the tab bar was laid out in the last frame.
    pub(crate) fn content_bounds(&self) -> Bounds<Pixels> {
        self.state.content_bounds()
    }

    /// Where the tab bar's button `ix` (a Markdown tab's preview buttons,
    /// Split Right, Split Down, then "…") was laid out in the last frame.
    pub(crate) fn action_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.actions.bounds_for_item(ix)
    }

    /// The tab bar's buttons the last frame drew, in order: ID and icon.
    pub(crate) fn drawn_actions(&self) -> &[(&'static str, Icon)] {
        &self.drawn_actions
    }

    /// The crumbs the last frame drew: label, kind and icon.
    pub(crate) fn drawn_crumbs(&self) -> &[(String, CrumbKind, Option<Icon>)] {
        &self.drawn_crumbs
    }

    /// The text of the last tooltip built.
    pub(crate) fn last_tooltip(&self) -> Option<String> {
        self.last_tooltip.borrow().clone()
    }

    /// Where a hovering tab would drop.
    pub(crate) fn hovered_drop(&self) -> Option<crate::pane_drag::DropTarget> {
        self.state.hovered_drop()
    }

    /// Where the last frame laid out the drop marker or overlay, if one
    /// showed.
    pub(crate) fn drop_hint_bounds(&self) -> Option<Bounds<Pixels>> {
        self.state.drop_hint_bounds()
    }
}

impl EventEmitter<PaneEvent> for Pane {}

impl EventEmitter<TabDropped> for Pane {}

impl EventEmitter<ToolbarRequest> for Pane {}

impl Focusable for Pane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.state.focus().clone()
    }
}

/// kiln-shell's pane operations, with fastedit's state kept in step: a
/// tab shown replaces a file being read or a read error, and the tabs
/// follow the items.
impl PaneView for Pane {
    fn pane(&self) -> &PaneState<Self> {
        &self.state
    }

    fn pane_mut(&mut self) -> &mut PaneState<Self> {
        &mut self.state
    }

    fn new_peer(&self, cx: &mut Context<Self>) -> Self {
        Pane::new_peer(self, cx)
    }

    /// None while a file is being read in, or its read failed: the pane
    /// shows that over its tab. A Markdown preview shown in its tab's
    /// place is shown, and a split copies it.
    fn shown_item(&self) -> Option<&dyn ItemHandle> {
        let item = self.active_item()?;
        match self.active_tab().and_then(Tab::preview) {
            Some(preview) => Some(preview),
            None => Some(item),
        }
    }

    /// Focus goes to a Markdown preview shown in the tab's place, not to
    /// its hidden editor. Read by the item: the tabs may not be in step
    /// with the items yet, as for an item moved in.
    fn activated(&mut self, ix: usize, window: &mut Window, cx: &mut App) {
        let editor =
            (self.state.items().get(ix)).and_then(|item| item.downcast_ref::<EditorView>());
        if let Some(preview) = editor.and_then(|editor| markdown_preview::in_place(editor, cx)) {
            window.focus(&preview.focus_handle(cx), cx);
        }
        if !self.landing {
            self.navigations += 1;
        }
        self.opening = None;
        // A read this replaces opens without its reveal.
        self.reveal = None;
        self.error = None;
        self.note_shown(cx);
    }

    fn items_changed(&mut self, cx: &mut App) {
        // A pane whose last tab went while a file is being read into it
        // isn't emptied: the file lands there, so the workspace keeps it.
        if self.opening.is_some() && self.state.items().is_empty() {
            self.state.take_emptied();
        }
        let before = self.tabs.len();
        self.sync_tabs(cx);
        if self.tabs.len() < before {
            self.navigations += 1;
        }
        if self.tabs.is_empty() {
            self.error = None;
        }
        self.note_shown(cx);
    }
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        crate::peek::sync(self, cx);
        self.state.end_hover(cx);
        let tab_bar = self.state.tab_bar(cx);
        let body = if let Some(path) = &self.opening {
            self.message(format!("Loading {}…", file_name(Some(path))), cx)
        } else if let Some((_, error)) = &self.error {
            self.message(error.clone(), cx)
        } else if let Some((tab, preview)) =
            (self.active_tab()).and_then(|tab| Some((tab, tab.preview()?)))
        {
            // Saving saves the file, from the keys that save in the editor
            // and from the menu; the editor's other commands don't apply.
            let editor = tab.editor().cloned();
            let (saver, keys) = (editor.clone(), self.save_keys.clone());
            div()
                .key_context(MARKDOWN_PREVIEW_CONTEXT)
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if let Some(editor) = &saver
                        && keys.borrow_mut().saves(&event.keystroke, cx)
                    {
                        editor.update(cx, |editor, cx| editor.save_from_command(window, cx));
                        cx.stop_propagation();
                    }
                })
                .on_action(move |run: &RunCommand, window, cx| match &editor {
                    Some(editor) if run.0.command.as_str() == SAVE => {
                        editor.update(cx, |editor, cx| editor.save_from_command(window, cx))
                    }
                    _ => cx.propagate(),
                })
                .size_full()
                .child(preview.clone())
                .into_any_element()
        } else if let Some(tab) = self.active_tab() {
            tab.item().to_any_view().into_any_element()
        } else {
            self.message("No open files".into(), cx)
        };
        let drop_layer = self.state.drop_layer(cx);
        // Zen Mode can leave the tab strip out, the toolbar with it.
        let (tab_row, toolbar_row) = match self.state.tab_bar_hidden() {
            false => {
                let tab_row = self.tab_row(tab_bar, cx).into_any_element();
                let toolbar_row = self.toolbar_row(cx).map(IntoElement::into_any_element);
                (Some(tab_row), toolbar_row)
            }
            true => (None, None),
        };
        Self::attach(div(), cx)
            .track_focus(self.state.focus())
            .capture_any_mouse_down(cx.listener(|_, _, _, cx| cx.emit(PaneEvent::Activated)))
            .on_drag_move(
                cx.listener(|pane, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    if pane.state.drag_moved(event) {
                        cx.notify();
                    }
                }),
            )
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(fe_ui::ui_theme(cx).background)
            .child(self.state.record_bounds())
            .children(tab_row)
            .children(toolbar_row)
            .children(self.find_row(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.state.record_content_bounds())
                    .child(body)
                    .children(crate::peek::layer(self))
                    .children(drop_layer),
            )
            .children(self.state.drop_hint(cx))
    }
}

/// The find bar: see [`crate::find_bar`].
impl Pane {
    /// Does `op` in the find bar, made by the first `find.show` or
    /// `find.show_replace`: before that, nothing else does anything.
    /// Closing it focuses the active editor.
    pub(crate) fn run_find(&mut self, op: FindOp, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(op, FindOp::Show | FindOp::ShowReplace) && self.find.is_none() {
            self.find = Some(FindBar::new(&fe_ui::ui_theme(cx), cx));
        }
        self.update_find(|find, editor, cx| find.run(op, editor, window, cx), cx);
        if op == FindOp::Close {
            window.focus(&self.focus_target(cx), cx);
        }
        cx.notify();
    }

    /// Changes the find bar, if there is one, given the active editor.
    pub(crate) fn update_find(
        &mut self,
        update: impl FnOnce(&mut FindBar, Option<&Entity<EditorView>>, &mut App),
        cx: &mut App,
    ) {
        let editor = self.active_editor().cloned();
        if let Some(find) = &mut self.find {
            update(find, editor.as_ref(), cx);
        }
    }

    fn find_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let editor = self.active_editor().cloned();
        self.find
            .as_ref()?
            .render(editor.as_ref(), &fe_ui::ui_theme(cx), cx)
    }

    /// What the find bar shows beside its query, and whether it's an
    /// error ([`FindBar::label`]); `None` with no bar or nothing shown. For
    /// tests outside the crate.
    #[doc(hidden)]
    pub fn find_label(&self, cx: &App) -> Option<(gpui::SharedString, bool)> {
        let editor = self.active_editor().map(|editor| editor.read(cx));
        self.find.as_ref()?.label(editor)
    }

    #[cfg(test)]
    pub(crate) fn find_bar(&self) -> Option<&FindBar> {
        self.find.as_ref()
    }
}

impl Pane {
    /// A tab dropped on one of the pane's rows: the workbench moves it.
    pub(crate) fn on_drop(
        &mut self,
        dragged: &DraggedTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.dropped(dragged, window.mouse_position(), cx);
    }
}

/// The tab bar's buttons and the toolbar: see [`crate::pane_toolbar`].
impl Pane {
    /// The tab bar, then a Markdown tab's preview buttons, the split and
    /// "…" buttons, which stay put as the tabs scroll, drawn as one bar.
    fn tab_row(&mut self, tab_bar: TabBar, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let markdown = self.markdown_shown(cx);
        // Invoked in the editor, or in the preview shown in its place,
        // which their handlers check.
        let markdown_button = |id, icon, title, command: &'static str, frame: &'static str| {
            self.button(id, icon, title, Some(command), &ui)
                .on_click(move |_, window, cx| dispatch_in(command, frame, window, cx))
        };
        let toggle = markdown.map(|shown| {
            let (icon, title, frame) = match shown {
                MarkdownShown::Source => (Icon::Eye, "Open Preview", fe_editor::commands::CONTEXT),
                MarkdownShown::Preview => (Icon::Code, "Show Source", MARKDOWN_PREVIEW_CONTEXT),
            };
            markdown_button("markdown-toggle", icon, title, TOGGLE_PREVIEW, frame)
        });
        let side = (markdown == Some(MarkdownShown::Source)).then(|| {
            markdown_button(
                "markdown-side",
                Icon::PanelRightOpen,
                "Open Preview to the Side",
                "markdown.show_preview_to_side",
                fe_editor::commands::CONTEXT,
            )
        });
        #[cfg(test)]
        {
            self.drawn_actions = match markdown {
                Some(MarkdownShown::Source) => {
                    vec![
                        ("markdown-toggle", Icon::Eye),
                        ("markdown-side", Icon::PanelRightOpen),
                    ]
                }
                Some(MarkdownShown::Preview) => vec![("markdown-toggle", Icon::Code)],
                None => Vec::new(),
            };
            self.drawn_actions.extend([
                ("split-right", Icon::Columns2),
                ("split-down", Icon::Rows2),
                ("pane-menu", Icon::Ellipsis),
            ]);
        }
        let split = |id, icon, title, command: &'static str| {
            self.button(id, icon, title, Some(command), &ui)
                .on_click(move |_, window, cx| {
                    pane_toolbar::dispatch(command, Source::Mouse, window, cx)
                })
        };
        let split_right = split(
            "split-right",
            Icon::Columns2,
            "Split Right",
            "pane.split_right",
        );
        let split_down = split("split-down", Icon::Rows2, "Split Down", "pane.split_down");
        let more = self
            .button("pane-menu", Icon::Ellipsis, "More Actions", None, &ui)
            .on_click(cx.listener(|pane, _, window, cx| {
                pane.request(ToolbarRequest::PaneCommands, window, cx)
            }));
        let strip = div()
            .id("pane-actions")
            .flex_none()
            .flex()
            .items_center()
            .gap(gpui::px(2.))
            .px_1()
            .h(TAB_HEIGHT)
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .children(toggle)
            .children(side)
            .child(split_right)
            .child(split_down)
            .child(more);
        #[cfg(test)]
        let strip = gpui::StatefulInteractiveElement::track_scroll(strip, &self.actions);
        div()
            .flex()
            .flex_none()
            .w_full()
            .h(TAB_HEIGHT)
            .on_drop(self.state.on_drop(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .child(tab_bar)
                    .child(self.state.record_tab_bar_bounds()),
            )
            .child(strip)
    }

    /// Back and forward, then the crumbs; nothing while hidden.
    fn toolbar_row(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        #[cfg(test)]
        self.drawn_crumbs.clear();
        if !self.toolbar.visible {
            return None;
        }
        let ui = fe_ui::ui_theme(cx);
        let nav = |button: IconButton, enabled: bool, command: &'static str| {
            button.disabled(!enabled).on_click(move |_, window, cx| {
                pane_toolbar::dispatch(command, Source::Mouse, window, cx)
            })
        };
        let NavButtons { back, forward } = self.toolbar.nav;
        let back_button = self.button("back", Icon::ArrowLeft, "Go Back", Some("nav.back"), &ui);
        let forward_button = self.button(
            "forward",
            Icon::ArrowRight,
            "Go Forward",
            Some("nav.forward"),
            &ui,
        );
        let crumbs = self.crumbs(cx);
        #[cfg(test)]
        {
            self.drawn_crumbs = crumbs
                .iter()
                .map(|crumb| (crumb.label.to_string(), crumb.kind, crumb.icon))
                .collect();
        }
        let toolbar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .w_full()
            .h(TOOLBAR_HEIGHT)
            .overflow_hidden()
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .debug_selector(|| "pane-toolbar".into())
            // A tab dropped here goes where the marker in the tab bar says.
            .on_drop(self.state.on_drop(cx))
            .child(nav(back_button, back, "nav.back"))
            .child(nav(forward_button, forward, "nav.forward"))
            .child(Breadcrumbs::new(crumbs, &ui));
        Some(toolbar)
    }

    /// A button with a tooltip of `title` and `command`'s key.
    fn button(
        &self,
        id: &'static str,
        icon: Icon,
        title: &'static str,
        command: Option<&'static str>,
        ui: &UiTheme,
    ) -> IconButton {
        #[cfg(test)]
        let last_tooltip = self.last_tooltip.clone();
        IconButton::new(id, icon, ui).tooltip(move |_, cx| {
            let shortcut = command.and_then(|command| pane_toolbar::shortcut(command, cx));
            let tooltip = Tooltip::new(title, shortcut, &fe_ui::ui_theme(cx));
            #[cfg(test)]
            last_tooltip.replace(Some(tooltip.text()));
            cx.new(|_| tooltip).into()
        })
    }

    /// The path crumbs of the file being read or that failed to be read,
    /// or else of the active item's ([`kiln_shell::PaneItem::path`]), then
    /// the item's own crumbs, then the symbol chain if it's of the editor
    /// shown.
    fn crumbs(&self, cx: &mut Context<Self>) -> Vec<Crumb> {
        let path = match (&self.opening, &self.error) {
            (Some(path), _) | (None, Some((path, _))) => Some(Some(path.clone())),
            (None, None) => self.state.active_item().map(|item| {
                let diff = item.downcast_ref::<DiffView>();
                item.path(cx).or_else(|| {
                    diff.and_then(|diff| {
                        diff.read(cx)
                            .right()
                            .read(cx)
                            .model()
                            .path()
                            .map(Path::to_owned)
                    })
                })
            }),
        };
        let Some(path) = path else {
            return Vec::new();
        };
        let path = path.as_deref();
        // A diff view with no working file on its right (HEAD against the
        // index, a commit's change) is named by its tab's title.
        let diff_title = (path.is_none() && self.opening.is_none() && self.error.is_none())
            .then(|| self.state.active_item())
            .flatten()
            .filter(|item| item.downcast_ref::<DiffView>().is_some())
            .map(|item| item.tab_title(cx));
        let path_crumbs = match diff_title {
            Some(title) => vec![kiln_shell::crumbs::PathCrumb {
                label: title,
                dir: None,
                file: true,
            }],
            None => pane_toolbar::path_crumbs(path, self.toolbar.root.as_deref()),
        };
        let mut crumbs: Vec<_> =
            path_crumbs
                .into_iter()
                .map(|crumb| {
                    let kind = if crumb.file {
                        CrumbKind::File
                    } else {
                        CrumbKind::Path
                    };
                    let mut element = Crumb::new(crumb.label.clone(), kind);
                    if crumb.file {
                        element = element.icon(file_icon(&crumb.label));
                    }
                    let Some(dir) = crumb.dir else {
                        return element;
                    };
                    let select = crumb
                        .file
                        .then(|| path.and_then(Path::file_name).map(ToOwned::to_owned))
                        .flatten();
                    let request = ToolbarRequest::Entries { dir, select };
                    element.on_click(cx.listener(move |pane, _, window, cx| {
                        pane.request(request.clone(), window, cx)
                    }))
                })
                .collect();
        if let Some(item) = self.state.active_item()
            && self.opening.is_none()
            && self.error.is_none()
        {
            let pane = cx.entity().downgrade();
            crumbs.extend(item.breadcrumbs(cx).into_iter().map(|mut crumb| {
                // Focus the item first, so focus goes back to it once a
                // picker the click opens is dismissed.
                crumb.on_click = crumb.on_click.map(|on_click| {
                    let pane = pane.clone();
                    Rc::new(move |window: &mut Window, cx: &mut App| {
                        if let Some(pane) = pane.upgrade() {
                            let target = pane.read(cx).focus_target(cx);
                            window.focus(&target, cx);
                        }
                        on_click(window, cx);
                    }) as kiln_shell::item::CrumbClick
                });
                crumb.into_crumb()
            }));
        }
        let chain = &self.toolbar.symbols;
        let editor = self
            .active_editor()
            .filter(|editor| chain.editor == Some(editor.entity_id()));
        if let Some(editor) = editor {
            let mut names = Vec::new();
            for (item, name, kind) in &chain.items {
                names.push(name.clone());
                let request = ToolbarRequest::Symbols {
                    editor: editor.downgrade(),
                    generation: chain.generation,
                    item: *item,
                    names: names.clone(),
                };
                crumbs.push(
                    Crumb::new(name.clone(), CrumbKind::Symbol)
                        .icon(crate::outline_panel::symbol_icon(*kind))
                        .on_click(cx.listener(move |pane, _, window, cx| {
                            pane.request(request.clone(), window, cx)
                        })),
                );
            }
        }
        crumbs
    }

    /// A toolbar click asks the workbench for `request`, the active editor
    /// focused first, so focus goes back to it once the picker opened is
    /// dismissed.
    fn request(&mut self, request: ToolbarRequest, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_target(cx), cx);
        cx.emit(request);
    }
}

#[cfg(test)]
#[path = "editorconfig_tests.rs"]
mod editorconfig_tests;

#[cfg(test)]
#[path = "editorconfig_save_tests.rs"]
mod editorconfig_save_tests;

#[cfg(test)]
#[path = "preview_tab_tests.rs"]
mod preview_tab_tests;

#[cfg(test)]
#[path = "large_file_tests.rs"]
mod large_file_tests;

#[cfg(test)]
#[path = "large_file_edit_tests.rs"]
mod large_file_edit_tests;

/// `editor.auto_close_pairs` reaches the open editors' models on a reload,
/// and those opened after it.
#[cfg(test)]
mod auto_close_tests {
    use std::sync::Arc;

    use fe_buffer::Buffer;
    use fe_config::Settings;
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::{Launch, Workbench};

    fn reload(workbench: &Entity<Workbench>, on: bool, cx: &mut VisualTestContext) {
        let mut settings = Settings::default();
        settings.editor.auto_close_pairs = on;
        let mut previous = Settings::default();
        previous.editor.auto_close_pairs = !on;
        let event = SettingsEvent {
            settings: Arc::new(settings),
            previous: Arc::new(previous),
            diagnostics: Vec::new(),
        };
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
        cx.run_until_parked();
    }

    /// Each editor's model's [`EditorModel::auto_close`], pane by pane.
    fn auto_close(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<bool> {
        workbench.read_with(cx, |workbench, cx| {
            (workbench.pane_group(cx).panes().iter())
                .flat_map(|pane| pane.read(cx).tabs().to_vec())
                .filter_map(|tab| Some(tab.editor()?.read(cx).model().auto_close()))
                .collect()
        })
    }

    #[gpui::test]
    fn reload_reaches_open_and_new_editors(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        std::fs::write(&path, "f\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "g\n").unwrap();
        let launch = Launch {
            root: Some(dir.path().to_owned()),
            file: Some((path, Some(Buffer::from_file_contents("f\n").into()))),
        };
        let (workbench, cx, _) = crate::tests::open(cx, launch);
        assert_eq!(auto_close(&workbench, cx), [true]);

        reload(&workbench, false, cx);
        assert_eq!(auto_close(&workbench, cx), [false]);
        // A file opened since opens as the settings now say.
        let pane = workbench.read_with(cx, |workbench, cx| {
            workbench.pane_group(cx).panes()[0].clone()
        });
        let b = dir.path().join("b.rs");
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(b, window, cx)));
        cx.run_until_parked();
        assert_eq!(auto_close(&workbench, cx), [false, false]);

        reload(&workbench, true, cx);
        assert_eq!(auto_close(&workbench, cx), [true, true]);
    }
}
