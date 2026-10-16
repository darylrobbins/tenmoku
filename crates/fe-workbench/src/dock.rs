//! The right dock: one panel at a time, the project tree, project search,
//! the outline, the git changes, find references' results or the problems,
//! under a title row naming it, and picked from the activity rail on the
//! window's right edge, which stays when the dock hides. Switching keeps each
//! panel's state. The References and Problems buttons join the rail once
//! their panel has shown.
//!
//! Its commands, and the search panel's, are those of [`init`]'s registry:
//! the dock's in the workbench's [`crate::KEY_CONTEXT`] (with
//! `search.show_replace` and `search.replace_all`), the search options' and
//! the replace toggle's in the search panel's [`search_panel::KEY_CONTEXT`],
//! the results list's in [`search_panel::RESULTS_KEY_CONTEXT`],
//! `text_input.submit` in its input's, and the problems commands
//! ([`problems_panel::commands`]). Their handlers only name an op: the
//! workbench, or the search panel, then does it with the window at hand.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_ui::{Icon, TextInput};
use gpui::{App, Context, Entity, EventEmitter, Global, IntoElement, Render, SharedString, Window};
use kiln_shell::{DockItem, DockPosition, DockState, DockView, PanelId};

use crate::{
    GitPanel, OutlinePanel, PANEL_WIDTH, ProjectPanel, ReferencesPanel,
    search_panel::{self, SearchPanel},
};

#[path = "problems_panel.rs"]
pub(crate) mod problems_panel;

pub(crate) use problems_panel::{ProblemsOp, ProblemsPanel};

/// A panel the dock shows: fastedit's name for the [`PanelId`]s it
/// registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPanel {
    Project,
    Search,
    Outline,
    Changes,
    References,
    Problems,
}

impl DockPanel {
    /// The panels the rail always has, in its order; References and
    /// Problems follow once they have shown.
    pub const ALL: [DockPanel; 4] = [
        DockPanel::Project,
        DockPanel::Search,
        DockPanel::Outline,
        DockPanel::Changes,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DockPanel::Project => "Project",
            DockPanel::Search => "Search",
            DockPanel::Outline => "Outline",
            DockPanel::Changes => "Changes",
            DockPanel::References => "References",
            DockPanel::Problems => "Problems",
        }
    }

    /// Its id in the dock.
    pub fn id(self) -> PanelId {
        PanelId(match self {
            DockPanel::Project => "project",
            DockPanel::Search => "search",
            DockPanel::Outline => "outline",
            DockPanel::Changes => "changes",
            DockPanel::References => "references",
            DockPanel::Problems => "problems",
        })
    }

    /// The panel registered as `id`.
    fn of(id: PanelId) -> Option<Self> {
        [DockPanel::References, DockPanel::Problems]
            .into_iter()
            .chain(DockPanel::ALL)
            .find(|panel| panel.id() == id)
    }

    /// The command that shows it.
    fn show_command(self) -> &'static str {
        match self {
            DockPanel::Project => "dock.show_project",
            DockPanel::Search => "search.show",
            DockPanel::Outline => "outline.show",
            DockPanel::Changes => "git.show_changes",
            DockPanel::References => "nav.find_references",
            DockPanel::Problems => problems_panel::SHOW,
        }
    }

    /// The op of [`DockPanel::show_command`].
    fn show_op(self) -> DockOp {
        match self {
            DockPanel::Project => DockOp::ShowProject,
            DockPanel::Search => DockOp::ShowSearch,
            DockPanel::Outline => DockOp::ShowOutline,
            DockPanel::Changes => DockOp::ShowChanges,
            DockPanel::References => DockOp::ShowReferences,
            DockPanel::Problems => DockOp::Problems(ProblemsOp::Show),
        }
    }
}

