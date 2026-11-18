//! The sidebar: one panel at a time, the Explorer, project search,
//! Source Control, find references' results, a file's git
//! history or the AI chat, under a title row naming it, and picked from the
//! activity rail on the window's left edge, or its right
//! (`workbench.rail`, [`rail_position`]), which stays when the dock hides.
//! Switching keeps each panel's state. The rail's top group is Project
//! (the Explorer), Search, Changes (Source Control), Agents (the lanes,
//! [`agents_panel`]) and Chat, then the
//! panels shown on first use; its foot is the bottom panel's tool windows,
//! Problems, Runs, the Git log and Terminal ([`rail_actions`]), and Debug after
//! Runs once a session has started ([`Dock::join_debug`]), which the bottom
//! dock shows under its own tab strip ([`BOTTOM_PANEL_HEIGHT`]): a click
//! runs the tool window's command, or hides the bottom dock if it shows
//! that one ([`DockEvent::Action`]). Changes' button counts the changed
//! files ([`Dock::set_changed_files`]), Agents' the lanes working or to
//! review ([`Dock::follow_agents`]), Problems' has a dot while there
//! are errors ([`Dock::set_errors`]). The Explorer is a
//! [`ViewContainer`] of two views, the project tree (Folders), its lead,
//! which the dock's header names ("Project") and carries the actions of,
//! over Structure, the outline (collapsed at the foot until shown or
//! expanded, its header then showing the symbol under the caret), whose
//! collapse and weights the dock's saved state keeps; `dock.show_project`
//! shows it, and `outline.show` shows it with Structure expanded and
//! focused. The header's ⋯ menu moves the sidebar to the other side
//! ([`dock_menu`]); its — runs `dock.toggle`. Source
//! Control is another, of the git changes alone ([`source_control`]):
//! `git.show_changes` shows it. The References,
//! History and Chat buttons join the rail once their panel has shown; the
//! History panel is made the first time `git.file_history` runs
//! ([`Dock::set_history`]), the others with the dock; the Call Hierarchy panel
//! is made by `nav.show_call_hierarchy` ([`Dock::set_hierarchy`]), and the
//! Quickfix List panel by `:copen` and its commands ([`Dock::set_quickfix`],
//! [`quickfix_panel`]). With vim on,
//! `<C-w>h`/`l`/`j` past the panes' edge focus what the dock shows
//! ([`Dock::focus_active`], `dock_focus.rs`).
//!
//! Its commands (`dock.show_project`, `dock.toggle`, the panels' show
//! commands, `search.show_replace`, `search.replace_all` and `git.refresh`)
//! are the workbench's typed commands (`workbench_commands.rs`), which do a
//! [`DockOp`], as a rail click does; the search panel's own are in its
//! registry ([`search_panel::registry`]).

use std::collections::HashMap;

use fe_config::settings::RailSide;
use fe_ui::{BadgeTone, Icon};
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    Styled as _, Subscription, Window, div, px,
};
use kiln_l10n::tr;
use kiln_platform::locale::ActiveLocale;
use kiln_shell::{
    DockItem, DockMenuItem, DockPosition, DockState, DockView, PanelId, RailAction, ViewContainer,
    ViewSpec,
    dock::{DockDrop, DockedPanel, RailBadge},
};

use crate::{
    ChatPanel, GitLogPanel, GitPanel, HierarchyPanel, HistoryPanel, OutlinePanel, PANEL_WIDTH,
    ProjectPanel, ReferencesPanel,
    agents_panel::{self, AgentsPanel},
    ai_changes_panel::AiChangesPanel,
    debug_panel::{self, DebugPanel},
    outline_panel::CaretSymbol,
    runs_panel::{self, RunsPanel},
    search_panel::{self, SearchPanel},
};

#[path = "problems_panel.rs"]
pub(crate) mod problems_panel;

pub(crate) use problems_panel::{ProblemsOp, ProblemsPanel};

#[path = "source_control.rs"]
pub(crate) mod source_control;

#[path = "quickfix_panel.rs"]
pub(crate) mod quickfix_panel;

pub(crate) use quickfix_panel::QuickfixPanel;

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
    pub const Agents: DockPanel = agents_panel::SIDEBAR.panel;
    pub const References: DockPanel = crate::references_panel::SIDEBAR.panel;
    /// The bottom panel's Problems, no longer the sidebar's: its id, which
    /// its rail button and badge go by.
    pub const Problems: DockPanel = problems_panel::SIDEBAR.panel;
    pub const AiChanges: DockPanel = crate::ai_changes_panel::SIDEBAR.panel;
    pub const History: DockPanel = crate::workbench::git_commands::git_history::SIDEBAR.panel;
    pub const Chat: DockPanel = crate::chat_panel::SIDEBAR.panel;
    pub const CallHierarchy: DockPanel = crate::hierarchy_panel::SIDEBAR.panel;
    pub const Quickfix: DockPanel = quickfix_panel::SIDEBAR.panel;
}

impl DockPanel {
    /// The panels the rail always has, in its order; References, History
    /// and the others join the top group once they have shown.
    pub const ALL: [DockPanel; 5] = always_on_rail();

