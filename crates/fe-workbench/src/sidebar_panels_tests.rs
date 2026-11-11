//! The sidebar's panels as their modules declare them ([`SidebarPanel`]):
//! the rail's order, icons and titles, the panels that join it once shown,
//! and each panel's show command, on repositories built with the `git` CLI
//! in temporary directories.

use std::{fs, path::Path, process::Command};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::PanelId;

use super::{DockOp, DockPanel, PANELS, SidebarPanel, panel_icon};
use crate::{Launch, Workbench, tests::open};

/// Each panel as it was before its module declared it, with B3's names
/// and icons (T1540): its name, id, rail icon, show command and that
/// command's op.
fn base() -> [(
    DockPanel,
    &'static str,
    &'static str,
    Icon,
    &'static str,
    DockOp,
); 9] {
    [
        (
            DockPanel::Explorer,
            "Project",
            "project",
            Icon::Files,
            "dock.show_project",
            DockOp::ShowProject,
        ),
        (
            DockPanel::Search,
            "Search",
            "search",
            Icon::Search,
            "search.show",
            DockOp::ShowSearch,
        ),
        (
            DockPanel::SourceControl,
            "Changes",
            "scm",
            Icon::GitBranch,
            "git.show_changes",
            DockOp::ShowChanges,
        ),
        (
            DockPanel::Chat,
            "Chat",
            "chat",
            Icon::Bot,
            "chat.open",
            DockOp::ShowChat,
        ),
        (
            DockPanel::References,
            "References",
            "references",
            Icon::FileSymlink,
            "nav.find_references",
            DockOp::ShowReferences,
        ),
        (
            DockPanel::History,
            "History",
            "history",
            Icon::FileDiff,
            "git.file_history",
            DockOp::ShowHistory,
        ),
        (
            DockPanel::AiChanges,
            "AI Changes",
            "ai_changes",
            Icon::FileDiff,
            "ai.show_changes",
            DockOp::ShowAiChanges,
        ),
        (
            DockPanel::CallHierarchy,
            "Hierarchy",
            "call_hierarchy",
            Icon::ListTree,
            "nav.show_call_hierarchy",
            DockOp::ShowHierarchy,
        ),
        (
            DockPanel::Quickfix,
            "Quickfix List",
            "quickfix",
            Icon::List,
            "quickfix.show",
            DockOp::ShowQuickfix,
        ),
    ]
}

/// `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository of one commit, `a.rs`.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    dir
}

/// A workbench on `root` showing its `a.rs`, focused as the app focuses it.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((root.join("a.rs"), Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The rail's panels, top to bottom, the foot's after the top's, with the
/// icons the last frame drew.
fn rail(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(DockPanel, Icon)> {
    workbench.read_with(cx, |workbench, cx| {
        let drawn = workbench.dock().read(cx).state().drawn();
        (drawn.rail_ids().into_iter())
            .zip(drawn.rail_icons())
            .filter_map(|(id, icon)| Some((DockPanel::of(id)?, icon)))
            .collect()
    })
}

/// Whether the dock shows, the panel it shows and its title row.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel, String) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (dock.is_visible(), dock.active(), title)
    })
}

#[test]
fn each_panel_keeps_its_base_name_id_icon_and_show_command() {
    for (panel, name, id, icon, command, op) in base() {
        assert_eq!(panel.name(), name);
        assert_eq!(panel.id(), PanelId(id));
        assert_eq!(DockPanel::of(PanelId(id)), Some(panel));
        assert_eq!(panel_icon(panel), icon, "{name}");
        assert_eq!(panel.show_command(), command);
        assert_eq!(panel.show_op(), op);
    }
    let declared: Vec<DockPanel> = PANELS.iter().map(|sidebar| sidebar.panel).collect();
    let base: Vec<DockPanel> = base().iter().map(|(panel, ..)| *panel).collect();
    assert_eq!(declared, base, "PANELS is in the base's rail order");
    assert_eq!(DockPanel::of(PanelId("terminal")), None);
}

