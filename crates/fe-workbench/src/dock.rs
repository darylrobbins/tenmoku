//! The right dock: one panel at a time, the Explorer, project search,
//! Source Control, find references' results, the problems, a file's git
//! history or the AI chat, under a title row naming it, and picked from the
//! activity rail on the window's right edge, which stays when the dock
//! hides. Switching keeps each panel's state. The Explorer is a
//! [`ViewContainer`] of two views, the project tree (Folders) over the
//! outline (collapsed until shown or expanded), whose collapse and weights
//! the dock's saved state keeps; `dock.show_project` shows it, and
//! `outline.show` shows it with the outline expanded. Source Control is
//! another, of the git changes (Changes) over the repository's log
//! (Graph), both expanded ([`source_control`]): `git.show_changes` shows it
//! with Changes expanded, `git.log` with Graph. The References, Problems,
//! History and Chat buttons join the rail once their panel has shown; the
//! History panel is made the first time `git.file_history` runs
//! ([`Dock::set_history`]), and the Chat panel the first time `chat.open`
//! does ([`Dock::set_chat`]).
//!
//! Its commands (`dock.show_project`, `dock.toggle`, the panels' show
//! commands, `search.show_replace`, `search.replace_all` and `git.refresh`)
//! are the workbench's typed commands (`workbench_commands.rs`), which do a
//! [`DockOp`], as a rail click does; the search panel's own are in its
//! registry ([`search_panel::registry`]).

use fe_ui::Icon;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, Render,
    SharedString, Subscription, Window,
};
use kiln_shell::{DockItem, DockPosition, DockState, DockView, PanelId, ViewContainer, ViewSpec};

use crate::{
    ChatPanel, GitLogPanel, GitPanel, HistoryPanel, OutlinePanel, PANEL_WIDTH, ProjectPanel,
    ReferencesPanel,
    ai_changes_panel::AiChangesPanel,
    search_panel::{self, SearchPanel},
};

#[path = "problems_panel.rs"]
pub(crate) mod problems_panel;

pub(crate) use problems_panel::{ProblemsOp, ProblemsPanel};

#[path = "source_control.rs"]
pub(crate) mod source_control;

/// A panel the dock shows: fastedit's name for the [`PanelId`]s it
/// registers, one per [`SidebarPanel`] in [`PANELS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockPanel(PanelId);

/// The panels each module declares, spelled as they were when `DockPanel`
/// was an enum.
#[allow(non_upper_case_globals)]
impl DockPanel {
    pub const Explorer: DockPanel = crate::project_panel::SIDEBAR.panel;
    pub const Search: DockPanel = search_panel::SIDEBAR.panel;
    pub const SourceControl: DockPanel = source_control::SIDEBAR.panel;
    pub const References: DockPanel = crate::references_panel::SIDEBAR.panel;
    pub const Problems: DockPanel = problems_panel::SIDEBAR.panel;
    pub const AiChanges: DockPanel = crate::ai_changes_panel::SIDEBAR.panel;
    pub const History: DockPanel = crate::workbench::git_commands::git_history::SIDEBAR.panel;
    pub const Chat: DockPanel = crate::chat_panel::SIDEBAR.panel;
}

impl DockPanel {
    /// The panels the rail always has, in its order; References, Problems
    /// and History follow once they have shown.
    pub const ALL: [DockPanel; 4] = always_on_rail();

    /// The panel registered as `id`, for a module's [`SidebarPanel`].
    pub(crate) const fn new(id: &'static str) -> Self {
        DockPanel(PanelId(id))
    }

    /// What its module declares of it.
    fn sidebar(self) -> &'static SidebarPanel {
        PANELS
            .iter()
            .find(|sidebar| sidebar.panel == self)
            .expect("every DockPanel is in PANELS")
    }

    pub fn name(self) -> &'static str {
        self.sidebar().name
    }

    /// Its id in the dock.
    pub fn id(self) -> PanelId {
        self.0
    }

    /// The panel registered as `id`.
    fn of(id: PanelId) -> Option<Self> {
        PANELS
            .iter()
            .map(|sidebar| sidebar.panel)
            .find(|panel| panel.id() == id)
    }

    /// The command that shows it.
    fn show_command(self) -> &'static str {
        self.sidebar().show_command
    }

    /// The op of [`DockPanel::show_command`].
    fn show_op(self) -> DockOp {
        self.sidebar().show_op
    }
}