    /// The panel registered as `id`, for a module's [`SidebarPanel`], or a
    /// bottom tool window's id ([`DockPanel::Problems`]).
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
    pub(crate) fn of(id: PanelId) -> Option<Self> {
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
    &agents_panel::SIDEBAR,
    &crate::chat_panel::SIDEBAR,
    &crate::references_panel::SIDEBAR,
    &crate::workbench::git_commands::git_history::SIDEBAR,
    &crate::ai_changes_panel::SIDEBAR,
    &crate::hierarchy_panel::SIDEBAR,
    &quickfix_panel::SIDEBAR,
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
    // History ([`Dock::set_history`]), then AI Changes
    // ([`Dock::set_ai_changes`]), Call Hierarchy ([`Dock::set_hierarchy`])
    // and Quickfix List ([`Dock::set_quickfix`]), the panels registered
    // after the dock is made, are added at the rail's end, in that order:
    // their places.
    assert!(
        PANELS[PANELS.len() - 4].order
            == crate::workbench::git_commands::git_history::SIDEBAR.order,
        "History, registered fourth from last, is fourth from last on the rail"
    );
    assert!(
        PANELS[PANELS.len() - 3].order == crate::ai_changes_panel::SIDEBAR.order,
        "AI Changes, registered third from last, is third from last on the rail"
    );
    assert!(
        PANELS[PANELS.len() - 2].order == crate::hierarchy_panel::SIDEBAR.order,
        "Call Hierarchy, registered last but one, is last but one on the rail"
    );
    assert!(
        PANELS[PANELS.len() - 1].order == quickfix_panel::SIDEBAR.order,
        "Quickfix List, registered last, is last on the rail"
    );
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

/// The bottom panel's height as built, which a double-click on its edge
/// puts back (`docs/design/design-system.md`, Measure).
pub(crate) const BOTTOM_PANEL_HEIGHT: Pixels = px(250.);

/// The sidebar's rail action that shows the terminal's dock, whose button
/// follows it.
pub(crate) const TERMINAL_BUTTON: &str = "terminal";

/// The Problems tool window's id in the bottom dock, and its rail button's.
pub(crate) const PROBLEMS_BUTTON: &str = DockPanel::Problems.0.0;

/// The Runs tool window's id in the bottom dock, and its rail button's.
pub(crate) const RUNS_BUTTON: &str = runs_panel::PANEL;

/// The Debug tool window's id in the bottom dock, and its rail button's.
pub(crate) const DEBUG_BUTTON: &str = debug_panel::PANEL;

/// The Debug tool window's rail action, which joins the rail's foot after
/// Runs' the first time a session starts ([`Dock::join_debug`]).
pub(crate) fn debug_rail_action() -> RailAction {
    RailAction {
        id: DEBUG_BUTTON,
        icon: debug_panel::ICON,
        title: tr!("workbench-debug-panel-title"),
        command: debug_panel::SHOW_COMMAND,
    }
}

/// The Git log tool window's id in the bottom dock, and its rail button's:
/// the id it had on the sidebar before it was Source Control's Graph, which
/// layouts saved then name.
pub(crate) const GIT_LOG_BUTTON: &str = crate::workbench::git_commands::git_log::OLD_PANEL;

/// The bottom panel's tool windows, in the order the design names them,
/// which their foot buttons follow: the panels placed in the bottom panel
/// unless moved ([`Place`]).
pub(crate) const BOTTOM_TOOLS: [&str; 5] = [
    PROBLEMS_BUTTON,
    RUNS_BUTTON,
    DEBUG_BUTTON,
    GIT_LOG_BUTTON,
    TERMINAL_BUTTON,
];

/// The bottom panel's tool windows in the order it registers them, its
/// tabs': where one moved back goes among them.
pub(crate) const BOTTOM_TABS: [&str; 5] = [
    PROBLEMS_BUTTON,
    RUNS_BUTTON,
    DEBUG_BUTTON,
    TERMINAL_BUTTON,
    GIT_LOG_BUTTON,
];

/// The transfer group of the sidebar and the bottom panel, which take
/// each other's tool windows by drag ([`kiln_shell::dock::DockedPanel`]).
pub(crate) const TOOL_WINDOWS: &str = "tool-windows";

/// Which dock holds a tool window: the sidebar or the bottom panel. Each
/// has its place by default, the bottom panel's tool windows
/// ([`BOTTOM_TOOLS`]) the bottom panel and the sidebar's panels the
/// sidebar, until it's moved ([`Dock::set_place`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Place {
    Sidebar,
    Bottom,
}

impl Place {
    /// Its name in the saved layout's `"placements"`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Place::Sidebar => "sidebar",
            Place::Bottom => "bottom",
        }
    }

    /// The place [`Place::name`] names `name`.
    pub(crate) fn named(name: &str) -> Option<Self> {
        match name {
            "sidebar" => Some(Place::Sidebar),
            "bottom" => Some(Place::Bottom),
            _ => None,
        }
    }

    /// Where `panel` is by default.
    pub(crate) fn default_of(panel: PanelId) -> Self {
        match BOTTOM_TOOLS.contains(&panel.0) {
            true => Place::Bottom,
            false => Place::Sidebar,
        }
    }
}

/// The tool window of the id `id`, if there's one that moves: every bottom
/// tool window and every sidebar panel but the Explorer, whose reveal and
/// focus (the tree and Structure) are the sidebar's.
pub(crate) fn movable(id: &str) -> Option<PanelId> {
    let bottom = BOTTOM_TOOLS
        .iter()
        .find(|tool| **tool == id)
        .map(|id| PanelId(id));
    let sidebar = (PANELS.iter())
        .map(|sidebar| sidebar.panel.id())
        .find(|panel| panel.0 == id && *panel != DockPanel::Explorer.id());
    bottom.or(sidebar)
}

/// The sidebar panel a dock op shows, if it shows one.
pub(crate) fn shown_by(op: DockOp) -> Option<PanelId> {
    let shown = match op {
        DockOp::ShowOutline => DockPanel::Explorer,
        DockOp::ShowReplace => DockPanel::Search,
        _ => PANELS.iter().find(|sidebar| sidebar.show_op == op)?.panel,
    };
    Some(shown.id())
}

/// The rail action of the bottom tool window `id`, which shows it.
pub(crate) fn tool_action(id: &str) -> Option<RailAction> {
    let mut actions = rail_actions().into_iter().chain([debug_rail_action()]);
    actions.find(|action| action.id == id)
}

/// The rail's actions, at its foot: the bottom panel's tool windows in
/// the order the design names them, each showing its tool window in the
/// bottom dock, its button highlighted while that shows.
pub(crate) fn rail_actions() -> [RailAction; 4] {
    [
        RailAction {
            id: PROBLEMS_BUTTON,
            icon: problems_panel::SIDEBAR.icon,
            title: tr!("workbench-problems-title"),
            command: problems_panel::SIDEBAR.show_command,
        },
        RailAction {
            id: RUNS_BUTTON,
            icon: runs_panel::ICON,
            title: tr!("workbench-runs-title"),
            command: runs_panel::SHOW_COMMAND,
        },
        RailAction {
            id: GIT_LOG_BUTTON,
            icon: Icon::GitGraph,
            title: tr!("workbench-dock-git-log"),
            command: "git.log",
        },
        RailAction {
            id: TERMINAL_BUTTON,
            icon: Icon::FileTerminal,
            title: tr!("workbench-dock-terminal"),
            command: "terminal.toggle",
        },
    ]
}