/// Its rail button's icon.
pub(crate) fn panel_icon(panel: DockPanel) -> Icon {
    match panel {
        DockPanel::Project => Icon::Files,
        DockPanel::Search => Icon::Search,
        DockPanel::Outline => Icon::ListTree,
        DockPanel::Changes => Icon::GitBranch,
        DockPanel::References => Icon::FileSymlink,
        // The bundle has no warning sign: an error's cross.
        DockPanel::Problems => Icon::X,
    }
}

/// fastedit's panels in kiln-shell's dock: each names itself, its icon and
/// its show command.
macro_rules! dock_item {
    ($panel:ty, $which:expr, $on_rail:expr, $focus:expr) => {
        impl DockItem for $panel {
            fn id(&self) -> PanelId {
                $which.id()
            }

            fn title(&self, _: &App) -> SharedString {
                $which.name().into()
            }

            fn icon(&self, _: &App) -> Icon {
                panel_icon($which)
            }

            fn show_command(&self) -> Option<&'static str> {
                Some($which.show_command())
            }

            fn always_on_rail(&self) -> bool {
                $on_rail
            }

            fn focus_on_show(&self) -> bool {
                $focus
            }
        }
    };
}

dock_item!(ProjectPanel, DockPanel::Project, true, false);
dock_item!(SearchPanel, DockPanel::Search, true, true);
dock_item!(OutlinePanel, DockPanel::Outline, true, false);
// Focused when shown, as its rows take Enter and the arrows.
dock_item!(GitPanel, DockPanel::Changes, true, true);
// On the rail once it has shown.
dock_item!(ReferencesPanel, DockPanel::References, false, false);
// On the rail once it has shown; focused when shown, for its commands.
dock_item!(ProblemsPanel, DockPanel::Problems, false, true);

/// A rail button was clicked; the workbench runs [`Dock::rail_op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockEvent {
    Rail(DockPanel),
}

/// The right dock: kiln-shell's [`DockState`] over fastedit's six panels,
/// whose rail clicks run the op the panel's key would.
pub struct Dock {
    state: DockState,
    project: Entity<ProjectPanel>,
    search: Entity<SearchPanel>,
    outline: Entity<OutlinePanel>,
    changes: Entity<GitPanel>,
    references: Entity<ReferencesPanel>,
    problems: Entity<ProblemsPanel>,
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
    /// input. Search options and submits, which the search panel does when
    /// focused, go on. Focus on a panel the op hides stays: the workbench
    /// moves it.
    pub(crate) fn run(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) -> bool {
        #[cfg(test)]
        self.ops.push(op);
        let panel = match op {
            DockOp::ShowSearch => DockPanel::Search,
            DockOp::ShowProject => DockPanel::Project,
            DockOp::ShowOutline => DockPanel::Outline,
            DockOp::ShowReferences => DockPanel::References,
            DockOp::ShowChanges => DockPanel::Changes,
            DockOp::Problems(ProblemsOp::Show) => DockPanel::Problems,
            // The workbench's to do.
            DockOp::Problems(_) => return false,
            DockOp::RefreshChanges => {
                self.changes.update(cx, GitPanel::refresh);
                return true;
            }
            DockOp::Toggle => {
                self.state.toggle();
                cx.notify();
                return true;
            }
            DockOp::ShowReplace => {
                self.show(DockPanel::Search, window, cx);
                self.search
                    .update(cx, |search, cx| search.show_replace(true, window, cx));
                return true;
            }
            DockOp::ReplaceAll => {
                self.search
                    .update(cx, |search, cx| search.confirm_replace_all(window, cx));
                return true;
            }
            DockOp::ToggleRegex
            | DockOp::ToggleCase
            | DockOp::ToggleWholeWord
            | DockOp::Submit
            | DockOp::ToggleReplace
            | DockOp::FocusResults
            | DockOp::SelectNext
            | DockOp::SelectPrevious
            | DockOp::OpenSelected => {
                return false;
            }
        };
        self.show(panel, window, cx);
        true
    }