#[test]
fn the_rail_always_has_the_base_four() {
    // Problems is the bottom panel's now, a foot button (T1548).
    assert_eq!(
        DockPanel::ALL,
        [
            DockPanel::Explorer,
            DockPanel::Search,
            DockPanel::SourceControl,
            DockPanel::Chat,
        ]
    );
    let on_rail: Vec<DockPanel> = (PANELS.iter())
        .filter(|sidebar| sidebar.always_on_rail)
        .map(|sidebar| sidebar.panel)
        .collect();
    assert_eq!(on_rail, DockPanel::ALL);
    let orders: Vec<i32> = PANELS.iter().map(|sidebar| sidebar.order).collect();
    assert!(orders.is_sorted(), "{orders:?}");
    // What each declares of focus.
    let focused = |sidebar: &&&SidebarPanel| sidebar.focus_on_show;
    let focused: Vec<DockPanel> = (PANELS.iter().filter(focused))
        .map(|sidebar| sidebar.panel)
        .collect();
    assert_eq!(
        focused,
        [
            DockPanel::Search,
            DockPanel::SourceControl,
            DockPanel::Chat,
            DockPanel::History,
            DockPanel::AiChanges,
            DockPanel::CallHierarchy,
            DockPanel::Quickfix,
        ]
    );
}

#[gpui::test]
fn the_rail_lists_the_panels_in_base_order_and_others_join_once_shown(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = opened(cx, dir.path());
    let top = [
        (DockPanel::Explorer, Icon::Files),
        (DockPanel::Search, Icon::Search),
        (DockPanel::SourceControl, Icon::GitBranch),
        (DockPanel::Chat, Icon::Bot),
    ];
    // The foot's buttons are the bottom panel's (T1548), not panels.
    let foot: [(DockPanel, Icon); 0] = [];
    let base = [top.as_slice(), &foot].concat();
    assert_eq!(rail(&workbench, cx), base);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Explorer, "Project".to_owned())
    );

    dispatch("problems.show", cx);
    assert_eq!(rail(&workbench, cx), base);

    dispatch("nav.find_references", cx);
    // After the top group, before the foot.
    let references = (DockPanel::References, Icon::FileSymlink);
    let both = [top.as_slice(), &[references], &foot].concat();
    assert_eq!(rail(&workbench, cx), both);

    // The Git log shows in the bottom panel, the sidebar as it was.
    dispatch("git.log", cx);
    assert_eq!(rail(&workbench, cx), both);
    assert_eq!(shown(&workbench, cx).1, DockPanel::References);

    dispatch("git.file_history", cx);
    let history = (DockPanel::History, Icon::FileDiff);
    let with_history = [top.as_slice(), &[references, history], &foot].concat();
    assert_eq!(rail(&workbench, cx), with_history);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::History, "History: a.rs".to_owned())
    );

    dispatch("chat.open", cx);
    assert_eq!(rail(&workbench, cx), with_history);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Chat, "Chat".to_owned())
    );
}

#[gpui::test]
fn each_panels_show_command_shows_it(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = opened(cx, dir.path());
    // Project shows first: another before it, so its command has work.
    let order = [
        DockPanel::Search,
        DockPanel::Explorer,
        DockPanel::SourceControl,
        DockPanel::References,
        DockPanel::History,
        DockPanel::AiChanges,
        DockPanel::Chat,
        DockPanel::Quickfix,
    ];
    for panel in order {
        dispatch(panel.show_command(), cx);
        let (visible, active, title) = shown(&workbench, cx);
        assert_eq!((visible, active), (true, panel), "{}", panel.show_command());
        assert!(title.starts_with(panel.name()), "{title}");
        // Hidden, it shows again.
        dispatch("dock.toggle", cx);
        assert!(!shown(&workbench, cx).0);
        dispatch(panel.show_command(), cx);
        assert_eq!(shown(&workbench, cx).1, panel);
    }
    // Every show command was tried, but Call Hierarchy's, which needs a
    // language server (its own tests show it).
    assert_eq!(order.len() + 1, PANELS.len());
}

/// The Git Log, the bottom panel's tool window, listed by the rail's Git
/// log button.
mod git_log_rail_tests {
    use std::collections::HashSet;

    use fe_ui::Icon;
    use gpui::{TestAppContext, VisualTestContext, Window};

    use kiln_shell::{DockView as _, PanelId};

