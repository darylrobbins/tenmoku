//! The right dock: one panel at a time, the project tree, project search,
//! the outline, the git changes, find references' results, the problems, a
//! file's git history or the repository's log, under a title row naming it,
//! and picked from the activity rail on the window's right edge, which
//! stays when the dock hides. Switching keeps each panel's state. The
//! References, Problems, History and Git Log buttons join the rail once
//! their panel has shown; the History panel is made the first time
//! `git.file_history` runs ([`Dock::set_history`]), the Git Log panel the
//! first time `git.log` or `git.log_all` does ([`Dock::set_git_log`]).
//!
//! Its commands (`dock.show_project`, `dock.toggle`, the panels' show
//! commands, `search.show_replace`, `search.replace_all` and `git.refresh`)
//! are the workbench's typed commands (`workbench_commands.rs`), which do a
//! [`DockOp`], as a rail click does; the search panel's own are in its
//! registry ([`search_panel::registry`]).

use fe_ui::Icon;
use gpui::{App, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Window};
use kiln_shell::{DockItem, DockPosition, DockState, DockView, PanelId};

use crate::{
    GitLogPanel, GitPanel, HistoryPanel, OutlinePanel, PANEL_WIDTH, ProjectPanel, ReferencesPanel,
    search_panel::{self, SearchPanel},
};

#[path = "problems_panel.rs"]
pub(crate) mod problems_panel;

pub(crate) use problems_panel::{ProblemsOp, ProblemsPanel};

/// A panel the dock shows: fastedit's name for the [`PanelId`]s it
/// registers, one per [`SidebarPanel`] in [`PANELS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockPanel(PanelId);

/// The panels each module declares, spelled as they were when `DockPanel`
/// was an enum.
#[allow(non_upper_case_globals)]
impl DockPanel {
    pub const Project: DockPanel = crate::project_panel::SIDEBAR.panel;
    pub const Search: DockPanel = search_panel::SIDEBAR.panel;
    pub const Outline: DockPanel = crate::outline_panel::SIDEBAR.panel;
    pub const Changes: DockPanel = crate::git_panel::SIDEBAR.panel;
    pub const References: DockPanel = crate::references_panel::SIDEBAR.panel;
    pub const Problems: DockPanel = problems_panel::SIDEBAR.panel;
    pub const History: DockPanel = crate::workbench::git_commands::git_history::SIDEBAR.panel;
    pub const GitLog: DockPanel = crate::workbench::git_commands::git_log::SIDEBAR.panel;
}

impl DockPanel {
    /// The panels the rail always has, in its order; References, Problems,
    /// History and Git Log follow once they have shown.
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
    &crate::outline_panel::SIDEBAR,
    &crate::git_panel::SIDEBAR,
    &crate::references_panel::SIDEBAR,
    &problems_panel::SIDEBAR,
    &crate::workbench::git_commands::git_history::SIDEBAR,
    &crate::workbench::git_commands::git_log::SIDEBAR,
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

dock_item!(ProjectPanel, crate::project_panel::SIDEBAR);
dock_item!(SearchPanel, search_panel::SIDEBAR);
dock_item!(OutlinePanel, crate::outline_panel::SIDEBAR);
dock_item!(GitPanel, crate::git_panel::SIDEBAR);
dock_item!(ReferencesPanel, crate::references_panel::SIDEBAR);
dock_item!(ProblemsPanel, problems_panel::SIDEBAR);
// Titled with the file it lists.
dock_item!(
    HistoryPanel,
    crate::workbench::git_commands::git_history::SIDEBAR,
    HistoryPanel::title
);
// Titled with the branch it lists.
dock_item!(
    GitLogPanel,
    crate::workbench::git_commands::git_log::SIDEBAR,
    GitLogPanel::title
);

/// A rail button was clicked; the workbench runs [`Dock::rail_op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockEvent {
    Rail(DockPanel),
}