/// The sidebar header's ⋯ menu at `position`: move it to the other side,
/// titled as its command is. Grow Dock and Shrink Dock aren't on it: they
/// resize whichever dock has focus, the terminal's maybe, not the sidebar.
pub(crate) fn dock_menu(position: DockPosition) -> Vec<DockMenuItem> {
    let command = match position {
        DockPosition::Left => "dock.move_sidebar_right",
        _ => "dock.move_sidebar_left",
    };
    vec![DockMenuItem {
        title: fe_command::l10n::command_label(command, ""),
        command,
    }]
}

/// The command the sidebar header's — runs: the dock's toggle, which
/// moves focus out of what it hides.
const HIDE_COMMAND: &str = "dock.toggle";

/// The edge `workbench.rail` puts the rail and the sidebar on.
pub(crate) fn rail_position(side: RailSide) -> DockPosition {
    match side {
        RailSide::Left => DockPosition::Left,
        RailSide::Right => DockPosition::Right,
    }
}

/// fastedit's panels in kiln-shell's dock: each is what its module's
/// [`SidebarPanel`] declares. Its title row has what `$title` gives: a
/// closure or method of the panel's, from the catalogue. `actions = …` and
/// `off_rail` follow a `$title` too, so no panel is titled by its name.
macro_rules! dock_item {
    ($panel:ty, $sidebar:expr, $title:expr) => {
        dock_item!($panel, $sidebar, $title, true);
    };
    ($panel:ty, $sidebar:expr, $title:expr, $joins_rail:expr) => {
        dock_item!(
            $panel,
            $sidebar,
            $title,
            $joins_rail,
            |_: &$panel, _: &App| Vec::new()
        );
    };
    ($panel:ty, $sidebar:expr, $title:expr, $joins_rail:expr, $actions:expr) => {
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

            fn joins_rail_on_show(&self) -> bool {
                $joins_rail
            }

            fn focus_on_show(&self) -> bool {
                $sidebar.focus_on_show
            }

            fn actions(&self, cx: &App) -> Vec<kiln_shell::PaneAction> {
                $actions(self, cx)
            }
        }
    };
}

// Its header carries the results' actions.
dock_item!(
    SearchPanel,
    search_panel::SIDEBAR,
    |_: &SearchPanel, _: &App| tr!("workbench-dock-search-title"),
    true,
    |panel: &SearchPanel, _: &App| panel.pane_actions()
);
dock_item!(
    ReferencesPanel,
    crate::references_panel::SIDEBAR,
    |_: &ReferencesPanel, _: &App| tr!("workbench-dock-references-title")
);
/// The bottom panel's Problems tool window, titled from the catalogue.
impl DockItem for ProblemsPanel {
    fn id(&self) -> PanelId {
        DockPanel::Problems.id()
    }

    fn title(&self, _: &App) -> SharedString {
        tr!("workbench-problems-title")
    }

    fn icon(&self, _: &App) -> Icon {
        problems_panel::SIDEBAR.icon
    }

    fn show_command(&self) -> Option<&'static str> {
        Some(problems_panel::SIDEBAR.show_command)
    }

    fn focus_on_show(&self) -> bool {
        true
    }
}

/// The bottom panel's Runs tool window, titled from the catalogue.
impl DockItem for RunsPanel {
    fn id(&self) -> PanelId {
        PanelId(RUNS_BUTTON)
    }

    fn title(&self, _: &App) -> SharedString {
        tr!("workbench-runs-title")
    }

    fn icon(&self, _: &App) -> Icon {
        runs_panel::ICON
    }

    fn show_command(&self) -> Option<&'static str> {
        Some(runs_panel::SHOW_COMMAND)
    }

    fn focus_on_show(&self) -> bool {
        true
    }
}

/// The bottom panel's Debug tool window, titled from the catalogue. A
/// session shows it without taking focus; `debug.show_panels` focuses it.
impl DockItem for DebugPanel {
    fn id(&self) -> PanelId {
        PanelId(DEBUG_BUTTON)
    }

    fn title(&self, _: &App) -> SharedString {
        tr!("workbench-debug-panel-title")
    }

    fn icon(&self, _: &App) -> Icon {
        debug_panel::ICON
    }

    fn show_command(&self) -> Option<&'static str> {
        Some(debug_panel::SHOW_COMMAND)
    }

    fn focus_on_show(&self) -> bool {
        false
    }
}

/// The bottom panel's Git log tool window, its tab titled with what it
/// lists ([`GitLogPanel::title`]), as Graph's header was.
impl DockItem for GitLogPanel {
    fn id(&self) -> PanelId {
        PanelId(GIT_LOG_BUTTON)
    }

    fn title(&self, cx: &App) -> SharedString {
        GitLogPanel::title(self, cx)
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::GitGraph
    }

    fn show_command(&self) -> Option<&'static str> {
        Some("git.log")
    }

    fn focus_on_show(&self) -> bool {
        true
    }
}
dock_item!(
    AiChangesPanel,
    crate::ai_changes_panel::SIDEBAR,
    |_: &AiChangesPanel, _: &App| tr!("workbench-dock-ai-changes-title"),
    false
);
// Titled with the file it lists.
dock_item!(
    HistoryPanel,
    crate::workbench::git_commands::git_history::SIDEBAR,
    HistoryPanel::title
);
dock_item!(
    AgentsPanel,
    agents_panel::SIDEBAR,
    |_: &AgentsPanel, _: &App| tr!("workbench-dock-agents-title")
);
dock_item!(
    ChatPanel,
    crate::chat_panel::SIDEBAR,
    |_: &ChatPanel, _: &App| tr!("workbench-dock-chat-title")
);
// Titled with the symbol at its root.
dock_item!(
    HierarchyPanel,
    crate::hierarchy_panel::SIDEBAR,
    HierarchyPanel::title
);
// Titled with the list it shows.
dock_item!(QuickfixPanel, quickfix_panel::SIDEBAR, QuickfixPanel::title);