/// A panel of the sidebar, as the module that makes it declares it (its
/// `SIDEBAR`): its id, name, rail icon, show command and that command's op,
/// its place on the rail, whether the rail always has it or it joins once
/// it has shown, and whether showing it focuses it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SidebarPanel {
    pub(crate) panel: DockPanel,
    pub(crate) name: &'static str,
    pub(crate) icon: Icon,
    pub(crate) show_command: &'static str,
    pub(crate) show_op: DockOp,
    /// Lower is higher on the rail.
    pub(crate) order: i32,
    pub(crate) always_on_rail: bool,
    pub(crate) focus_on_show: bool,
}

/// The sidebar's panels, by [`SidebarPanel::order`].
const PANELS: &[&SidebarPanel] = &[
    &crate::project_panel::SIDEBAR,
    &search_panel::SIDEBAR,
    &source_control::SIDEBAR,
    &crate::chat_panel::SIDEBAR,
    &crate::references_panel::SIDEBAR,
    &problems_panel::SIDEBAR,
    &crate::workbench::git_commands::git_history::SIDEBAR,
    &crate::ai_changes_panel::SIDEBAR,
];

const _: () = {
    let mut ix = 1;
    while ix < PANELS.len() {
        assert!(
            PANELS[ix - 1].order < PANELS[ix].order,
            "PANELS is in rail order"
        );
        ix += 1;
    }
};

/// The [`SidebarPanel::always_on_rail`] panels of [`PANELS`], in its order.
const fn always_on_rail<const N: usize>() -> [DockPanel; N] {
    let mut panels = [PANELS[0].panel; N];
    let (mut ix, mut on_rail) = (0, 0);
    while ix < PANELS.len() {
        if PANELS[ix].always_on_rail {
            panels[on_rail] = PANELS[ix].panel;
            on_rail += 1;
        }
        ix += 1;
    }
    assert!(on_rail == N, "DockPanel::ALL has every always-on panel");
    panels
}

/// Its rail button's icon.
pub(crate) fn panel_icon(panel: DockPanel) -> Icon {
    panel.sidebar().icon
}

/// fastedit's panels in kiln-shell's dock: each is what its module's
/// [`SidebarPanel`] declares. Its title row has its name, or what `$title`,
/// a method of the panel's, gives.
macro_rules! dock_item {
    ($panel:ty, $sidebar:expr) => {
        dock_item!($panel, $sidebar, |_: &$panel, _: &App| {
            $sidebar.name.into()
        });
    };
    ($panel:ty, $sidebar:expr, $title:expr) => {
        impl DockItem for $panel {
            fn id(&self) -> PanelId {
                $sidebar.panel.id()
            }

            fn title(&self, cx: &App) -> SharedString {
                $title(self, cx)
            }

            fn icon(&self, _: &App) -> Icon {
                panel_icon($sidebar.panel)
            }

            fn show_command(&self) -> Option<&'static str> {
                Some($sidebar.panel.show_command())
            }

            fn always_on_rail(&self) -> bool {
                $sidebar.always_on_rail
            }

            fn focus_on_show(&self) -> bool {
                $sidebar.focus_on_show
            }
        }
    };
}

dock_item!(SearchPanel, search_panel::SIDEBAR);
dock_item!(ReferencesPanel, crate::references_panel::SIDEBAR);
dock_item!(ProblemsPanel, problems_panel::SIDEBAR);
dock_item!(AiChangesPanel, crate::ai_changes_panel::SIDEBAR);
// Titled with the file it lists.
dock_item!(
    HistoryPanel,
    crate::workbench::git_commands::git_history::SIDEBAR,
    HistoryPanel::title
);
dock_item!(ChatPanel, crate::chat_panel::SIDEBAR);

/// A rail button was clicked; the workbench runs [`Dock::rail_op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockEvent {
    Rail(DockPanel),
}