/// The right dock: kiln-shell's [`DockState`] over fastedit's eight panels,
/// whose rail clicks run the op the panel's key would; the History and Git
/// Log panels' show the list last listed.
pub struct Dock {
    state: DockState,
    project: Entity<ProjectPanel>,
    search: Entity<SearchPanel>,
    outline: Entity<OutlinePanel>,
    changes: Entity<GitPanel>,
    references: Entity<ReferencesPanel>,
    problems: Entity<ProblemsPanel>,
    /// Made the first time it's asked for.
    history: Option<Entity<HistoryPanel>>,
    /// Made the first time it's asked for.
    git_log: Option<Entity<GitLogPanel>>,
    #[cfg(test)]
    ops: Vec<DockOp>,
}

impl Dock {
    /// Shows `project` first.
    pub fn new(
        project: Entity<ProjectPanel>,
        search: Entity<SearchPanel>,
        outline: Entity<OutlinePanel>,
        changes: Entity<GitPanel>,
        references: Entity<ReferencesPanel>,
        problems: Entity<ProblemsPanel>,
        cx: &App,
    ) -> Self {
        let mut state = DockState::new(DockPosition::Right)
            .size(PANEL_WIDTH)
            .shortcut_contexts([crate::KEY_CONTEXT, fe_gpui::KEY_CONTEXT]);
        state.add_panel(project.clone(), cx);
        state.add_panel(search.clone(), cx);
        state.add_panel(outline.clone(), cx);
        state.add_panel(changes.clone(), cx);
        state.add_panel(references.clone(), cx);
        state.add_panel(problems.clone(), cx);
        Self {
            state,
            project,
            search,
            outline,
            changes,
            references,
            problems,
            history: None,
            git_log: None,
            #[cfg(test)]
            ops: Vec::new(),
        }
    }

    pub fn project(&self) -> &Entity<ProjectPanel> {
        &self.project
    }

    /// Shows `project` in the old one's place, for a new root. The dock's
    /// visibility and active panel stay.
    pub(crate) fn set_project(&mut self, project: Entity<ProjectPanel>, cx: &mut Context<Self>) {
        self.state.add_panel(project.clone(), cx);
        self.project = project;
        cx.notify();
    }

    pub fn search(&self) -> &Entity<SearchPanel> {
        &self.search
    }

    pub fn outline(&self) -> &Entity<OutlinePanel> {
        &self.outline
    }

    pub fn changes(&self) -> &Entity<GitPanel> {
        &self.changes
    }

    /// Shows `changes` in the old one's place, for a new root, as
    /// [`Dock::set_project`] does.
    pub(crate) fn set_changes(&mut self, changes: Entity<GitPanel>, cx: &mut Context<Self>) {
        self.state.add_panel(changes.clone(), cx);
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

    /// Registers `git_log`, the Git Log panel made on first use, as
    /// [`Dock::set_history`] does.
    pub(crate) fn set_git_log(&mut self, git_log: Entity<GitLogPanel>, cx: &mut Context<Self>) {
        self.state.add_panel(git_log.clone(), cx);
        self.git_log = Some(git_log);
        cx.notify();
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
            .unwrap_or(DockPanel::Project)
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
    /// input. Focus on a panel the op hides stays: the workbench moves it.
    pub(crate) fn run(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) -> bool {
        #[cfg(test)]
        self.ops.push(op);
        // A panel's show op shows it: History and Git Log nothing until
        // they have been made.
        if let Some(sidebar) = PANELS.iter().find(|sidebar| sidebar.show_op == op) {
            self.show(sidebar.panel, window, cx);
            return true;
        }
        match op {
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
    /// active panel and whether References has joined the rail.
    pub(crate) fn restore(&mut self, saved: &fe_command::Value, cx: &mut Context<Self>) {
        self.state.restore(saved, cx);
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

/// What a dock command, or a rail click, asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockOp {
    ShowSearch,
    ShowProject,
    /// `outline.show`.
    ShowOutline,
    /// The References button: shows the last results, finding none.
    ShowReferences,
    /// `git.show_changes`.
    ShowChanges,
    /// The History button: shows the history last listed.
    ShowHistory,
    /// The Git Log button: shows the log last listed.
    ShowGitLog,
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
