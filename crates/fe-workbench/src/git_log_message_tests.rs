//! A Git Log commit's whole message: its body listed above its files when
//! it's expanded, and its card scrolling a message taller than the window,
//! on repositories built with the `git` CLI in temporary directories. The
//! card's contents are `git_log_card_tests.rs`'s.

use std::{path::Path, time::Duration};

use fe_ui::ROW_HEIGHT;
use gpui::{
    Entity, Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext,
    VisualTestContext, point, px,
};

use super::{GitLogPanel, dispatch, draw, git, panel, press, window};
use crate::Workbench;

/// The message of the newest commit.
const MESSAGE: &str = "subject\n\nline one\nline two\n\npara\n";

/// `one line` (a.rs added), then [`MESSAGE`] (a.rs edited). Their ids,
/// newest first.
fn fixture() -> (tempfile::TempDir, [String; 2]) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let commit = |text: &str, message: &str| {
        std::fs::write(root.join("a.rs"), text).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", message]);
        git(root, &["rev-parse", "HEAD"]).trim().to_owned()
    };
    let first = commit("one\n", "one line");
    let second = commit("two\n", MESSAGE);
    (dir, [second, first])
}

/// A window on `root` with a.rs open and the Git Log panel shown.
fn log_window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    let (workbench, mut vcx) = window(cx, root, "a.rs");
    dispatch("git.log", &mut vcx);
    (workbench, vcx)
}

fn lines(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.lines())
}

fn selected(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<usize> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.selected())
}

/// The middle of row `ix` of the panel's list, as last drawn, scrolled to
/// the top.
fn row_center(panel: &Entity<GitLogPanel>, ix: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let list = panel.read_with(cx, |panel, _| panel.scroll.0.borrow().base_handle.bounds());
    point(
        list.left() + px(40.),
        list.top() + ROW_HEIGHT * (ix as f32 + 0.5),
    )
}

/// `id`'s author date as `git log` prints it in `git.blame_line`'s
/// popover's format: `2026-10-14 09:30 +0200`.
fn cli_date(root: &Path, id: &str) -> String {
    let format = "--date=format:%Y-%m-%d %H:%M %z";
    let date = git(root, &["log", "-1", "--format=%ad", format, id]);
    date.trim().to_owned()
}

#[gpui::test]
fn expanding_a_commit_lists_its_body_above_its_files(cx: &mut TestAppContext) {
    let (dir, [second, first]) = fixture();
    let (workbench, mut cx) = log_window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let commits = [
        format!("{}  subject", &second[..7]),
        format!("{}  one line", &first[..7]),
    ];
    assert_eq!(all, commits);
    let body = ["line one", "line two", "", "para"].map(str::to_owned);

    // The body is there as soon as the commit expands, its files loading
    // under it; then its files.
    let panel = panel(&workbench, cx);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.open_row(0, window, cx)));
    let loading = lines(&workbench, cx);
    let mut expected = vec![commits[0].clone()];
    expected.extend(body.iter().cloned());
    assert_eq!(loading[..5], expected[..]);
    assert_eq!(loading[5..], ["Loading…".to_owned(), commits[1].clone()]);
    draw(cx);
    expected.extend([
        "1 file changed, +1 -1".to_owned(),
        "M a.rs".to_owned(),
        commits[1].clone(),
    ]);
    assert_eq!(lines(&workbench, cx), expected);

    // The selection moves over the body rows; Enter or `o` on one does
    // nothing.
    assert_eq!(selected(&workbench, cx), Some(0));
    press("j", cx);
    assert_eq!(selected(&workbench, cx), Some(1));
    press("enter", cx);
    press("j", cx);
    press("j", cx);
    assert_eq!(selected(&workbench, cx), Some(3), "on the blank line");
    press("o", cx);
    press("enter", cx);
    assert_eq!(lines(&workbench, cx), expected);
    assert_eq!(selected(&workbench, cx), Some(3));
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len());
    assert_eq!(tabs, 1, "only a.rs is open");

    // A one-line message lists only its files.
    press("G", cx);
    assert_eq!(selected(&workbench, cx), Some(7));
    press("enter", cx);
    let mut both = expected.clone();
    both.extend(["1 file changed, +1 -0".to_owned(), "A a.rs".to_owned()]);
    assert_eq!(lines(&workbench, cx), both);

    // Enter again hides both the body and the files.
    press("g g", cx);
    press("enter", cx);
    assert_eq!(
        lines(&workbench, cx),
        [
            commits[0].clone(),
            commits[1].clone(),
            "1 file changed, +1 -0".to_owned(),
            "A a.rs".to_owned()
        ]
    );
}

#[gpui::test]
fn a_message_taller_than_the_window_scrolls_in_its_card(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    git(root, &["add", "-A"]);
    let body: Vec<String> = (1..=300).map(|line| format!("line {line}")).collect();
    let message = format!("long\n\n{}\n", body.join("\n"));
    git(root, &["commit", "-q", "-m", &message]);
    let id = git(root, &["rev-parse", "HEAD"]).trim().to_owned();
    let (workbench, mut cx) = log_window(cx, root);
    let cx = &mut cx;
    let panel = panel(&workbench, cx);
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    let drawn = || probe.borrow().drawn.clone();
    let row = row_center(&panel, 0, cx);
    cx.simulate_mouse_move(row, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    assert_eq!(probe.borrow().built, 1, "the card shows");
    assert!(probe.borrow().message.ends_with("line 299\nline 300"));
    let dates = format!("just now ({})", cli_date(root, &id));
    assert_eq!(probe.borrow().dates, dates);
    assert_eq!(probe.borrow().short, id[..7]);

    // It's within the window, the author and dates above and the totals,
    // id and buttons below the summary, body and refs, which scroll, in
    // view.
    let window = cx.update(|window, _| window.viewport_size());
    let parts = drawn();
    let names: Vec<_> = parts.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["header", "scrolled", "totals", "footer"]);
    for (_, part) in &parts {
        assert!(
            part.top() >= px(0.) && part.bottom() <= window.height,
            "{part:?} in {window:?}"
        );
    }
    let scroll = probe.borrow().scroll.clone().unwrap();
    assert!(
        scroll.max_offset().y > px(1000.),
        "{:?}",
        scroll.max_offset()
    );
    assert_eq!(scroll.offset().y, px(0.));

    // The mouse moves into it and it stays open; the wheel scrolls its
    // message, the rest staying put.
    let message = parts[1].1;
    cx.simulate_mouse_move(message.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(1000));
    probe.borrow_mut().drawn.clear();
    draw(cx);
    assert_eq!(drawn(), parts, "still shown");
    cx.simulate_event(ScrollWheelEvent {
        position: message.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-200.))),
        ..Default::default()
    });
    probe.borrow_mut().drawn.clear();
    draw(cx);
    assert_eq!(scroll.offset().y, px(-200.));
    assert_eq!(drawn(), parts, "still shown, where it was");

    // Away from the row and the card, it hides.
    cx.simulate_mouse_move(
        point(px(1.), window.height - px(1.)),
        None,
        Modifiers::none(),
    );
    cx.executor().advance_clock(Duration::from_millis(1000));
    draw(cx);
    probe.borrow_mut().drawn.clear();
    draw(cx);
    assert!(drawn().is_empty(), "hidden");
}