    /// Shows the dock with `panel`; search focuses its input.
    pub fn show(&mut self, panel: DockPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.state.show(panel.id(), window, cx);
        cx.notify();
    }

    /// Puts back the state [`DockState::save`] saved: whether it shows, its
    /// active panel and whether References has joined the rail.
    pub(crate) fn restore(&mut self, saved: &fe_command::Value, cx: &mut Context<Self>) {
        self.state.restore(saved);
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

/// What a dock or search command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockOp {
    ShowSearch,
    ShowProject,
    /// `outline.show`, which the outline panel's registry has.
    ShowOutline,
    /// The References button: shows the last results, finding none.
    ShowReferences,
    /// `git.show_changes`.
    ShowChanges,
    /// `git.refresh`: reads the git status again, shown or not.
    RefreshChanges,
    Toggle,
    ToggleRegex,
    ToggleCase,
    ToggleWholeWord,
    /// Enter in the search panel's input: [`TextInput::submit`].
    Submit,
    /// `search.show_replace`: shows search with its replace field, focused.
    ShowReplace,
    /// `search.replace_all`: asks, then replaces in the results' files.
    ReplaceAll,
    ToggleReplace,
    FocusResults,
    SelectNext,
    SelectPrevious,
    OpenSelected,
    /// A problems command: `problems.show` is the dock's to do, the others
    /// the workbench's.
    Problems(ProblemsOp),
}

/// The [`CommandTarget`] of dock commands: the op to do.
pub(crate) struct DockCommands;

impl CommandTarget for DockCommands {
    type Tx = ();
    type Target<'a> = Option<DockOp>;
}

struct Commands(Rc<Registry<DockCommands>>);

impl Global for Commands {}

/// Registers the dock and search commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<DockCommands> {
    use DockOp::*;
    let workbench = crate::KEY_CONTEXT;
    let search = search_panel::KEY_CONTEXT;
    let results = search_panel::RESULTS_KEY_CONTEXT;
    let commands = [
        ("search.show", "Find in Files", workbench, ShowSearch),
        ("dock.show_project", "Show Project", workbench, ShowProject),
        ("dock.toggle", "Toggle Dock", workbench, Toggle),
        ("git.show_changes", "Show Changes", workbench, ShowChanges),
        ("git.refresh", "Refresh Changes", workbench, RefreshChanges),
        (
            "search.show_replace",
            "Replace in Files…",
            workbench,
            ShowReplace,
        ),
        (
            "search.replace_all",
            "Replace All in Files",
            workbench,
            ReplaceAll,
        ),
        ("search.toggle_regex", "Toggle Regex", search, ToggleRegex),
        (
            "search.toggle_case",
            "Toggle Match Case",
            search,
            ToggleCase,
        ),
        (
            "search.toggle_whole_word",
            "Toggle Whole Word",
            search,
            ToggleWholeWord,
        ),
        (
            "search.toggle_replace",
            "Toggle Replace",
            search,
            ToggleReplace,
        ),
        (
            "search.focus_results",
            "Focus Search Results",
            search,
            FocusResults,
        ),
        (
            "search.select_next",
            "Select Next Result",
            results,
            SelectNext,
        ),
        (
            "search.select_previous",
            "Select Previous Result",
            results,
            SelectPrevious,
        ),
        (
            "search.open_selected",
            "Open Selected Result",
            results,
            OpenSelected,
        ),
        (
            "text_input.submit",
            "Submit",
            TextInput::KEY_CONTEXT,
            Submit,
        ),
    ];
    let problems = (problems_panel::commands().into_iter())
        .map(|(id, title, context, op)| (id, title, context, Problems(op)));
    let mut registry = Registry::new();
    for (id, title, context, op) in commands.into_iter().chain(problems) {
        registry
            .register(id, title, move |target: &mut Option<DockOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("dock command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The op of `run`, if it's a dock or search command that applies in its
/// context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<DockOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