/// The right dock: kiln-shell's [`DockState`] over fastedit's seven panels,
/// whose rail clicks run the op the panel's key would; the History panel's
/// shows the list last listed.
pub struct Dock {
    state: DockState,
    /// The project tree and the outline, its views.
    explorer: Entity<ViewContainer>,
    project: Entity<ProjectPanel>,
    search: Entity<SearchPanel>,
    outline: Entity<OutlinePanel>,
    /// The git changes and the repository's log, its views.
    source_control: Entity<ViewContainer>,
    changes: Entity<GitPanel>,
    /// What Graph's header says: what the log lists.
    graph_title: SharedString,
    references: Entity<ReferencesPanel>,
    problems: Entity<ProblemsPanel>,
    /// Made the first time it's asked for.
    history: Option<Entity<HistoryPanel>>,
    /// Made with the dock.
    git_log: Option<Entity<GitLogPanel>>,
    /// Made the first time it's asked for.
    chat: Option<Entity<ChatPanel>>,
    /// Made the first time a changeset is shown.
    ai_changes: Option<Entity<AiChangesPanel>>,
    #[cfg(test)]
    ops: Vec<DockOp>,
    /// The explorer's views' collapse and weights are the dock's state,
    /// for the workbench to save.
    _explorer_changed: Subscription,
    /// Source Control's are too.
    _source_control_changed: Subscription,
    /// Retitles Graph when the log lists something else.
    _graph_changed: Subscription,
}

/// The Explorer: `project`'s tree over `outline`, collapsed at first.
fn explorer(
    project: &Entity<ProjectPanel>,
    outline: &Entity<OutlinePanel>,
    cx: &mut Context<Dock>,
) -> Entity<ViewContainer> {
    let sidebar = crate::project_panel::SIDEBAR;
    cx.new(|cx| {
        let mut explorer = ViewContainer::new(sidebar.panel.id(), sidebar.name, sidebar.icon, cx)
            .show_command(sidebar.show_command);
        explorer.add_view(folders(project), cx);
        explorer.add_view(
            ViewSpec {
                id: crate::outline_panel::VIEW,
                title: "Outline".into(),
                order: 20,
                default_collapsed: true,
                view: outline.clone().into(),
            },
            cx,
        );
        explorer
    })
}

/// The Explorer's Folders view: `project`'s tree.
fn folders(project: &Entity<ProjectPanel>) -> ViewSpec {
    ViewSpec {
        id: crate::project_panel::FOLDERS,
        title: "Folders".into(),
        order: 10,
        default_collapsed: false,
        view: project.clone().into(),
    }
}

impl Dock {
    /// Shows the Explorer, of `project` and `outline`, first; Source
    /// Control, of `changes` over `git_log`, follows search on the rail,
    /// and `chat` it.
    #[allow(clippy::too_many_arguments, private_interfaces)]
    pub fn new(
        project: Entity<ProjectPanel>,
        search: Entity<SearchPanel>,
        outline: Entity<OutlinePanel>,
        changes: Entity<GitPanel>,
        git_log: Entity<GitLogPanel>,
        chat: Entity<ChatPanel>,
        references: Entity<ReferencesPanel>,
        problems: Entity<ProblemsPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut state = DockState::new(DockPosition::Right)
            .size(PANEL_WIDTH)
            .shortcut_contexts([crate::KEY_CONTEXT, fe_gpui::KEY_CONTEXT]);
        let explorer = explorer(&project, &outline, cx);
        let explorer_changed = cx.observe(&explorer, |_, _, cx| cx.notify());
        state.add_panel(explorer.clone(), cx);
        state.add_panel(search.clone(), cx);
        let source_control = source_control::container(&changes, &git_log, cx);
        let source_control_changed = cx.observe(&source_control, |_, _, cx| cx.notify());
        state.add_panel(source_control.clone(), cx);
        let graph_changed = cx.observe(&git_log, |dock: &mut Self, git_log, cx| {
            let title = git_log.read(cx).title(cx);
            if title != dock.graph_title {
                dock.graph_title = title;
                let graph = source_control::graph(&git_log, cx);
                (dock.source_control).update(cx, |container, cx| container.add_view(graph, cx));
            }
        });
        // Registered here, not after the References and Problems panels,
        // as the rail lists the panels in the order they were added.
        state.add_panel(chat.clone(), cx);
        state.add_panel(references.clone(), cx);
        state.add_panel(problems.clone(), cx);
        Self {
            state,
            explorer,
            project,
            search,
            outline,
            source_control,
            graph_title: git_log.read(cx).title(cx),
            changes,
            references,
            problems,
            history: None,
            git_log: Some(git_log),
            chat: Some(chat),
            ai_changes: None,
            #[cfg(test)]
            ops: Vec::new(),
            _explorer_changed: explorer_changed,
            _source_control_changed: source_control_changed,
            _graph_changed: graph_changed,
        }
    }

