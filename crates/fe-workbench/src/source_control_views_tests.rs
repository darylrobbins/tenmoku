//! Source Control: one rail button for a view container of two views, the
//! git changes (Changes) over the repository's log (Graph), both expanded;
//! `git.show_changes` shows it with Changes, `git.log` and `git.log_all`
//! with Graph, expanded and focused; the views' collapse and weights are
//! saved with the layout, and a layout saved with the Changes or Git Log
//! panel active loads Source Control, on repositories built with the `git`
//! CLI in temporary directories.

use std::{fs, path::Path, process::Command};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{PanelId, ViewId};
use serde_json::json;

use super::{
    DockPanel,
    source_control::{CHANGES, GRAPH},
};
use crate::{Launch, Workbench, tests::open};

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

/// A repository of one commit, `a.rs`, on `main`.
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

/// Whether the dock shows, its active panel and its title row.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel, String) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (dock.is_visible(), dock.active(), title)
    })
}

/// Whether each of Source Control's views, Changes then Graph, is
/// collapsed.
fn collapsed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [Option<bool>; 2] {
    workbench.read_with(cx, |workbench, cx| {
        let container = workbench.dock().read(cx).source_control().read(cx);
        let stack = container.stack().read(cx);
        [stack.is_collapsed(CHANGES), stack.is_collapsed(GRAPH)]
    })
}

/// The weight of each of Source Control's views, Changes then Graph.
fn weights(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [Option<f32>; 2] {
    workbench.read_with(cx, |workbench, cx| {
        let container = workbench.dock().read(cx).source_control().read(cx);
        let stack = container.stack().read(cx);
        [stack.weight(CHANGES), stack.weight(GRAPH)]
    })
}

/// Source Control's view stack, to collapse or weigh its views.
fn stack(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<kiln_shell::ViewStack> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.dock().read(cx).source_control().read(cx).stack()).clone()
    })
}

fn set_collapsed(
    workbench: &Entity<Workbench>,
    view: ViewId,
    collapsed: bool,
    cx: &mut VisualTestContext,
) {
    let stack = stack(workbench, cx);
    stack.update(cx, |stack, cx| stack.set_collapsed(view, collapsed, cx));
    draw(cx);
}

/// What Graph's header says.
fn graph_title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).graph_title().to_string()
    })
}

/// The Git Log's rows.
fn log(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let panel = workbench.dock().read(cx).git_log().expect("made");
        panel.read(cx).lines()
    })
}

/// Whether the Changes panel, else the Git Log, has focus.
fn focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [bool; 2] {
    let (changes, log) = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        (
            dock.changes().clone(),
            dock.git_log().expect("made").clone(),
        )
    });
    cx.update(|window, cx| {
        [
            changes.focus_handle(cx).contains_focused(window, cx),
            log.focus_handle(cx).contains_focused(window, cx),
        ]
    })
}

/// The views' saved state in the window's layout.
fn saved_views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Value {
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    entry["dock"]["panels"]["scm"]["views"].clone()
}

#[gpui::test]
fn the_rail_has_one_source_control_button(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = opened(cx, dir.path());
    dispatch("git.show_changes", cx);
    let (rail, icons, tooltip_title) = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let state = dock.state();
        let rail: Vec<_> = (state.panel_ids().into_iter())
            .filter(|id| state.on_rail(*id, cx))
            .collect();
        let title = state.title(cx).unwrap_or_default().to_string();
        (rail, dock.rail_icons(), title)
    });
    assert_eq!(
        rail,
        [
            DockPanel::Explorer.id(),
            DockPanel::Search.id(),
            DockPanel::SourceControl.id(),
            DockPanel::Chat.id(),
            DockPanel::Problems.id(),
        ]
    );
    for old in ["changes", "git_log"] {
        assert_eq!(DockPanel::of(PanelId(old)), None, "{old}");
    }
    // The Git log's is the rail action's alone, at the foot.
    let graphs = icons.iter().filter(|icon| **icon == Icon::GitGraph);
    assert_eq!(graphs.count(), 1, "{icons:?}");
    let branches = icons.iter().filter(|icon| **icon == Icon::GitBranch);
    assert_eq!(branches.count(), 1, "{icons:?}");
    assert_eq!(icons[2], Icon::GitBranch);
    assert_eq!(DockPanel::SourceControl.name(), "Changes");
    assert_eq!(tooltip_title, "Changes");

    // Its views: the changes over the log, both expanded, Graph's header
    // naming the branch its first draw listed.
    let views = workbench.read_with(cx, |workbench, cx| {
        let container = workbench.dock().read(cx).source_control().read(cx);
        container.view_ids(cx)
    });
    assert_eq!(views, [CHANGES, GRAPH]);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert_eq!(graph_title(&workbench, cx), "Graph: main");
    let lines = log(&workbench, cx);
    assert!(lines.iter().any(|line| line.contains("init")), "{lines:?}");
}

