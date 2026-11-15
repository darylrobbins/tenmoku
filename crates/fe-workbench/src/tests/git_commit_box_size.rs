//! The commit box's height (T333): its bottom edge drags and takes the
//! arrow keys, up to what leaves the list a usable height; a long message
//! scrolls inside it, by the wheel too; and a panel too short for both
//! scrolls as a whole, the wheel over the box scrolling the box alone. On a
//! repository built with the `git` CLI in a temporary directory.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, VisualTestContext, point, px, size,
};

use crate::{GitPanel, Launch, Workbench, git_panel::set_box_text};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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

/// A repository with a few changed files, so the list has rows.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@t")] {
        git(root, &["config", key, value]);
    }
    for name in ["a", "b", "c", "d", "e", "f"] {
        fs::write(root.join(format!("{name}.txt")), "x\n").unwrap();
    }
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench for `root` showing the Changes panel.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<GitPanel>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
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
    let commit_box = panel.read_with(cx, |panel, _| panel.commit_box().cloned());
    (workbench, panel, commit_box.expect("the box"), cx)
}

fn line_height(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    commit_box.read_with(cx, |editor, _| {
        let theme = editor.theme();
        f32::from((theme.font_size * theme.line_height).round())
    })
}

/// The box's height as drawn, in rows.
fn rows(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    let height = f32::from(cx.debug_bounds("commit-box").expect("drawn").size.height);
    height / line_height(commit_box, cx)
}

fn list_height(cx: &mut VisualTestContext) -> f32 {
    f32::from(cx.debug_bounds("change-list").expect("drawn").size.height)
}

fn edge(cx: &mut VisualTestContext) -> Point<gpui::Pixels> {
    cx.debug_bounds("commit-box-edge").expect("drawn").center()
}

/// Drags the box's edge to `y` in the window.
fn drag_to(y: f32, cx: &mut VisualTestContext) {
    let from = edge(cx);
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    let to = point(from.x, px(y));
    for step in 1..=4 {
        let at = point(from.x, from.y + (to.y - from.y) * (step as f32 / 4.));
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
        draw(cx);
    }
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn the_default_is_unchanged_at_three_rows(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, _, commit_box, cx) = open(cx, dir.path());
    let empty = rows(&commit_box, cx);
    assert!((3.0..3.1).contains(&empty), "{empty}");
}

#[gpui::test]
fn dragging_the_handle_grows_the_box_and_shrinks_the_list(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, _, commit_box, cx) = open(cx, dir.path());
    let (before, list_before) = (rows(&commit_box, cx), list_height(cx));
    let lh = line_height(&commit_box, cx);
    let top = f32::from(cx.debug_bounds("commit-box").unwrap().top());
    drag_to(top + lh * 6., cx);
    let after = rows(&commit_box, cx);
    assert!((5.9..6.2).contains(&after), "{before} -> {after}");
    let list_after = list_height(cx);
    assert!(list_after < list_before, "{list_before} -> {list_after}");

    // Back up, no shorter than three rows.
    drag_to(top, cx);
    let least = rows(&commit_box, cx);
    assert!((3.0..3.1).contains(&least), "{least}");

    // The edge takes the arrow keys, and an accessible name.
    cx.update(|window, cx| {
        let panel_focus = cx.focus_handle();
        let _ = panel_focus;
        window.focus_next(cx);
    });
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    assert!(
        nodes
            .iter()
            .any(|node| node.role == fe_ui::Role::Splitter && !node.label.is_empty()),
        "{nodes:?}"
    );
}

#[gpui::test]
fn the_edge_takes_the_arrow_keys(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, panel, commit_box, cx) = open(cx, dir.path());
    panel.update_in(cx, |panel, window, cx| panel.focus_box_edge(window, cx));
    draw(cx);
    cx.simulate_keystrokes("down down");
    draw(cx);
    let grown = rows(&commit_box, cx);
    assert!((4.9..5.2).contains(&grown), "{grown}");
    cx.simulate_keystrokes("up");
    draw(cx);
    let back = rows(&commit_box, cx);
    assert!((3.9..4.2).contains(&back), "{back}");
    cx.simulate_keystrokes("end");
    draw(cx);
    let most = rows(&commit_box, cx);
    assert!(most > 5., "{most}");
    assert!(list_height(cx) >= 71., "{}", list_height(cx));
}