/// A rail button was clicked: a panel's, for which the workbench runs
/// [`Dock::rail_op`], or a bottom tool window's action, or a bottom tool
/// window's button moved into the sidebar, by its id and command, which
/// the workbench runs or, if the dock holding that tool window shows it,
/// hides it. Or a tool window was dropped on the sidebar or its rail's
/// foot, from the dock view of that id ([`DockView::panel_dropped`]), for
/// the workbench to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockEvent {
    Rail(DockPanel),
    Action(&'static str, &'static str),
    Dropped(PanelId, EntityId, DockDrop),
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
    /// What Structure's header shows while it's collapsed, which the
    /// Explorer draws: the tests' to read.
    #[cfg_attr(not(test), allow(dead_code))]
    caret_symbol: Entity<CaretSymbol>,
    /// The side the header's ⋯ menu was last built for.
    menu_side: DockPosition,
    /// The git changes, its view.
    source_control: Entity<ViewContainer>,
    changes: Entity<GitPanel>,
    /// The window's lanes.
    agents: Entity<AgentsPanel>,
    references: Entity<ReferencesPanel>,
    /// The bottom dock's, kept here for the workbench's problems code.
    problems: Entity<ProblemsPanel>,
    /// Made the first time it's asked for.
    history: Option<Entity<HistoryPanel>>,
    /// Made the first time it's asked for.
    hierarchy: Option<Entity<HierarchyPanel>>,
    /// Made the first time it's asked for.
    quickfix: Option<Entity<QuickfixPanel>>,
    /// The bottom dock's, kept here for the workbench's git code.
    git_log: Option<Entity<GitLogPanel>>,
    /// Made the first time it's asked for.
    chat: Option<Entity<ChatPanel>>,
    /// Made the first time a changeset is shown.
    ai_changes: Option<Entity<AiChangesPanel>>,
    /// What Changes' and Problems' badges count: the changed files and the
    /// errors.
    changed_files: usize,
    errors: usize,
    /// The Debug tool window's action has joined the rail, and whether a
    /// session is stopped, which its dot says.
    debug_joined: bool,
    debug_stopped: bool,
    /// The tool windows moved from their default dock ([`Place`]).
    placements: HashMap<PanelId, Place>,
    /// The bottom panel's saved groups, kept while a tool window made on
    /// first use, saved placed there, hasn't been: it joins them when it is
    /// ([`Dock::pending_groups`]).
    pending_groups: Option<PendingGroups>,
    /// The rail foot's actions, as last put there ([`Dock::sync_foot`]),
    /// and the bottom panel's tool windows shown, which light theirs.
    foot: Vec<&'static str>,
    bottom_shown: Vec<PanelId>,
    #[cfg(test)]
    ops: Vec<DockOp>,
    /// The explorer's views' collapse and weights are the dock's state,
    /// for the workbench to save.
    _explorer_changed: Subscription,
    /// Source Control's are too.
    _source_control_changed: Subscription,
    /// Agents' badge follows its lanes.
    _agents_changed: Subscription,
    /// Its text in a new language ([`Dock::follow_locale`]).
    _locale: Subscription,
}

/// The bottom panel's groups as a restore saved them, and as that left
/// them: a tool window made after, placed there, rejoins its saved group
/// only while the groups are still as the restore left them.
#[derive(Clone, Debug)]
pub(crate) struct PendingGroups {
    pub(crate) saved: fe_command::Value,
    pub(crate) left: Option<fe_command::Value>,
}

/// The tool windows made on first use.
pub(crate) const LAZY_TOOLS: [DockPanel; 4] = [
    DockPanel::History,
    DockPanel::CallHierarchy,
    DockPanel::Quickfix,
    DockPanel::AiChanges,
];

/// The Explorer: `project`'s tree, its lead, over `outline`, collapsed at
/// first, its header showing `caret_symbol`.
fn explorer(
    project: &Entity<ProjectPanel>,
    outline: &Entity<OutlinePanel>,
    caret_symbol: &Entity<CaretSymbol>,
    cx: &mut Context<Dock>,
) -> Entity<ViewContainer> {
    let sidebar = crate::project_panel::SIDEBAR;
    cx.new(|cx| {
        let title = tr!("workbench-project-title");
        let mut explorer = ViewContainer::new(sidebar.panel.id(), title, sidebar.icon, cx)
            .show_command(sidebar.show_command)
            .lead(crate::project_panel::FOLDERS);
        explorer.add_view(folders(project), cx);
        let actions = crate::project_panel::pane_actions(project);
        explorer.set_actions(crate::project_panel::FOLDERS, actions, cx);
        let view = crate::outline_panel::VIEW;
        explorer.set_detail(view, caret_symbol.clone().into(), cx);
        let outline = outline_view(outline, cx);
        explorer.add_view(outline, cx);
        explorer
    })
}

/// The Explorer's Structure view: `outline`, collapsed at first.
fn outline_view(outline: &Entity<OutlinePanel>, cx: &mut App) -> ViewSpec {
    ViewSpec {
        id: crate::outline_panel::VIEW,
        title: tr!("workbench-dock-structure"),
        order: 20,
        default_collapsed: true,
        view: cx.new(|cx| Tracked::new(outline, cx)).into(),
    }
}

/// A view drawn in an element tracking its focus, for one that draws
/// without, as the outline does: [`Dock::focus_active`] focusing it then
/// puts focus in the sidebar, under the workbench's keys.
struct Tracked {
    focus: FocusHandle,
    view: AnyView,
}

impl Tracked {
    fn new<V: Render + Focusable>(view: &Entity<V>, cx: &App) -> Self {
        Self {
            focus: view.focus_handle(cx),
            view: view.clone().into(),
        }
    }
}

impl Render for Tracked {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Fills the view's body, a row, as the view would.
        div()
            .track_focus(&self.focus)
            .flex_1()
            .min_w_0()
            .flex()
            .child(self.view.clone())
    }
}

/// The Explorer's Folders view: `project`'s tree.
fn folders(project: &Entity<ProjectPanel>) -> ViewSpec {
    ViewSpec {
        id: crate::project_panel::FOLDERS,
        title: tr!("workbench-dock-folders"),
        order: 10,
        default_collapsed: false,
        view: project.clone().into(),
    }
}