    pub fn project(&self) -> &Entity<ProjectPanel> {
        &self.project
    }

    /// Shows `project` in the old one's place in the Explorer, for a new
    /// root. The dock's visibility and active panel, and the Folders
    /// view's collapse, stay.
    pub(crate) fn set_project(&mut self, project: Entity<ProjectPanel>, cx: &mut Context<Self>) {
        (self.explorer).update(cx, |explorer, cx| explorer.add_view(folders(&project), cx));
        self.project = project;
        cx.notify();
    }

    pub fn search(&self) -> &Entity<SearchPanel> {
        &self.search
    }

    pub fn outline(&self) -> &Entity<OutlinePanel> {
        &self.outline
    }

    /// The Explorer, of the project tree and the outline.
    pub fn explorer(&self) -> &Entity<ViewContainer> {
        &self.explorer
    }

    /// Source Control, of the git changes and the repository's log.
    pub fn source_control(&self) -> &Entity<ViewContainer> {
        &self.source_control
    }

    /// Expands or collapses `container`'s `view`.
    fn set_collapsed(
        container: &Entity<ViewContainer>,
        view: kiln_shell::ViewId,
        collapsed: bool,
        cx: &mut App,
    ) {
        let stack = container.read(cx).stack().clone();
        stack.update(cx, |stack, cx| stack.set_collapsed(view, collapsed, cx));
    }

    pub fn changes(&self) -> &Entity<GitPanel> {
        &self.changes
    }

    /// Shows `changes` in the old one's place in Source Control, for a new
    /// root, as [`Dock::set_project`] does.
    pub(crate) fn set_changes(&mut self, changes: Entity<GitPanel>, cx: &mut Context<Self>) {
        let view = source_control::changes_view(&changes);
        (self.source_control).update(cx, |container, cx| container.add_view(view, cx));
        self.changes = changes;
        cx.notify();
    }

    pub fn references(&self) -> &Entity<ReferencesPanel> {
        &self.references
    }

    pub(crate) fn problems(&self) -> &Entity<ProblemsPanel> {
        &self.problems
    }

    pub(crate) fn history(&self) -> Option<&Entity<HistoryPanel>> {
        self.history.as_ref()
    }

    /// Registers `history`, the History panel made on first use, in the
    /// place of any before it, as [`Dock::set_project`] does.
    pub(crate) fn set_history(&mut self, history: Entity<HistoryPanel>, cx: &mut Context<Self>) {
        self.state.add_panel(history.clone(), cx);
        self.history = Some(history);
        cx.notify();
    }

    pub(crate) fn git_log(&self) -> Option<&Entity<GitLogPanel>> {
        self.git_log.as_ref()
    }

    pub(crate) fn ai_changes(&self) -> Option<&Entity<AiChangesPanel>> {
        self.ai_changes.as_ref()
    }

    /// Registers `panel`, the AI Changes panel made on first use.
    pub(crate) fn set_ai_changes(&mut self, panel: Entity<AiChangesPanel>, cx: &mut Context<Self>) {
        self.state.add_panel(panel.clone(), cx);
        self.ai_changes = Some(panel);
        cx.notify();
    }

    pub(crate) fn chat(&self) -> Option<&Entity<ChatPanel>> {
        self.chat.as_ref()
    }

    /// The dock's state, in kiln-shell.
    pub fn state(&self) -> &DockState {
        &self.state
    }

    /// The panel shown while the dock is visible.
    pub fn active(&self) -> DockPanel {
        self.state
            .active()
            .and_then(DockPanel::of)
            .unwrap_or(DockPanel::Explorer)
    }