    use super::{DockPanel, dispatch, draw, opened, rail, repo, shown};
    use crate::{Launch, tests::open};

    /// Clicks the rail button of `panel`.
    fn click(
        workbench: &gpui::Entity<crate::Workbench>,
        panel: DockPanel,
        cx: &mut VisualTestContext,
    ) {
        let bounds = workbench.read_with(cx, |workbench, cx| {
            (workbench.dock().read(cx).rail_button_bounds(panel)).expect("drawn")
        });
        cx.simulate_click(bounds.center(), Default::default());
        draw(cx);
    }

    /// The Git Log panel's rows and its message.
    fn log(
        workbench: &gpui::Entity<crate::Workbench>,
        cx: &mut VisualTestContext,
    ) -> (Vec<String>, Option<String>) {
        workbench.read_with(cx, |workbench, cx| {
            let panel = workbench.dock().read(cx).git_log().expect("made").read(cx);
            (panel.lines(), panel.shown_message())
        })
    }

    #[gpui::test]
    fn the_rail_has_source_control_from_the_start_with_its_own_icon(cx: &mut TestAppContext) {
        let dir = repo();
        let (workbench, cx) = opened(cx, dir.path());
        let panels = |cx: &mut VisualTestContext| {
            (rail(&workbench, cx).into_iter())
                .map(|(panel, _)| panel)
                .collect::<Vec<_>>()
        };
        let distinct = |cx: &mut VisualTestContext| {
            let icons: Vec<Icon> = rail(&workbench, cx)
                .into_iter()
                .map(|(_, icon)| icon)
                .collect();
            let unique: HashSet<_> = icons.iter().map(|icon| icon.path()).collect();
            assert_eq!(unique.len(), icons.len(), "{icons:?}");
            icons
        };
        assert_eq!(
            panels(cx),
            [
                DockPanel::Explorer,
                DockPanel::Search,
                DockPanel::SourceControl,
                DockPanel::Chat,
            ]
        );
        let icons = distinct(cx);
        assert_eq!(icons[2], Icon::GitBranch);

        dispatch("git.log", cx);
        distinct(cx);
        dispatch("git.file_history", cx);
        distinct(cx);
        dispatch("chat.open", cx);
        distinct(cx);
    }

    /// Whether the bottom panel shows the Git log.
    fn log_shown(workbench: &gpui::Entity<crate::Workbench>, cx: &mut VisualTestContext) -> bool {
        workbench.read_with(cx, |workbench, cx| {
            let dock = workbench.terminal_dock().read(cx).dock();
            dock.rail_highlighted() == Some(GIT_LOG)
        })
    }

    const GIT_LOG: PanelId = PanelId("git_log");

    #[gpui::test]
    fn clicking_the_git_log_on_the_rail_lists_heads_log(cx: &mut TestAppContext) {
        let dir = repo();
        let (workbench, cx) = opened(cx, dir.path());
        click(&workbench, DockPanel::new(GIT_LOG.0), cx);
        assert!(log_shown(&workbench, cx));
        assert_eq!(
            shown(&workbench, cx),
            (true, DockPanel::Explorer, "Project".to_owned())
        );
        let (lines, message) = log(&workbench, cx);
        assert!(lines.iter().any(|line| line.contains("init")), "{lines:?}");
        assert_eq!(message, None);

        click(&workbench, DockPanel::new(GIT_LOG.0), cx);
        assert!(!log_shown(&workbench, cx));
    }

    #[gpui::test]
    fn the_git_log_outside_a_repository_says_so_without_a_toast(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let launch = Launch {
            root: Some(dir.path().to_owned()),
            file: None,
        };
        let (workbench, cx, _) = open(cx, launch);
        cx.update(|window: &mut Window, _| window.activate_window());
        draw(cx);
        // Shown by its tab, not its command, which would answer with a
        // toast: it lists when first drawn.
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.show_bottom_panel(GIT_LOG, window, cx)
        });
        draw(cx);
        assert!(log_shown(&workbench, cx));
        assert_eq!(
            log(&workbench, cx).1.as_deref(),
            Some("Not in a git repository")
        );
        let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
        let shown = layer.read_with(cx, |layer, _| layer.queue().shown().len());
        assert_eq!(shown, 0, "no toast");
    }
}