#[gpui::test]
fn show_changes_and_git_log_show_their_view(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = opened(cx, dir.path());

    // Changes collapsed, the dock on the Explorer: git.show_changes shows
    // Source Control with Changes expanded and focused.
    dispatch("git.show_changes", cx);
    set_collapsed(&workbench, CHANGES, true, cx);
    dispatch("dock.show_project", cx);
    dispatch("git.show_changes", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert_eq!(focused(&workbench, cx), [true, false]);

    // Graph collapsed and weighed: git.log expands it, listing HEAD's
    // commits, and focuses it.
    set_collapsed(&workbench, GRAPH, true, cx);
    let stack = stack(&workbench, cx);
    stack.update(cx, |stack, cx| stack.set_weight(GRAPH, 3., cx));
    dispatch("dock.show_project", cx);
    dispatch("git.log", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    assert_eq!(graph_title(&workbench, cx), "Graph: main");
    let lines = log(&workbench, cx);
    assert!(lines.iter().any(|line| line.contains("init")), "{lines:?}");
    assert_eq!(focused(&workbench, cx), [false, true]);

    // git.log_all retitles it, its weight kept.
    dispatch("git.log_all", cx);
    assert_eq!(graph_title(&workbench, cx), "Graph: All Branches");
    assert_eq!(weights(&workbench, cx), [Some(1.), Some(3.)]);
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
}

#[gpui::test]
fn views_collapse_and_weights_survive_a_restart(cx: &mut TestAppContext) {
    let dir = repo();
    let entry = {
        let (workbench, cx) = opened(cx, dir.path());
        dispatch("git.show_changes", cx);
        set_collapsed(&workbench, CHANGES, true, cx);
        // As dragging a divider does.
        let stack = stack(&workbench, cx);
        stack.update(cx, |stack, cx| stack.set_weight(GRAPH, 2.5, cx));
        draw(cx);
        assert_eq!(
            saved_views(&workbench, cx),
            json!({
                "changes": {"collapsed": true, "weight": 1.0},
                "graph": {"collapsed": false, "weight": 2.5},
            })
        );
        workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx))
    };

    let (workbench, cx) = opened(cx, dir.path());
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(false)]);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx)
    });
    draw(cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(true), Some(false)]);
    assert_eq!(weights(&workbench, cx), [Some(1.), Some(2.5)]);
}

/// A layout entry of fastedit's keys alone, its sidebar as `dock`.
fn entry(dock: Value) -> Value {
    json!({
        "docks": {"sidebar": "right", "terminal": "bottom"},
        "dock": dock,
        "breadcrumbs": true,
    })
}

#[gpui::test]
fn an_old_layout_on_git_log_restores_the_graph(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = opened(cx, dir.path());
    let restore = |active: &str, cx: &mut VisualTestContext| {
        let saved = entry(json!({
            "visible": true,
            "active": active,
            "joined": [],
            "size": 300.0,
            "side_size": 300.0,
            "bottom_size": 240.0,
        }));
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.restore_layout(&saved, window, cx)
        });
        draw(cx);
    };

    // The Git Log panel was active: Source Control shows, Graph expanded
    // and listing.
    dispatch("git.show_changes", cx);
    set_collapsed(&workbench, GRAPH, true, cx);
    dispatch("search.show", cx);
    restore("git_log", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx)[1], Some(false));
    let lines = log(&workbench, cx);
    assert!(lines.iter().any(|line| line.contains("init")), "{lines:?}");

    // The Changes panel was: Source Control shows, its views as they were.
    set_collapsed(&workbench, GRAPH, true, cx);
    dispatch("search.show", cx);
    restore("changes", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    assert_eq!(collapsed(&workbench, cx), [Some(false), Some(true)]);
}