    pub fn is_visible(&self) -> bool {
        self.state.is_visible()
    }

    /// The panel whose rail button is highlighted: the active one, while
    /// the dock shows.
    pub fn rail_highlighted(&self) -> Option<DockPanel> {
        self.state.rail_highlighted().and_then(DockPanel::of)
    }

    /// What clicking `panel`'s rail button does: hides the dock if it's the
    /// highlighted one, else shows the dock with it, as its show command
    /// does.
    pub(crate) fn rail_op(&self, panel: DockPanel) -> DockOp {
        if self.rail_highlighted() == Some(panel) {
            DockOp::Toggle
        } else {
            panel.show_op()
        }
    }

    /// Does a dock op, for a key or a rail click: showing search focuses its
    /// input, and showing Source Control the view it shows. Focus on a
    /// panel the op hides stays: the workbench moves it.
    pub(crate) fn run(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) -> bool {
        #[cfg(test)]
        self.ops.push(op);
        // A panel's show op shows it: History and Chat nothing until they
        // have been made.
        if let Some(sidebar) = PANELS.iter().find(|sidebar| sidebar.show_op == op) {
            self.show(sidebar.panel, window, cx);
            match op {
                // The tree the workbench focuses.
                DockOp::ShowProject => {
                    Self::set_collapsed(&self.explorer, crate::project_panel::FOLDERS, false, cx);
                }
                DockOp::ShowChanges => {
                    Self::set_collapsed(&self.source_control, source_control::CHANGES, false, cx);
                    window.focus(&self.changes.focus_handle(cx), cx);
                }
                _ => {}
            }
            return true;
        }
        match op {
            DockOp::ShowOutline => {
                self.show(DockPanel::Explorer, window, cx);
                Self::set_collapsed(&self.explorer, crate::outline_panel::VIEW, false, cx);
                true
            }
            DockOp::ShowGitLog => {
                self.show(DockPanel::SourceControl, window, cx);
                Self::set_collapsed(&self.source_control, source_control::GRAPH, false, cx);
                if let Some(git_log) = &self.git_log {
                    window.focus(&git_log.focus_handle(cx), cx);
                }
                true
            }
            // The workbench's to do.
            DockOp::Problems(_) => false,
            DockOp::RefreshChanges => {
                self.changes.update(cx, GitPanel::refresh);
                true
            }
            DockOp::Toggle => {
                self.state.toggle();
                cx.notify();
                true
            }
            DockOp::ShowReplace => {
                self.show(DockPanel::Search, window, cx);
                self.search
                    .update(cx, |search, cx| search.show_replace(true, window, cx));
                true
            }
            DockOp::ReplaceAll => {
                self.search
                    .update(cx, |search, cx| search.confirm_replace_all(window, cx));
                true
            }
            // The panels' show ops, done above.
            _ => false,
        }
    }

    /// Shows the dock with `panel`; search focuses its input.
    pub fn show(&mut self, panel: DockPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.state.show(panel.id(), window, cx);
        cx.notify();
    }

    /// Puts back the state [`DockState::save`] saved: whether it shows, its
    /// active panel, whether References has joined the rail and the
    /// Explorer's and Source Control's views' collapse and weights. One
    /// saved before them, with no views, leaves them as built; its Outline
    /// panel active shows the Explorer, its Changes panel Source Control
    /// and its Git Log panel Source Control with Graph expanded.
    pub(crate) fn restore(&mut self, saved: &fe_command::Value, cx: &mut Context<Self>) {
        let mut saved = saved.clone();
        let active = saved.get("active").and_then(fe_command::Value::as_str);
        let graph = active == Some(crate::workbench::git_commands::git_log::OLD_PANEL);
        let container = match active {
            Some("outline") => Some(DockPanel::Explorer),
            Some("changes" | crate::workbench::git_commands::git_log::OLD_PANEL) => {
                Some(DockPanel::SourceControl)
            }
            _ => None,
        };
        if let Some(container) = container {
            saved["active"] = container.id().0.into();
        }
        self.state.restore(&saved, cx);
        if graph {
            Self::set_collapsed(&self.source_control, source_control::GRAPH, false, cx);
        }
        cx.notify();
    }

