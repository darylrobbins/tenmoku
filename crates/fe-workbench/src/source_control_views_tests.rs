//! Source Control: one rail button for a view container of the git
//! changes (Changes) alone, which `git.show_changes` shows, focused; the
//! repository's log is the bottom panel's Git log, which `git.log` and
//! `git.log_all` show, focused, its tab naming what it lists (T1548); and a
//! layout saved with the Changes or Git Log panel active still loads, on
//! repositories built with the `git` CLI in temporary directories.

use std::{fs, path::Path, process::Command};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{DockView as _, PanelId};
use serde_json::json;

use super::{DockPanel, source_control::CHANGES};
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

/// The tool window the bottom panel shows, if it shows, and its tab's
/// title.
fn bottom(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(PanelId, String)> {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.terminal_dock().read(cx).dock();
        let title = dock.title(cx).unwrap_or_default().to_string();
        dock.rail_highlighted().map(|panel| (panel, title))
    })
}

const GIT_LOG: PanelId = PanelId("git_log");

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

    // Its one view: the changes.
    let views = workbench.read_with(cx, |workbench, cx| {
        let container = workbench.dock().read(cx).source_control().read(cx);
        container.view_ids(cx)
    });
    assert_eq!(views, [CHANGES]);
}

#[gpui::test]
fn show_changes_and_git_log_show_their_view(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = opened(cx, dir.path());

    // The dock on the Explorer: git.show_changes shows Source Control,
    // the changes focused.
    dispatch("dock.show_project", cx);
    dispatch("git.show_changes", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    assert_eq!(focused(&workbench, cx), [true, false]);

    // git.log shows the Git log in the bottom panel, listing HEAD's
    // commits, focused, its tab naming the branch; the sidebar stays.
    dispatch("dock.show_project", cx);
    dispatch("git.log", cx);
    assert_eq!(
        bottom(&workbench, cx),
        Some((GIT_LOG, "Graph: main".to_owned()))
    );
    assert_eq!(shown(&workbench, cx).1, DockPanel::Explorer);
    let lines = log(&workbench, cx);
    assert!(lines.iter().any(|line| line.contains("init")), "{lines:?}");
    assert_eq!(focused(&workbench, cx), [false, true]);

    // git.log_all retitles it.
    dispatch("git.log_all", cx);
    assert_eq!(
        bottom(&workbench, cx),
        Some((GIT_LOG, "Graph: All Branches".to_owned()))
    );
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
fn an_old_layout_on_git_log_restores_the_log(cx: &mut TestAppContext) {
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
            "panels": {"scm": {"views": {
                "changes": {"collapsed": false, "weight": 1.0},
                "graph": {"collapsed": true, "weight": 2.0},
            }}},
        }));
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.restore_layout(&saved, window, cx)
        });
        draw(cx);
    };

    // The Git Log panel was active: the bottom panel shows it, listing.
    dispatch("search.show", cx);
    restore("git_log", cx);
    assert_eq!(
        bottom(&workbench, cx).map(|(panel, _)| panel),
        Some(GIT_LOG)
    );
    let lines = log(&workbench, cx);
    assert!(lines.iter().any(|line| line.contains("init")), "{lines:?}");

    // The Changes panel was: Source Control shows, its one view, the
    // saved Graph left out.
    dispatch("search.show", cx);
    restore("changes", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::SourceControl, "Changes".to_owned())
    );
    let views = workbench.read_with(cx, |workbench, cx| {
        let container = workbench.dock().read(cx).source_control().read(cx);
        container.view_ids(cx)
    });
    assert_eq!(views, [CHANGES]);
}