impl Dock {
    /// Shows the Explorer, of `project` and `outline`, first; Source
    /// Control, of `changes`, follows search on the rail, `agents` it and
    /// `chat` that.
    /// At `position`, the rail's side. `git_log` and `problems` are the
    /// bottom dock's, which the workbench registers them in; the dock keeps
    /// them for the workbench's code that reaches them through it.
    #[allow(clippy::too_many_arguments, private_interfaces)]
    pub fn new(
        position: DockPosition,
        project: Entity<ProjectPanel>,
        search: Entity<SearchPanel>,
        outline: Entity<OutlinePanel>,
        changes: Entity<GitPanel>,
        agents: Entity<AgentsPanel>,
        git_log: Entity<GitLogPanel>,
        chat: Entity<ChatPanel>,
        references: Entity<ReferencesPanel>,
        problems: Entity<ProblemsPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut state = DockState::new(position)
            .size(PANEL_WIDTH)
            .shortcut_contexts([crate::KEY_CONTEXT, fe_gpui::KEY_CONTEXT])
            .hide_command(HIDE_COMMAND)
            .transfer_group(TOOL_WINDOWS);
        state.set_menu(dock_menu(position));
        let caret_symbol = cx.new(|cx| CaretSymbol::new(&outline, cx));
        let explorer = explorer(&project, &outline, &caret_symbol, cx);
        let explorer_changed = cx.observe(&explorer, |_, _, cx| cx.notify());
        state.add_panel(explorer.clone(), cx);
        state.add_panel(search.clone(), cx);
        let source_control = source_control::container(&changes, cx);
        let source_control_changed = cx.observe(&source_control, |_, _, cx| cx.notify());
        state.add_panel(source_control.clone(), cx);
        state.add_panel(agents.clone(), cx);
        let agents_changed = cx.observe(&agents, |dock, _, cx| dock.follow_agents(cx));
        // Registered here, not after the References panel, as the rail
        // lists the panels in the order they were added.
        state.add_panel(chat.clone(), cx);
        state.add_panel(references.clone(), cx);
        let mut dock = Self {
            state,
            explorer,
            project,
            search,
            outline,
            caret_symbol,
            menu_side: position,
            source_control,
            changes,
            agents,
            references,
            problems,
            history: None,
            hierarchy: None,
            quickfix: None,
            git_log: Some(git_log),
            chat: Some(chat),
            ai_changes: None,
            changed_files: 0,
            errors: 0,
            debug_joined: false,
            debug_stopped: false,
            placements: HashMap::new(),
            pending_groups: None,
            foot: Vec::new(),
            bottom_shown: Vec::new(),
            #[cfg(test)]
            ops: Vec::new(),
            _explorer_changed: explorer_changed,
            _source_control_changed: source_control_changed,
            _agents_changed: agents_changed,
            _locale: cx.observe_global::<ActiveLocale>(Self::follow_locale),
        };
        // The bottom panel's tool windows, at the rail's foot.
        dock.sync_foot(cx);
        dock
    }

    /// Puts its text in the language just chosen: the rail actions'
    /// titles, the Explorer's views' and the badges' names. The views keep
    /// their collapse, as [`Dock::set_project`]'s do.
    fn follow_locale(&mut self, cx: &mut Context<Self>) {
        self.sync_foot(cx);
        if self.debug_joined {
            self.show_debug_badge();
        }
        self.state.set_menu(dock_menu(self.menu_side));
        let outline = outline_view(&self.outline, cx);
        let folders = folders(&self.project);
        let actions = crate::project_panel::pane_actions(&self.project);
        (self.explorer).update(cx, |explorer, cx| {
            explorer.set_title(tr!("workbench-project-title"), cx);
            explorer.add_view(folders, cx);
            explorer.set_actions(crate::project_panel::FOLDERS, actions, cx);
            explorer.add_view(outline, cx);
        });
        source_control::follow_locale(&self.source_control, &self.changes, cx);
        let (changed_files, errors) = (self.changed_files, self.errors);
        self.set_changed_files(changed_files, cx);
        self.set_errors(errors, cx);
        self.follow_agents(cx);
        cx.notify();
    }

    pub fn project(&self) -> &Entity<ProjectPanel> {
        &self.project
    }

    /// Shows `project` in the old one's place in the Explorer, for a new
    /// root. The dock's visibility and active panel, and the Folders
    /// view's collapse, stay.
    pub(crate) fn set_project(&mut self, project: Entity<ProjectPanel>, cx: &mut Context<Self>) {
        let actions = crate::project_panel::pane_actions(&project);
        (self.explorer).update(cx, |explorer, cx| {
            explorer.add_view(folders(&project), cx);
            explorer.set_actions(crate::project_panel::FOLDERS, actions, cx);
        });
        self.project = project;
        cx.notify();
    }

    pub fn search(&self) -> &Entity<SearchPanel> {
        &self.search
    }

    pub fn outline(&self) -> &Entity<OutlinePanel> {
        &self.outline
    }

    /// What Structure's header shows while it's collapsed.
    #[cfg(test)]
    pub(crate) fn caret_symbol(&self) -> &Entity<CaretSymbol> {
        &self.caret_symbol
    }

    /// The Explorer, of the project tree and the outline.
    pub fn explorer(&self) -> &Entity<ViewContainer> {
        &self.explorer
    }

