//! The sidebar's Source Control, as VS Code's: one rail button for a view
//! container of two views, the git changes ([`CHANGES`]: the
//! [`GitPanel`], its commit box and all) over the repository's log
//! ([`GRAPH`]: the [`GitLogPanel`]), both expanded at first.
//! `git.show_changes` and its button show it with Changes expanded and
//! focused; `git.log` and `git.log_all` show it with Graph expanded and
//! focused, listing into it. Graph's header names what it lists, "Graph:
//! main" or "Graph: All Branches" ([`GitLogPanel::title`]), retitled by
//! adding the view again, which keeps its collapse and weight. The views'
//! collapse and weights are the dock's saved state, as the Explorer's are.

use gpui::{App, AppContext as _, Context, Entity};
use kiln_shell::{ViewContainer, ViewId, ViewSpec};

use super::Dock;
use crate::{GitLogPanel, GitPanel};

/// The Source Control container in the sidebar: focused when shown, as
/// its views' rows take keys (the dock focuses the view it shows).
pub(crate) const SIDEBAR: super::SidebarPanel = super::SidebarPanel {
    panel: crate::DockPanel::new("scm"),
    name: "Changes",
    icon: fe_ui::Icon::GitBranch,
    show_command: "git.show_changes",
    show_op: super::DockOp::ShowChanges,
    order: 40,
    always_on_rail: true,
    focus_on_show: true,
};

/// The git changes' view in Source Control.
pub(crate) const CHANGES: ViewId = ViewId("changes");

/// The repository's log's view in Source Control, under the changes.
pub(crate) const GRAPH: ViewId = ViewId("graph");

/// Source Control: `changes` over `git_log`, both expanded.
pub(super) fn container(
    changes: &Entity<GitPanel>,
    git_log: &Entity<GitLogPanel>,
    cx: &mut Context<Dock>,
) -> Entity<ViewContainer> {
    let graph = graph(git_log, cx);
    cx.new(|cx| {
        let mut container = ViewContainer::new(SIDEBAR.panel.id(), SIDEBAR.name, SIDEBAR.icon, cx)
            .show_command(SIDEBAR.show_command);
        container.add_view(changes_view(changes), cx);
        container.add_view(graph, cx);
        container
    })
}

/// The Changes view: `changes`.
pub(super) fn changes_view(changes: &Entity<GitPanel>) -> ViewSpec {
    ViewSpec {
        id: CHANGES,
        title: "Changes".into(),
        order: 10,
        default_collapsed: false,
        view: changes.clone().into(),
    }
}

/// The Graph view: `git_log`, headed with what it lists.
pub(super) fn graph(git_log: &Entity<GitLogPanel>, cx: &App) -> ViewSpec {
    ViewSpec {
        id: GRAPH,
        title: git_log.read(cx).title(cx),
        order: 20,
        default_collapsed: false,
        view: git_log.clone().into(),
    }
}
