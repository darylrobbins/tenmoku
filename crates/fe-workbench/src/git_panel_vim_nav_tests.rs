//! The Changes panel's `gg` and `G`, on repositories built with the `git`
//! CLI in temporary directories.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::GitPanel;
use crate::{Launch, Workbench};

/// Runs `git` in `dir`, with the developer's own config shut out.
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

/// A repository with `a.rs` staged and `b.rs` and `c.rs` changed: three
/// rows in two groups. With `changed` false, nothing is changed.
fn fixture(changed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for file in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.join(file), "head\n").unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    if changed {
        fs::write(root.join("a.rs"), "index\n").unwrap();
        git(root, &["add", "a.rs"]);
        fs::write(root.join("b.rs"), "working\n").unwrap();
        fs::write(root.join("c.rs"), "working\n").unwrap();
    }
    dir
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<GitPanel>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let run = RunCommand(
        Invocation::of("git.show_changes")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    focus(&panel, cx);
    (panel, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn focus(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
}

fn selected(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Option<usize> {
    panel.read_with(cx, |panel, _| panel.selected())
}

fn vim(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_vim_enabled(cx, on));
}

#[gpui::test]
fn first_last(cx: &mut TestAppContext) {
    let dir = fixture(true);
    let (panel, cx) = open(cx, dir.path());
    // Too short for the five rows (two headers, three files), but tall
    // enough for a whole row: the list gets about two of them.
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(260.)));
    draw(cx);
    vim(true, cx);
    focus(&panel, cx);

    let in_view = |ix: usize, cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| {
            let view = panel.list_viewport();
            let row = panel.row_bounds(ix).expect("drawn");
            row.top() >= view.top() && row.bottom() <= view.bottom()
        })
    };
    keys("shift-g", cx);
    assert_eq!(selected(&panel, cx), Some(2));
    assert!(in_view(2, cx));
    assert!(!in_view(0, cx), "the window is too tall to scroll");
    keys("g g", cx);
    assert_eq!(selected(&panel, cx), Some(0));
    assert!(in_view(0, cx));
    keys("shift-g", cx);
    assert_eq!(selected(&panel, cx), Some(2));
    assert!(in_view(2, cx));
}

#[gpui::test]
fn pending_g(cx: &mut TestAppContext) {
    let dir = fixture(true);
    let (panel, cx) = open(cx, dir.path());
    vim(true, cx);
    focus(&panel, cx);

    keys("shift-g k", cx);
    assert_eq!(selected(&panel, cx), Some(1));
    // The g is dropped by the j; it selects the next entry.
    keys("g j", cx);
    assert_eq!(selected(&panel, cx), Some(2));
    // A modified key drops it too.
    keys("g ctrl-x", cx);
    assert_eq!(selected(&panel, cx), Some(2));
    keys("g shift-g", cx);
    assert_eq!(selected(&panel, cx), Some(2));

    // A g before the focus left isn't completed after it.
    keys("g", cx);
    cx.update(|window, cx| window.blur(cx));
    draw(cx);
    focus(&panel, cx);
    keys("g", cx);
    assert_eq!(selected(&panel, cx), Some(2));
    keys("g", cx);
    assert_eq!(selected(&panel, cx), Some(0));

    // Nor after a round trip through the commit box.
    keys("shift-g g", cx);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.focus_commit_box(window, cx)));
    draw(cx);
    focus(&panel, cx);
    keys("g", cx);
    assert_eq!(selected(&panel, cx), Some(2));

    // A g cancels a pending c.
    keys("c g c", cx);
    assert!(!cx.has_pending_prompt());
    let focused = panel.read_with(cx, |panel, _| panel.commit_line_open());
    assert!(!focused);
    let in_box = cx.update(|window, cx| panel.read(cx).box_focused(window, cx));
    assert!(!in_box);
}

#[gpui::test]
fn vim_off(cx: &mut TestAppContext) {
    let dir = fixture(true);
    let (panel, cx) = open(cx, dir.path());
    vim(false, cx);
    focus(&panel, cx);

    keys("down down", cx);
    assert_eq!(selected(&panel, cx), Some(1));
    keys("g g shift-g", cx);
    assert_eq!(selected(&panel, cx), Some(1));
    keys("up", cx);
    assert_eq!(selected(&panel, cx), Some(0));

    // The commit box's keys are its own.
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.focus_commit_box(window, cx)));
    draw(cx);
    cx.simulate_input("g");
    cx.simulate_keystrokes("shift-g");
    draw(cx);
    let text = panel.read_with(cx, |panel, cx| {
        let commit = panel.commit.as_ref().expect("a box");
        commit.editor.read(cx).model().text().to_string()
    });
    assert_eq!(text, "gG");
    assert_eq!(selected(&panel, cx), Some(0));
}

#[gpui::test]
fn empty(cx: &mut TestAppContext) {
    let dir = fixture(false);
    let (panel, cx) = open(cx, dir.path());
    vim(true, cx);
    focus(&panel, cx);

    keys("g g shift-g g", cx);
    keys("g g", cx);
    assert_eq!(selected(&panel, cx), None);
}
