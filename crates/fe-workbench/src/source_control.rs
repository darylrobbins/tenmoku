//! The sidebar's Source Control, as VS Code's: one rail button for a view
//! container of the git changes ([`CHANGES`]: the [`GitPanel`], its commit
//! box and all), its lone view, which the dock's header names.
//! `git.show_changes` and its button show it, focused. The repository's
//! log is the bottom panel's Git log, a tool window of its own (T1548); a
//! layout saved with it here as a Graph view leaves that view out.

use gpui::{AppContext as _, Context, Entity};
use kiln_l10n::tr;
use kiln_shell::{ViewContainer, ViewId, ViewSpec};

use super::Dock;
use crate::GitPanel;

/// The Source Control container in the sidebar: focused when shown, as
/// its views' rows take keys (the dock focuses the view it shows). Its
/// name is the dock's, which every panel's `SIDEBAR` gives as English for
/// now; where this module shows it, it's `workbench-changes-title`.
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

/// Source Control: `changes`.
pub(super) fn container(
    changes: &Entity<GitPanel>,
    cx: &mut Context<Dock>,
) -> Entity<ViewContainer> {
    cx.new(|cx| {
        let mut container = ViewContainer::new(
            SIDEBAR.panel.id(),
            tr!("workbench-changes-title"),
            SIDEBAR.icon,
            cx,
        )
        .show_command(SIDEBAR.show_command);
        container.add_view(changes_view(changes), cx);
        container
    })
}

/// Retitles `container`, Source Control, and its Changes view, `changes`,
/// in the language now active. The view keeps its collapse and weight.
pub(super) fn follow_locale(
    container: &Entity<ViewContainer>,
    changes: &Entity<GitPanel>,
    cx: &mut Context<Dock>,
) {
    let view = changes_view(changes);
    container.update(cx, |container, cx| {
        container.set_title(tr!("workbench-changes-title"), cx);
        container.add_view(view, cx);
    });
}

/// The Changes view: `changes`.
pub(super) fn changes_view(changes: &Entity<GitPanel>) -> ViewSpec {
    ViewSpec {
        id: CHANGES,
        title: tr!("workbench-changes-title"),
        order: 10,
        default_collapsed: false,
        view: changes.clone().into(),
    }
}