#[gpui::test]
fn a_long_message_scrolls_inside_the_box_past_the_cap(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, _, commit_box, cx) = open(cx, dir.path());
    let message = (0..40).map(|n| format!("line {n}\n")).collect::<String>();
    cx.update(|_, cx| set_box_text(&commit_box, &message, cx));
    draw(cx);
    let tall = rows(&commit_box, cx);
    assert!(tall <= 10.1, "{tall}");
    cx.update(|_, cx| {
        commit_box.update(cx, |editor, cx| {
            editor.model_mut().set_cursor(0);
            cx.notify();
        })
    });
    draw(cx);
    let at = cx.debug_bounds("commit-box").unwrap().center();
    let top = commit_box.read_with(cx, |editor, _| editor.scroll_top());
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., -3.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    let after = commit_box.read_with(cx, |editor, _| editor.scroll_top());
    assert!(after > top, "{top} -> {after}");
}

#[gpui::test]
fn the_height_is_remembered_per_window(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, panel, commit_box, cx) = open(cx, dir.path());
    panel.update_in(cx, |panel, window, cx| panel.focus_box_edge(window, cx));
    cx.simulate_keystrokes("down down down");
    draw(cx);
    let set = rows(&commit_box, cx);
    assert!((5.9..6.2).contains(&set), "{set}");

    // Away from the panel and back, it's as it was, with text in the box too.
    let hide = RunCommand(
        Invocation::of("git.show_changes")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(hide), cx));
    draw(cx);
    let again = rows(&commit_box, cx);
    assert!((5.9..6.2).contains(&again), "{again}");
    let _ = workbench;
}

#[gpui::test]
fn a_short_panel_keeps_the_box_and_a_usable_list(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, _, commit_box, cx) = open(cx, dir.path());
    cx.simulate_resize(size(px(900.), px(200.)));
    draw(cx);
    draw(cx);
    // The box keeps its rows, whole, and the list its least; the panel is
    // what scrolls.
    let box_rows = rows(&commit_box, cx);
    assert!((3.0..3.1).contains(&box_rows), "{box_rows}");
    let list = list_height(cx);
    assert!(list >= 71., "{list}");
}

/// Commits every file of `root`'s, so nothing's changed.
fn commit_all(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "all"]);
}

/// A wheel turn of `lines` at `at`.
fn wheel(at: Point<gpui::Pixels>, lines: f32, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., lines)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
}

#[gpui::test]
fn in_a_short_panel_the_wheel_over_the_box_scrolls_the_box_alone(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, panel, commit_box, cx) = open(cx, dir.path());
    let message = (0..40).map(|n| format!("line {n}\n")).collect::<String>();
    cx.update(|_, cx| set_box_text(&commit_box, &message, cx));
    cx.update(|_, cx| {
        commit_box.update(cx, |editor, cx| {
            editor.model_mut().set_cursor(0);
            cx.notify();
        })
    });
    cx.simulate_resize(size(px(900.), px(200.)));
    draw(cx);
    draw(cx);
    let viewport = panel.read_with(cx, |panel, _| panel.panel_viewport());
    let shown = cx.debug_bounds("commit-box").expect("drawn");
    assert!(
        shown.bottom() > viewport.bottom(),
        "the panel is too short for the box and the list: {shown:?} {viewport:?}"
    );

    // Over the box: its text scrolls, and the panel stays where it is.
    let inside = point(shown.center().x, shown.top().max(viewport.top()) + px(10.));
    let top = commit_box.read_with(cx, |editor, _| editor.scroll_top());
    wheel(inside, -3., cx);
    let after = commit_box.read_with(cx, |editor, _| editor.scroll_top());
    assert!(after > top, "the box scrolls: {top} -> {after}");
    let moved = cx.debug_bounds("commit-box").unwrap();
    assert_eq!(moved.top(), shown.top(), "the panel stays");

    // Over the commit area outside the box: the panel scrolls.
    let outside = point(shown.center().x, shown.top() - px(4.));
    assert!(viewport.contains(&outside), "{outside:?} {viewport:?}");
    let before = commit_box.read_with(cx, |editor, _| editor.scroll_top());
    wheel(outside, -3., cx);
    let moved = cx.debug_bounds("commit-box").unwrap();
    assert!(
        moved.top() < shown.top(),
        "the panel scrolls: {shown:?} -> {moved:?}"
    );
    let still = commit_box.read_with(cx, |editor, _| editor.scroll_top());
    assert_eq!(still, before, "the box's text stays");
}