    /// Adds a rail action, pinned to the rail's bottom.
    pub(crate) fn add_action(&mut self, action: kiln_shell::RailAction, cx: &mut Context<Self>) {
        self.state.add_action(action);
        cx.notify();
    }

    /// Draws the rail action `id` highlighted, or not, redrawing the rail
    /// if that changes.
    pub(crate) fn set_action_highlighted(
        &mut self,
        id: &str,
        highlighted: bool,
        cx: &mut Context<Self>,
    ) {
        self.state.set_action_highlighted(id, highlighted);
        cx.notify();
    }

    /// Whether a panel that isn't shown has focus.
    pub(crate) fn hidden_panel_focused(&self, window: &Window, cx: &App) -> bool {
        self.state.hidden_panel_focused(window, cx)
    }
}

#[cfg(test)]
impl Dock {
    /// The title row's text: the active panel's name.
    pub(crate) fn title(&self) -> SharedString {
        self.active().name().into()
    }

    /// What Source Control's Graph header says.
    pub(crate) fn graph_title(&self) -> &SharedString {
        &self.graph_title
    }

    /// Where the rail's button for `panel` was laid out in the last frame.
    pub(crate) fn rail_button_bounds(
        &self,
        panel: DockPanel,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.state.drawn().rail_button_bounds(panel.id())
    }

    /// Where the rail was laid out in the last frame.
    pub(crate) fn rail_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.state.drawn().rail_bounds()
    }

    /// Where the dock's column, its title row then its panel, was laid out
    /// in the last frame, if it showed.
    pub(crate) fn column_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.state.drawn().column_bounds()
    }

    /// The icons the last frame's rail buttons drew, top to bottom.
    pub(crate) fn rail_icons(&self) -> Vec<Icon> {
        self.state.drawn().rail_icons()
    }

    /// Every op [`Dock::run`] was asked to do, oldest first.
    pub(crate) fn ops(&self) -> &[DockOp] {
        &self.ops
    }

    /// The text of the last tooltip built.
    pub(crate) fn last_tooltip(&self) -> Option<String> {
        self.state.drawn().last_tooltip()
    }
}

impl EventEmitter<DockEvent> for Dock {}

impl DockView for Dock {
    fn dock(&self) -> &DockState {
        &self.state
    }

    fn dock_mut(&mut self) -> &mut DockState {
        &mut self.state
    }

    /// The workbench runs the op the panel's key would, or hides the dock.
    fn rail_clicked(&mut self, panel: PanelId, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = DockPanel::of(panel) {
            cx.emit(DockEvent::Rail(panel));
        }
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.state.render(cx)
    }
}

#[cfg(test)]
#[path = "sidebar_panels_tests.rs"]
mod sidebar_panels_tests;

#[cfg(test)]
#[path = "explorer_views_tests.rs"]
mod explorer_views_tests;

#[cfg(test)]
#[path = "source_control_views_tests.rs"]
mod source_control_views_tests;

/// What a dock command, or a rail click, asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockOp {
    ShowSearch,
    ShowProject,
    /// `outline.show`: shows the Explorer with its outline expanded.
    ShowOutline,
    /// The References button: shows the last results, finding none.
    ShowReferences,
    /// `git.show_changes`: shows Source Control with Changes expanded and
    /// focused.
    ShowChanges,
    /// The History button: shows the history last listed.
    ShowHistory,
    /// `git.log` and `git.log_all`, once they have asked for the list:
    /// show Source Control with Graph expanded and focused.
    ShowGitLog,
    /// The Chat button, and `chat.open` once it has made the panel.
    ShowChat,
    /// `ai.show_changes`, and a changeset set: shows the AI Changes panel.
    ShowAiChanges,
    /// `git.refresh`: reads the git status again, shown or not.
    RefreshChanges,
    Toggle,
    /// `search.show_replace`: shows search with its replace field, focused.
    ShowReplace,
    /// `search.replace_all`: asks, then replaces in the results' files.
    ReplaceAll,
    /// A problems command: `problems.show` is the dock's to do, the others
    /// the workbench's.
    Problems(ProblemsOp),
}
