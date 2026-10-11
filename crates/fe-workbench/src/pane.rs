//! The editor pane: a tab per open file, the active one's editor below.
//! Its tabs, their strip and their drag and drop are kiln-shell's
//! ([`PaneState`], whose items are the editors); opening files, revealing
//! a place in them, and what shows while a file is read or failed to be,
//! are fastedit's. [`Pane::tabs`] keeps a [`Tab`] per item in step.

use std::{
    collections::HashMap,
    io,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{Bias, Buffer, ResolvedSelection};
use fe_command::Source;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use fe_ui::{
    Breadcrumbs, Crumb, CrumbKind, Icon, IconButton, TAB_HEIGHT, TOOLBAR_HEIGHT, TabBar, Tooltip,
    UiTheme, file_icon,
};
use gpui::{
    App, AppContext as _, Bounds, Context, DragMoveEvent, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, Styled as _, Task, Window, div,
};
pub use kiln_shell::PaneEvent;
use kiln_shell::{ItemHandle, ItemId, PaneState, PaneView};

use crate::{
    find_bar::{self, FindBar, FindOp},
    loader,
    nav::Place,
    pane_drag::{DraggedTab, TabDropped},
    pane_toolbar::{self, NavButtons, Toolbar, ToolbarRequest},
};

/// A tab's ID, which stays with it as tabs open, close and move: its
/// item's, the editor's.
pub(crate) type TabId = ItemId;

/// An open file: the pane's item, its editor, as fastedit reads it. The
/// editor is the tab's [`PaneItem`](kiln_shell::PaneItem) ([`Tab::item`]),
/// which the tab strip and the window's title read.
#[derive(Clone)]
pub struct Tab {
    /// `None` for a buffer that was never saved, such as the welcome text.
    pub path: Option<PathBuf>,
    pub editor: Entity<EditorView>,
}

impl Tab {
    /// The tab for `item`, an editor, its path as it is now.
    fn of(item: &dyn ItemHandle, cx: &App) -> Self {
        let editor = item
            .downcast::<EditorView>()
            .expect("fastedit's pane items are editors");
        let path = editor.read(cx).model().path().map(Path::to_owned);
        Self { path, editor }
    }

    pub(crate) fn id(&self) -> TabId {
        self.item().id()
    }

    /// The tab's item: its editor, as kiln-shell holds items.
    pub fn item(&self) -> &dyn ItemHandle {
        &self.editor
    }
}

/// Every pane of a workbench, each pane included, so a pane can see what
/// the others show.
pub(crate) type Peers = kiln_shell::pane::Peers<Pane>;

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
    theme: EditorTheme,
    /// For editors yet to open.
    settings: EditorSettings,
    ui: UiTheme,
    toolbar: Toolbar,
    /// Made by the first `find.show`.
    find: Option<FindBar>,
    /// The tab bar's buttons' laid-out bounds.
    #[cfg(test)]
    actions: gpui::ScrollHandle,
    /// The crumbs the last frame drew: label, kind and icon.
    #[cfg(test)]
    drawn_crumbs: Vec<(String, CrumbKind, Option<Icon>)>,
    #[cfg(test)]
    last_tooltip: Rc<std::cell::RefCell<Option<String>>>,
}