/// Makes the box as tall as it can be, by End on its edge.
fn grow_to_most(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.focus_box_edge(window, cx));
    draw(cx);
    cx.simulate_keystrokes("end");
    draw(cx);
    draw(cx);
}

/// At its most, the box leaves the list's least in view, all of it, and
/// nothing scrolls.
fn assert_the_least_list_in_view(
    panel: &Entity<GitPanel>,
    commit_box: &Entity<EditorView>,
    state: &str,
    cx: &mut VisualTestContext,
) {
    let most = rows(commit_box, cx);
    assert!(most > 5., "{state}: grown, {most}");
    let viewport = panel.read_with(cx, |panel, _| panel.panel_viewport());
    let list = cx.debug_bounds("change-list").expect("drawn");
    assert!(list.size.height >= px(71.5), "{state}: {list:?}");
    assert!(
        list.bottom() <= viewport.bottom() + px(0.5),
        "{state}: the list in view, {list:?} in {viewport:?}"
    );
}

#[gpui::test]
fn at_its_most_the_box_leaves_the_least_list_in_view(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, panel, commit_box, cx) = open(cx, dir.path());
    grow_to_most(&panel, cx);
    assert_the_least_list_in_view(&panel, &commit_box, "changes", cx);
}

#[gpui::test]
fn in_a_clean_repository_the_box_at_its_most_leaves_the_message_and_list(cx: &mut TestAppContext) {
    let dir = fixture();
    commit_all(dir.path());
    let (_, panel, commit_box, cx) = open(cx, dir.path());
    let message = panel.read_with(cx, |panel, _| panel.message_text());
    assert_eq!(message.as_deref(), Some("No changes"));
    grow_to_most(&panel, cx);
    assert_the_least_list_in_view(&panel, &commit_box, "clean", cx);
}

#[gpui::test]
fn with_an_error_shown_the_box_at_its_most_leaves_it_and_the_list(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    commit_all(root);
    fs::write(root.join(".git/index"), "not an index").unwrap();
    let (_, panel, commit_box, cx) = open(cx, root);
    let message = panel.read_with(cx, |panel, _| panel.message_text());
    assert!(
        message
            .as_ref()
            .is_some_and(|message| message != "No changes"),
        "an error: {message:?}"
    );
    grow_to_most(&panel, cx);
    assert_the_least_list_in_view(&panel, &commit_box, "error", cx);
}

#[gpui::test]
fn up_in_a_shorter_window_takes_a_row_off_the_box_as_shown(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, panel, commit_box, cx) = open(cx, dir.path());
    grow_to_most(&panel, cx);
    let tall = rows(&commit_box, cx);
    cx.simulate_resize(size(px(900.), px(420.)));
    draw(cx);
    draw(cx);
    let shown = rows(&commit_box, cx);
    assert!(shown + 1. < tall, "clamped: {tall} -> {shown}");
    assert!(shown > 4.9, "{shown}");
    panel.update_in(cx, |panel, window, cx| panel.focus_box_edge(window, cx));
    cx.simulate_keystrokes("up");
    draw(cx);
    let after = rows(&commit_box, cx);
    assert!(
        (shown - 1.1..shown - 0.9).contains(&after),
        "one row off: {shown} -> {after}"
    );
}