    /// Source Control, of the git changes.
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
        // Until the new root's changes are listed.
        self.set_changed_files(0, cx);
        cx.notify();
    }

    /// Badges Changes' rail button with `count`, the files the Changes
    /// view lists, in `status.modified`; none at none.
    pub(crate) fn set_changed_files(&mut self, count: usize, cx: &mut Context<Self>) {
        self.changed_files = count;
        let badge = (count > 0).then(|| {
            let name = tr!("workbench-dock-changed-files", count = count);
            RailBadge::count(count, BadgeTone::Modified, name)
        });
        self.set_badge(DockPanel::SourceControl, badge, cx);
    }

    /// The Agents panel, of the window's lanes: the tests' to read.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn agents(&self) -> &Entity<AgentsPanel> {
        &self.agents
    }

    /// Badges Agents' rail button with the lanes Working or with hunks
    /// pending, in silver-blue ([`AgentsPanel::rail_badge`]); none at
    /// none. Run as the panel redraws, its lanes having changed.
    pub(crate) fn follow_agents(&mut self, cx: &mut Context<Self>) {
        let badge = self.agents.read(cx).rail_badge(cx);
        self.set_badge(DockPanel::Agents, badge, cx);
    }

    /// Badges Problems' rail button with a red dot while there are
    /// `errors`.
    pub(crate) fn set_errors(&mut self, errors: usize, cx: &mut Context<Self>) {
        self.errors = errors;
        let badge = (errors > 0).then(|| {
            let name = tr!("workbench-dock-errors", count = errors);
            RailBadge::dot(BadgeTone::Error, name)
        });
        self.set_badge(DockPanel::Problems, badge, cx);
    }

    /// Has the Debug tool window's action join the rail's foot, after
    /// Runs', if it hasn't: on the window's first session.
    pub(crate) fn join_debug(&mut self, cx: &mut Context<Self>) {
        if self.debug_joined {
            return;
        }
        self.debug_joined = true;
        // Placed in the sidebar, it's on the rail's top group already.
        self.sync_foot(cx);
        self.show_debug_badge();
        cx.notify();
    }

    /// Where the tool window `panel` is: its default place unless moved.
    pub(crate) fn place_of(&self, panel: PanelId) -> Place {
        (self.placements.get(&panel).copied()).unwrap_or(Place::default_of(panel))
    }

    /// The tool windows moved from their default dock, and where to, in
    /// id order.
    pub(crate) fn placements(&self) -> Vec<(PanelId, Place)> {
        let mut moved: Vec<_> = self.placements.iter().map(|(id, at)| (*id, *at)).collect();
        moved.sort_by_key(|(id, _)| id.0);
        moved
    }

    /// Keeps the bottom panel's saved `groups`, as `restored` has them, for
    /// the tool windows made on first use that they place there.
    pub(crate) fn keep_pending_groups(&mut self, groups: Option<PendingGroups>) {
        self.pending_groups = groups;
    }

    /// The saved groups kept for tool windows not made yet.
    pub(crate) fn pending_groups(&self) -> Option<&PendingGroups> {
        self.pending_groups.as_ref()
    }

    /// Notes that the tool window `panel` is placed `at`, its foot button
    /// following: the workbench moves it ([`Dock::take`], [`Dock::insert`]).
    pub(crate) fn set_place(&mut self, panel: PanelId, at: Place, cx: &mut Context<Self>) {
        match at == Place::default_of(panel) {
            true => self.placements.remove(&panel),
            false => self.placements.insert(panel, at),
        };
        self.sync_foot(cx);
        cx.notify();
    }

    /// Takes the tool window `panel` out of the sidebar, for the bottom
    /// panel: none if it isn't the sidebar's or doesn't move.
    pub(crate) fn take(&mut self, panel: PanelId, cx: &mut Context<Self>) -> Option<DockedPanel> {
        movable(panel.0)?;
        let taken = self.state.take_panel(panel);
        cx.notify();
        taken
    }

    /// Puts the tool window `panel`, taken from the bottom panel, in the
    /// sidebar: a sidebar panel in its place on the rail, a bottom tool
    /// window after the sidebar's panels, in the design's order.
    pub(crate) fn insert(&mut self, panel: DockedPanel, cx: &mut Context<Self>) {
        let rank = |id: PanelId| {
            let sidebar = PANELS.iter().position(|sidebar| sidebar.panel.id() == id);
            let bottom = BOTTOM_TOOLS.iter().position(|tool| *tool == id.0);
            sidebar.or(bottom.map(|ix| PANELS.len() + ix))
        };
        let own = rank(panel.id());
        let at = (self.state.panel_ids().into_iter())
            .position(|id| rank(id) > own)
            .unwrap_or(usize::MAX);
        self.state.insert_panel(panel, at);
        cx.notify();
    }

    /// The rail's foot: the tool windows placed in the bottom panel, the
    /// bottom ones in the design's order (Debug once joined), then the
    /// sidebar's panels moved there, in the rail's, each a [`RailAction`]
    /// with its panel's icon, title and show command; lit while the bottom
    /// panel shows it.
    fn sync_foot(&mut self, cx: &App) {
        let bottom = (BOTTOM_TOOLS.into_iter())
            .filter(|id| *id != DEBUG_BUTTON || self.debug_joined)
            .filter(|id| self.place_of(PanelId(id)) == Place::Bottom)
            .filter_map(tool_action);
        let moved = (PANELS.iter())
            .filter(|sidebar| self.place_of(sidebar.panel.id()) == Place::Bottom)
            .filter_map(|sidebar| {
                Some(RailAction {
                    id: sidebar.panel.id().0,
                    icon: sidebar.icon,
                    title: self.panel_title(sidebar.panel, cx)?,
                    command: sidebar.show_command,
                })
            });
        let foot: Vec<RailAction> = bottom.chain(moved).collect();
        let ids: Vec<&'static str> = foot.iter().map(|action| action.id).collect();
        // The same buttons keep their places and light, retitled; others
        // are put there anew, lit as the bottom panel shows.
        let changed = ids != self.foot;
        if changed {
            for id in std::mem::take(&mut self.foot) {
                self.state.remove_action(id);
            }
        }
        for action in foot {
            self.state.add_action(action);
        }
        self.foot = ids;
        if changed {
            self.light_foot_buttons();
        }
    }

    /// Lights the foot buttons of the tool windows the bottom panel shows,
    /// `shown`, in any of its groups.
    pub(crate) fn light_foot(&mut self, shown: Vec<PanelId>, cx: &mut Context<Self>) {
        self.bottom_shown = shown;
        self.light_foot_buttons();
        cx.notify();
    }

    fn light_foot_buttons(&mut self) {
        for &id in &self.foot {
            let lit = self.bottom_shown.contains(&PanelId(id));
            self.state.set_action_highlighted(id, lit);
        }
    }

    /// The title of the sidebar's panel `panel`, as its header has it, if
    /// it has been made.
    fn panel_title(&self, panel: DockPanel, cx: &App) -> Option<SharedString> {
        // A panel made on first use, restored in the bottom panel before
        // it has been, has the title it starts with.
        Some(match panel {
            DockPanel::Explorer => DockItem::title(self.explorer.read(cx), cx),
            DockPanel::Search => DockItem::title(self.search.read(cx), cx),
            DockPanel::SourceControl => DockItem::title(self.source_control.read(cx), cx),
            DockPanel::Agents => DockItem::title(self.agents.read(cx), cx),
            DockPanel::References => DockItem::title(self.references.read(cx), cx),
            DockPanel::History => match self.history.as_ref() {
                Some(history) => DockItem::title(history.read(cx), cx),
                None => tr!("workbench-git-log-history-title"),
            },
            DockPanel::Chat => DockItem::title(self.chat.as_ref()?.read(cx), cx),
            DockPanel::CallHierarchy => match self.hierarchy.as_ref() {
                Some(hierarchy) => DockItem::title(hierarchy.read(cx), cx),
                None => tr!("workbench-lsp-hierarchy-calls"),
            },
            DockPanel::Quickfix => match self.quickfix.as_ref() {
                Some(quickfix) => DockItem::title(quickfix.read(cx), cx),
                None => tr!("workbench-lsp-quickfix-quickfix-list"),
            },
            DockPanel::AiChanges => match self.ai_changes.as_ref() {
                Some(panel) => DockItem::title(panel.read(cx), cx),
                None => tr!("workbench-dock-ai-changes-title"),
            },
            _ => return None,
        })
    }

    /// Dots the Debug action while a session is stopped: it needs you.
    pub(crate) fn set_debug_stopped(&mut self, stopped: bool, cx: &mut Context<Self>) {
        if self.debug_stopped != stopped {
            self.debug_stopped = stopped;
            self.show_debug_badge();
            cx.notify();
        }
    }

    fn show_debug_badge(&mut self) {
        let badge = (self.debug_stopped && self.debug_joined)
            .then(|| RailBadge::dot(BadgeTone::Accent, tr!("workbench-debug-panel-rail-stopped")));
        self.state.set_badge(PanelId(DEBUG_BUTTON), badge);
    }

    /// Puts `badge` on `panel`'s rail button, redrawing if it changed.
    fn set_badge(&mut self, panel: DockPanel, badge: Option<RailBadge>, cx: &mut Context<Self>) {
        if self.state.badge(panel.id()) != badge.as_ref() {
            self.state.set_badge(panel.id(), badge);
            cx.notify();
        }
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
        self.sync_foot(cx);
        cx.notify();
    }

    pub(crate) fn hierarchy(&self) -> Option<&Entity<HierarchyPanel>> {
        self.hierarchy.as_ref()
    }

    /// Registers `hierarchy`, the Call Hierarchy panel made on first use,
    /// as [`Dock::set_history`] does the History panel.
    pub(crate) fn set_hierarchy(
        &mut self,
        hierarchy: Entity<HierarchyPanel>,
        cx: &mut Context<Self>,
    ) {
        self.state.add_panel(hierarchy.clone(), cx);
        self.hierarchy = Some(hierarchy);
        self.sync_foot(cx);
        cx.notify();
    }

    pub(crate) fn quickfix(&self) -> Option<&Entity<QuickfixPanel>> {
        self.quickfix.as_ref()
    }

    /// Registers `quickfix`, the Quickfix List panel made on first use, as
    /// [`Dock::set_history`] does the History panel.
    pub(crate) fn set_quickfix(&mut self, quickfix: Entity<QuickfixPanel>, cx: &mut Context<Self>) {
        self.state.add_panel(quickfix.clone(), cx);
        self.quickfix = Some(quickfix);
        self.sync_foot(cx);
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
        self.sync_foot(cx);
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
                (self.outline).update(cx, |outline, cx| outline.focus_from_show(window, cx));
                true
            }
            // The bottom dock's, which the workbench does.
            DockOp::ShowGitLog | DockOp::ShowRuns | DockOp::Problems(_) => false,
            DockOp::RefreshChanges => {
                self.changes.update(cx, GitPanel::refresh);
                true
            }
            DockOp::Toggle => {
                // Its menu closes first, giving focus back to what had it.
                self.state.close_menu(window, cx);
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

    /// Shows the dock with the tool window `panel`, a bottom one moved
    /// here or a panel of its own, focused if it focuses on show. False if
    /// it isn't here.
    pub(crate) fn show_tool(
        &mut self,
        panel: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let shown = self.state.show(panel, window, cx);
        cx.notify();
        shown
    }

    /// Puts back the state [`DockState::save`] saved: whether it shows, its
    /// active panel, whether References has joined the rail and the
    /// Explorer's and Source Control's views' collapse and weights. One
    /// saved before them, with no views, leaves them as built; its Outline
    /// panel active shows the Explorer and its Changes panel Source
    /// Control. A saved Graph view under Source Control is left out.
    ///
    /// A panel saved active that is now the bottom dock's, Problems or the
    /// Git log, leaves the sidebar's active panel as it is: the id of the
    /// tool window the bottom dock is to show, if the sidebar showed it.
    /// The terminal, moved here, comes back closed, with no session: the
    /// sidebar hidden, its active panel as it is.
    pub(crate) fn restore(
        &mut self,
        saved: &fe_command::Value,
        cx: &mut Context<Self>,
    ) -> Option<PanelId> {
        let mut saved = saved.clone();
        let active = saved.get("active").and_then(fe_command::Value::as_str);
        let here = self.state.panel_ids();
        // Moved here, a bottom tool window is the sidebar's to show.
        let bottom = [PROBLEMS_BUTTON, GIT_LOG_BUTTON]
            .into_iter()
            .find(|id| active == Some(*id) && !here.contains(&PanelId(id)))
            .map(PanelId);
        if active == Some(TERMINAL_BUTTON) {
            if let Some(saved) = saved.as_object_mut() {
                saved.remove("active");
            }
            saved["visible"] = false.into();
        }
        let active = saved.get("active").and_then(fe_command::Value::as_str);
        let container = match active {
            Some("outline") => Some(DockPanel::Explorer),
            Some("changes") => Some(DockPanel::SourceControl),
            _ => None,
        };
        if let Some(container) = container {
            saved["active"] = container.id().0.into();
        }
        let shown = saved.get("visible").and_then(fe_command::Value::as_bool);
        self.state.restore(&saved, cx);
        cx.notify();
        bottom.filter(|_| shown != Some(false))
    }

    /// Draws the rail action `id` highlighted, or not, redrawing the rail
    /// if that changes.
    #[cfg(test)]
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

    /// Focuses what the dock shows, as showing it would: search's input,
    /// or the top expanded view of the Explorer or Source Control. False,
    /// leaving focus, if the dock is hidden or its container's views are
    /// all collapsed.
    pub(crate) fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(focus) = self.active_focus(cx) else {
            return false;
        };
        window.focus(&focus, cx);
        true
    }

    /// What [`Dock::focus_active`] focuses.
    fn active_focus(&self, cx: &App) -> Option<FocusHandle> {
        if !self.is_visible() {
            return None;
        }
        let active = self.state.active()?;
        match DockPanel::of(active) {
            // Not lists vim's window keys go to.
            Some(DockPanel::CallHierarchy | DockPanel::AiChanges) => None,
            Some(panel) => self.panel_focus(panel, cx),
            // A bottom tool window moved here.
            None => self.state.panel_focus(active, cx),
        }
    }

    /// What focusing the sidebar's panel `panel` focuses, wherever it is:
    /// search's input, or the top expanded view of the Explorer or Source
    /// Control. None if it hasn't been made, or its views are all
    /// collapsed.
    pub(crate) fn panel_focus(&self, panel: DockPanel, cx: &App) -> Option<FocusHandle> {
        let (container, views) = match panel {
            DockPanel::Explorer => (
                &self.explorer,
                vec![
                    (crate::project_panel::FOLDERS, self.project.focus_handle(cx)),
                    (crate::outline_panel::VIEW, self.outline.focus_handle(cx)),
                ],
            ),
            DockPanel::SourceControl => {
                let changes = (source_control::CHANGES, self.changes.focus_handle(cx));
                (&self.source_control, vec![changes])
            }
            DockPanel::Search => return Some(self.search.focus_handle(cx)),
            DockPanel::References => return Some(self.references.focus_handle(cx)),
            DockPanel::History => return self.history.as_ref().map(|panel| panel.focus_handle(cx)),
            DockPanel::Chat => return self.chat.as_ref().map(|panel| panel.focus_handle(cx)),
            DockPanel::Agents => return Some(self.agents.focus_handle(cx)),
            DockPanel::Quickfix => {
                return self.quickfix.as_ref().map(|panel| panel.focus_handle(cx));
            }
            DockPanel::CallHierarchy => {
                return self.hierarchy.as_ref().map(|panel| panel.focus_handle(cx));
            }
            DockPanel::AiChanges => {
                return self.ai_changes.as_ref().map(|panel| panel.focus_handle(cx));
            }
            _ => return None,
        };
        let stack = container.read(cx).stack().read(cx);
        (stack.view_ids().into_iter())
            .filter(|view| stack.is_collapsed(*view) == Some(false))
            .find_map(|view| views.iter().find(|(id, _)| *id == view))
            .map(|(_, focus)| focus.clone())
    }
}