impl Pane {
    pub fn new(
        theme: EditorTheme,
        settings: EditorSettings,
        ui: UiTheme,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state: PaneState::new(ui, cx),
            tabs: Vec::new(),
            opening: None,
            reads: HashMap::new(),
            error: None,
            reveal: None,
            open_serial: 0,
            navigations: 0,
            landing: false,
            shown: None,
            shown_serial: 0,
            revealed: None,
            theme,
            settings,
            ui,
            toolbar: Toolbar::default(),
            find: None,
            #[cfg(test)]
            actions: gpui::ScrollHandle::new(),
            #[cfg(test)]
            drawn_crumbs: Vec::new(),
            #[cfg(test)]
            last_tooltip: Rc::default(),
        }
    }

    /// A pane like this one, empty, opening editors as it does.
    pub(crate) fn new_peer(&self, cx: &mut Context<Self>) -> Self {
        let mut pane = Self::new(self.theme.clone(), self.settings.clone(), self.ui, cx);
        pane.state.set_peers(self.state.peers().clone());
        pane
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

    /// Joins the workbench's panes, `peers`.
    pub(crate) fn set_peers(&mut self, peers: Peers) {
        self.state.set_peers(peers);
    }

    /// Where the last frame laid the pane out; zero-sized before that.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.state.bounds()
    }

    /// Whether the pane's last tab closed, and no file has opened in it
    /// since or is being read into it. Asking clears it once it's true.
    pub(crate) fn take_emptied(&mut self) -> bool {
        if self.opening.is_some() && self.state.items().is_empty() {
            return false;
        }
        self.state.take_emptied()
    }

    /// Whether the pane is reading `path` to show it.
    pub(crate) fn is_opening(&self, path: &Path) -> bool {
        self.opening.as_deref() == Some(path)
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

    /// The active tab's editor, unless a file being opened or a read error
    /// shows in its place.
    pub fn active_editor(&self) -> Option<&Entity<EditorView>> {
        if self.opening.is_some() || self.error.is_some() {
            return None;
        }
        self.state.active_item()?.downcast_ref::<EditorView>()
    }

    /// The active tab's item, its editor, unless a file being opened or a
    /// read error shows in its place: what the status bar follows.
    pub fn active_item(&self) -> Option<&dyn ItemHandle> {
        self.active_editor().map(|editor| editor as &dyn ItemHandle)
    }

    /// What gets focus when focus leaves a hidden panel: the active editor,
    /// or else the pane.
    pub(crate) fn focus_target(&self, cx: &App) -> FocusHandle {
        match self.active_editor() {
            Some(editor) => editor.focus_handle(cx),
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

    /// Opens `item`, an editor, in a new tab and shows it, without
    /// focusing it.
    pub(crate) fn add_item(&mut self, item: Box<dyn ItemHandle>, cx: &mut Context<Self>) {
        let ix = self.state.push(item, cx);
        self.state.set_active(ix);
        self.error = None;
        self.sync_tabs(cx);
        self.note_shown(cx);
        cx.notify();
    }

    /// A tab per item, reusing the tab of an item still open.
    fn sync_tabs(&mut self, cx: &App) {
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
    }

    /// Shows the tab for `path` if it's open; otherwise reads the file in the
    /// background and opens it in a new tab, focused, once read. Meanwhile
    /// the pane is focused, as the editor it showed is hidden.
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_at(path, None, window, cx);
    }

    /// [`Pane::open`], then selects `reveal` in the file's editor: at once if
    /// it's open, else once it's read, if it's still the file opened last.
    pub(crate) fn open_at(
        &mut self,
        path: PathBuf,
        reveal: Option<Reveal>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reveal = None;
        self.open_serial += 1;
        if let Some(ix) = self.position(&path) {
            self.activate(ix, window, cx);
            if let Some(reveal) = reveal {
                self.apply(&reveal, ix, cx);
            }
            return;
        }
        if let Some(model) = self.peer_model(&path, cx) {
            self.add(model, cx);
            let ix = self.tabs.len() - 1;
            self.activate(ix, window, cx);
            if let Some(reveal) = reveal {
                self.apply(&reveal, ix, cx);
            }
            return;
        }
        self.reveal = reveal.map(|reveal| (path.clone(), reveal));
        self.opening = Some(path.clone());
        self.error = None;
        self.note_shown(cx);
        window.focus(self.state.focus(), cx);
        cx.notify();
        if self.reads.contains_key(&path) {
            return;
        }
        let read = cx.background_spawn({
            let path = path.clone();
            async move { loader::read(path) }
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

    /// Shows `path` as being read, by the host, which hands the result to
    /// [`Pane::file_read`].
    pub fn opening(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.opening = Some(path);
        self.note_shown(cx);
        cx.notify();
    }

    /// Opens a finished read of `path` (`Ok(None)` is a new file) in a new
    /// tab, unless it's open already; if another pane has it open by now,
    /// the tab shares that pane's buffer instead. The tab is shown and focused if it's
    /// the file opened last, or if the pane shows nothing else (no tab,
    /// read or error); an error is shown in the tab's place, with the pane
    /// focused, only for the file opened last. So an older read never hides
    /// a newer one or its error, and never shows without focus.
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: io::Result<Option<Buffer>>,
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
        result: io::Result<Option<Buffer>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reads.remove(&path);
        let last = self.opening.as_ref() == Some(&path);
        if last {
            self.opening = None;
        }
        let empty = self.active().is_none() && self.opening.is_none() && self.error.is_none();
        let reveal = self.reveal.take_if(|(at, _)| last && *at == path);
        match result {
            Ok(buffer) => {
                let open = self.position(&path);
                let ix = open.unwrap_or_else(|| {
                    let model = self.peer_model(&path, cx).unwrap_or_else(|| {
                        EditorModel::new(buffer.unwrap_or_else(|| Buffer::new("")), Some(path))
                    });
                    let editor = self.new_editor(model, cx);
                    let ix = self.state.push(Box::new(editor), cx);
                    self.sync_tabs(cx);
                    ix
                });
                if last || empty {
                    self.activate(ix, window, cx);
                    cx.emit(PaneEvent::Activated);
                }
                if let Some((_, reveal)) = reveal {
                    self.apply(&reveal, ix, cx);
                }
            }
            Err(error) => {
                log::error!("cannot open {}: {error}", path.display());
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

    /// Selects `reveal` in tab `ix`'s editor, for the open last made.
    fn apply(&mut self, reveal: &Reveal, ix: usize, cx: &mut App) {
        let editor = &self.tabs[ix].editor;
        reveal.select(editor, cx);
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

    /// A model over the buffer of another pane's tab for `path`, if one has
    /// it open.
    fn peer_model(&self, path: &Path, cx: &App) -> Option<EditorModel> {
        let item = self
            .state
            .peer_item(cx, |item| item.path(cx).as_deref() == Some(path))?;
        let editor = item.downcast::<EditorView>()?;
        Some(editor.read(cx).model().split())
    }

    /// Applies reloaded settings to the open editors and to editors yet to
    /// open.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        self.settings = event.settings.editor.clone();
        self.theme.set_metrics(&self.settings, cx);
        for tab in &self.tabs {
            tab.editor
                .update(cx, |editor, cx| editor.reload_settings(event, cx));
        }
        cx.notify();
    }

    pub(crate) fn position(&self, path: &Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.path.as_deref() == Some(path))
    }

    fn new_editor(&self, mut model: EditorModel, cx: &mut Context<Self>) -> Entity<EditorView> {
        if model.tab_size() != self.settings.tab_size {
            model.set_tab_size(self.settings.tab_size);
        }
        if model.soft_wrap() != self.settings.soft_wrap {
            model.set_soft_wrap(self.settings.soft_wrap);
        }
        let theme = self.theme.clone();
        cx.new(|cx| EditorView::new(model, theme, cx))
    }

    fn message(&self, text: String) -> gpui::AnyElement {
        div()
            .size_full()
            .px_3()
            .py_2()
            .text_color(self.ui.muted_text)
            .child(text)
            .into_any_element()
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
}

impl Reveal {
    /// `range`, in bytes of the whole text as the editor loads it, as
    /// navigation gives one: row 0's columns run on past its end.
    pub(crate) fn bytes(range: Range<usize>) -> Self {
        Self {
            row: 0,
            columns: range,
        }
    }

    /// Selects the range, widened to char boundaries (a byte match can
    /// split a char), against the editor's text as it is now, and scrolls
    /// to it; `select_and_reveal` clamps a stale one.
    fn select(&self, editor: &Entity<EditorView>, cx: &mut App) {
        editor.update(cx, |editor, cx| {
            let text = editor.model().text();
            let start = text.row_start(self.row);
            let range = text.clip_offset(start + self.columns.start, Bias::Left)
                ..text.clip_offset(start + self.columns.end, Bias::Right);
            editor.select_and_reveal(range, cx);
        });
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

    /// Where the tab bar's button `ix` (Split Right, Split Down, then "…")
    /// was laid out in the last frame.
    pub(crate) fn action_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.actions.bounds_for_item(ix)
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

    fn activated(&mut self, _: usize, _: &mut Window, cx: &mut App) {
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.state.end_hover(cx);
        let tab_bar = self.state.tab_bar(cx);
        let body = if let Some(path) = &self.opening {
            self.message(format!("Loading {}…", file_name(Some(path))))
        } else if let Some((_, error)) = &self.error {
            self.message(error.clone())
        } else if let Some(tab) = self.tabs.get(self.state.active().unwrap_or(usize::MAX)) {
            tab.editor.clone().into_any_element()
        } else {
            self.message("No open files".into())
        };
        let drop_layer = self.state.drop_layer(cx);
        div()
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
            .bg(self.ui.background)
            .child(self.state.record_bounds())
            .on_action(cx.listener(Self::on_run_command))
            .child(self.tab_row(tab_bar, cx))
            .children(self.toolbar_row(cx))
            .children(self.find_row(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.state.record_content_bounds())
                    .child(body)
                    .children(drop_layer),
            )
            .children(self.state.drop_hint(cx))
    }
}

/// The find bar: see [`crate::find_bar`].
impl Pane {
    /// Runs a `find.*` command; anything else goes on.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        match find_bar::op(run, cx) {
            Some(op) => self.run_find(op, window, cx),
            None => cx.propagate(),
        }
    }

    /// Does `op` in the find bar, made by the first `find.show`: before
    /// that, nothing else does anything. Closing it focuses the active
    /// editor.
    pub(crate) fn run_find(&mut self, op: FindOp, window: &mut Window, cx: &mut Context<Self>) {
        if op == FindOp::Show && self.find.is_none() {
            self.find = Some(FindBar::new(&self.ui, cx));
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
        self.find.as_ref()?.render(editor.as_ref(), &self.ui, cx)
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
    /// The tab bar, then the split and "…" buttons, which stay put as the
    /// tabs scroll, drawn as one bar.
    fn tab_row(&mut self, tab_bar: TabBar, cx: &mut Context<Self>) -> impl IntoElement {
        let split = |id, icon, title, command: &'static str| {
            self.button(id, icon, title, Some(command))
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
            .button("pane-menu", Icon::Ellipsis, "More Actions", None)
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
            .bg(self.ui.panel_background)
            .border_b_1()
            .border_color(self.ui.border)
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
        let nav = |button: IconButton, enabled: bool, command: &'static str| {
            button.disabled(!enabled).on_click(move |_, window, cx| {
                pane_toolbar::dispatch(command, Source::Mouse, window, cx)
            })
        };
        let NavButtons { back, forward } = self.toolbar.nav;
        let back_button = self.button("back", Icon::ArrowLeft, "Go Back", Some("nav.back"));
        let forward_button = self.button(
            "forward",
            Icon::ArrowRight,
            "Go Forward",
            Some("nav.forward"),
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
            .bg(self.ui.panel_background)
            .border_b_1()
            .border_color(self.ui.border)
            .debug_selector(|| "pane-toolbar".into())
            // A tab dropped here goes where the marker in the tab bar says.
            .on_drop(self.state.on_drop(cx))
            .child(nav(back_button, back, "nav.back"))
            .child(nav(forward_button, forward, "nav.forward"))
            .child(Breadcrumbs::new(crumbs, &self.ui));
        Some(toolbar)
    }

    /// A button with a tooltip of `title` and `command`'s key.
    fn button(
        &self,
        id: &'static str,
        icon: Icon,
        title: &'static str,
        command: Option<&'static str>,
    ) -> IconButton {
        let ui = self.ui;
        #[cfg(test)]
        let last_tooltip = self.last_tooltip.clone();
        IconButton::new(id, icon, &self.ui).tooltip(move |_, cx| {
            let shortcut = command.and_then(|command| pane_toolbar::shortcut(command, cx));
            let tooltip = Tooltip::new(title, shortcut, &ui);
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
            (None, None) => self.state.active_item().map(|item| item.path(cx)),
        };
        let Some(path) = path else {
            return Vec::new();
        };
        let path = path.as_deref();
        let path_crumbs = pane_toolbar::path_crumbs(path, self.toolbar.root.as_deref());
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
            for (item, name) in &chain.items {
                names.push(name.clone());
                let request = ToolbarRequest::Symbols {
                    editor: editor.downgrade(),
                    generation: chain.generation,
                    item: *item,
                    names: names.clone(),
                };
                crumbs.push(
                    Crumb::new(name.clone(), CrumbKind::Symbol).on_click(cx.listener(
                        move |pane, _, window, cx| pane.request(request.clone(), window, cx),
                    )),
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