#[cfg(test)]
impl Dock {
    /// The title row's text: the active item's [`DockItem::title`], as the
    /// header draws it.
    pub(crate) fn title(&self, cx: &App) -> SharedString {
        self.state.title(cx).unwrap_or_default()
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

    /// The workbench runs the op the panel's key would, or hides the dock;
    /// for a bottom tool window moved here, what its foot button does.
    fn rail_clicked(&mut self, panel: PanelId, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = DockPanel::of(panel) {
            cx.emit(DockEvent::Rail(panel));
        } else if let Some(action) = tool_action(panel.0) {
            cx.emit(DockEvent::Action(action.id, action.command));
        }
    }

    /// The workbench moves it.
    fn panel_dropped(
        &mut self,
        panel: PanelId,
        from: EntityId,
        target: DockDrop,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DockEvent::Dropped(panel, from, target));
    }

    /// The workbench runs the action's command, or hides the bottom dock
    /// showing its tool window.
    fn action_clicked(
        &mut self,
        id: &'static str,
        command: &'static str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DockEvent::Action(id, command));
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Moved, its menu offers the other side.
        let side = self.state.position();
        if side != self.menu_side {
            self.menu_side = side;
            self.state.set_menu(dock_menu(side));
        }
        // The foot's titles are read as drawn: a panel's can change, as
        // History's with its file, or a locale's switch, without a sync.
        self.sync_foot(cx);
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
    /// `outline.show`: shows the Explorer with its outline expanded and
    /// focused.
    ShowOutline,
    /// The References button: shows the last results, finding none.
    ShowReferences,
    /// `git.show_changes`: shows Source Control with Changes expanded and
    /// focused.
    ShowChanges,
    /// The History button: shows the history last listed.
    ShowHistory,
    /// `git.log` and `git.log_all`, once they have asked for the list:
    /// show the Git log in the bottom dock, focused (the workbench's).
    ShowGitLog,
    /// `runs.show`: shows the Runs tool window in the bottom dock, focused
    /// (the workbench's).
    ShowRuns,
    /// `nav.show_call_hierarchy`, once it has made the panel and its root
    /// has landed.
    ShowHierarchy,
    /// `quickfix.show`, `location.show` and `:copen`, once they have made
    /// the panel and set the list it shows; its rail button.
    ShowQuickfix,
    /// The Chat button, and `chat.open` once it has made the panel.
    ShowChat,
    /// The Agents button and `agent.show_panel`: shows the Agents panel.
    ShowAgents,
    /// `ai.show_changes`, and a changeset set: shows the AI Changes panel.
    ShowAiChanges,
    /// `git.refresh`: reads the git status again, shown or not.
    RefreshChanges,
    Toggle,
    /// `search.show_replace`: shows search with its replace field, focused.
    ShowReplace,
    /// `search.replace_all`: asks, then replaces in the results' files.
    ReplaceAll,
    /// A problems command, the workbench's: `problems.show` shows the
    /// Problems tool window in the bottom dock.
    Problems(ProblemsOp),
}
