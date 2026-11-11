//! Expanding a Git Log commit to its files and opening a file's change at
//! that commit in a diff view, on repositories built with the `git` CLI in
//! temporary directories.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{DiffView, EditorSettings, EditorTheme, commands::RunCommand};
use fe_ui::ROW_HEIGHT;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use super::GitLogPanel;
use crate::{Launch, Workbench};

/// Runs `git` in `dir`, with the developer's own config shut out, and
/// gives its output.
fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap()
}

fn commit(root: &Path, summary: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", summary]);
    git(root, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn write(root: &Path, file: &str, text: &str) {
    fs::write(root.join(file), text).unwrap();
}

/// `init` (a.rs, d.rs), `edit` (a.rs edited, n.rs added, d.rs deleted),
/// `rename` (a.rs to b.rs), then `topic work` (t.rs) merged with
/// `main work` (m.rs) by `merge`. Each commit's id, by summary.
pub(super) fn fixture() -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    write(root, "a.rs", "one\n");
    write(root, "d.rs", "gone soon\n");
    let init = commit(root, "init");
    write(root, "a.rs", "two\n");
    write(root, "n.rs", "new\n");
    fs::remove_file(root.join("d.rs")).unwrap();
    let edit = commit(root, "edit");
    git(root, &["mv", "a.rs", "b.rs"]);
    let rename = commit(root, "rename");
    git(root, &["checkout", "-q", "-b", "topic"]);
    write(root, "t.rs", "topic\n");
    let topic = commit(root, "topic work");
    git(root, &["checkout", "-q", "main"]);
    write(root, "m.rs", "main\n");
    let main = commit(root, "main work");
    git(root, &["merge", "-q", "--no-ff", "topic", "-m", "merge"]);
    let merge = git(root, &["rev-parse", "HEAD"]).trim().to_owned();
    let ids = [
        ("init", init),
        ("edit", edit),
        ("rename", rename),
        ("topic work", topic),
        ("main work", main),
        ("merge", merge),
    ];
    (dir, ids.into())
}

fn send(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

/// Lets background work land, then draws a frame.
pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

pub(super) fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// A window of a workbench rooted at `root`, focused, with vim on, and the
/// Git Log panel shown.
pub(super) fn window(
    cx: &mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    send("git.log", cx);
    // The log in the bottom panel (T1548) with the room Graph had in the
    // sidebar, the editor above it with the room it had.
    cx.simulate_resize(gpui::size(gpui::px(1920.), gpui::px(1600.)));
    (workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone())).update(
        cx,
        |dock, cx| {
            kiln_shell::DockView::dock_mut(dock).set_size(gpui::px(500.));
            cx.notify();
        },
    );
    draw(cx);
    (workbench, vcx)
}

pub(super) fn panel(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<GitLogPanel> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.dock().read(cx).git_log().cloned()).expect("the Git Log panel was made")
    })
}

pub(super) fn lines(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.lines())
}

pub(super) fn selected(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<usize> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.selected())
}

/// The row of the commit `summary` in `lines`.
pub(super) fn row_of(lines: &[String], summary: &str) -> usize {
    let tail = format!("  {summary}");
    (lines.iter())
        .position(|line| line.ends_with(&tail))
        .unwrap_or_else(|| panic!("no row for {summary}: {lines:?}"))
}

/// Selects `ix`, as a click would.
fn select_row(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let panel = panel(workbench, cx);
    panel.update(cx, |panel, cx| panel.select(ix, cx));
    draw(cx);
}

fn short(id: &str) -> &str {
    &id[..7]
}

/// The active tab's title.
pub(super) fn title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item().expect("a tab").tab_title(cx).to_string()
    })
}

pub(super) fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
}

/// The active tab's diff view.
fn diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    workbench
        .read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_item()?.downcast_ref::<DiffView>().cloned()
        })
        .expect("a diff view is the active tab")
}

/// A diff view's sides: each one's text and whether it's read-only.
fn sides(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [(String, bool); 2] {
    diff.read_with(cx, |diff, cx| {
        [diff.left(), diff.right()].map(|side| {
            let view = side.read(cx);
            (
                view.model().buffer().text().to_string(),
                view.is_read_only(),
            )
        })
    })
}

fn diff_focused(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> bool {
    let [left, right] = diff.read_with(cx, |diff, cx| {
        [diff.left(), diff.right()].map(|side| side.focus_handle(cx))
    });
    cx.update(|window, cx| left.contains_focused(window, cx) || right.contains_focused(window, cx))
}

pub(super) fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let panel = panel(workbench, cx);
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
}

/// The lines under row `ix` of `lines`, up to the next commit's row.
fn under(lines: &[String], ix: usize) -> Vec<String> {
    let is_file =
        |line: &String| line.len() > 2 && line.as_bytes()[1] == b' ' && !line.contains("  ");
    let rest = &lines[ix + 1..];
    // The commit's totals sit above its files.
    let rest = match rest.first() {
        Some(first) if first.contains(" changed") => &rest[1..],
        _ => rest,
    };
    let count = rest
        .iter()
        .take_while(|line| is_file(line) || *line == "Loading…")
        .count();
    rest[..count].to_vec()
}

#[gpui::test]
fn expands(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    assert_eq!(all.len(), 6);
    let edit = row_of(&all, "edit");
    select_row(&workbench, edit, cx);

    // Loading first, while the files are read.
    let panel_entity = panel(&workbench, cx);
    cx.update(|window, cx| panel_entity.update(cx, |panel, cx| panel.open_row(edit, window, cx)));
    let loading = lines(&workbench, cx);
    assert_eq!(under(&loading, edit), ["Loading…"]);
    draw(cx);
    let listed = lines(&workbench, cx);
    assert_eq!(under(&listed, edit), ["M a.rs", "D d.rs", "A n.rs"]);
    assert_eq!(listed.len(), 9);
    assert_eq!(selected(&workbench, cx), Some(edit));

    // Enter again hides them.
    focus_panel(&workbench, cx);
    press("enter", cx);
    assert_eq!(lines(&workbench, cx), all);

    // A rename.
    let rename = row_of(&all, "rename");
    select_row(&workbench, rename, cx);
    press("o", cx);
    let listed = lines(&workbench, cx);
    assert_eq!(under(&listed, rename), ["R a.rs → b.rs"]);

    // A merge lists what differs from its first parent.
    let merge = row_of(&listed, "merge");
    select_row(&workbench, merge, cx);
    press("enter", cx);
    let listed = lines(&workbench, cx);
    assert_eq!(under(&listed, merge), ["A t.rs"]);
    assert!(listed[merge].starts_with(short(&ids[5].1)));

    // The selection moves over commit and file rows alike.
    select_row(&workbench, rename, cx);
    press("j", cx);
    assert_eq!(selected(&workbench, cx), Some(rename + 1));
    press("down", cx);
    assert_eq!(selected(&workbench, cx), Some(rename + 2));
    press("k", cx);
    press("up", cx);
    assert_eq!(selected(&workbench, cx), Some(rename));
}

#[gpui::test]
fn opens(cx: &mut TestAppContext) {
    let (dir, ids) = fixture();
    let id = |summary: &str| ids.iter().find(|(s, _)| *s == summary).unwrap().1.clone();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);

    // A modified file: a.rs at the parent against the commit.
    let edit = row_of(&all, "edit");
    select_row(&workbench, edit, cx);
    press("enter", cx);
    press("j enter", cx);
    let s = short(&id("edit")).to_owned();
    assert_eq!(title(&workbench, cx), format!("a.rs ({s}^ ↔ {s})"));
    let first = diff(&workbench, cx);
    let read_only = |text: &str| (text.to_owned(), true);
    assert_eq!(sides(&first, cx), [read_only("one\n"), read_only("two\n")]);
    assert!(diff_focused(&first, cx));
    assert_eq!(tab_count(&workbench, cx), 1);

    // A deleted file: nothing on the right.
    focus_panel(&workbench, cx);
    press("j enter", cx);
    let s = short(&id("edit")).to_owned();
    assert_eq!(title(&workbench, cx), format!("d.rs ({s}^ ↔ {s})"));
    let deleted = diff(&workbench, cx);
    assert_eq!(
        sides(&deleted, cx),
        [read_only("gone soon\n"), read_only("")]
    );

    // An added one: nothing on the left, opened with `o`.
    focus_panel(&workbench, cx);
    press("j o", cx);
    assert_eq!(title(&workbench, cx), format!("n.rs ({s}^ ↔ {s})"));
    let added = diff(&workbench, cx);
    assert_eq!(sides(&added, cx), [read_only(""), read_only("new\n")]);
    assert_eq!(tab_count(&workbench, cx), 3);

    // A rename: the old path's text on the left, opened by a click.
    let rename = row_of(&lines(&workbench, cx), "rename");
    focus_panel(&workbench, cx);
    select_row(&workbench, rename, cx);
    press("enter", cx);
    let panel_entity = panel(&workbench, cx);
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.select(rename + 1, cx);
            panel.open_row(rename + 1, window, cx)
        })
    });
    draw(cx);
    let s = short(&id("rename")).to_owned();
    assert_eq!(title(&workbench, cx), format!("b.rs ({s}^ ↔ {s})"));
    let renamed = diff(&workbench, cx);
    assert_eq!(
        sides(&renamed, cx),
        [read_only("two\n"), read_only("two\n")]
    );
    assert!(diff_focused(&renamed, cx));
    assert_eq!(tab_count(&workbench, cx), 4);

    // Again: the same tab, focused.
    focus_panel(&workbench, cx);
    let edit = row_of(&lines(&workbench, cx), "edit");
    select_row(&workbench, edit + 1, cx);
    press("enter", cx);
    assert_eq!(diff(&workbench, cx), first);
    assert!(diff_focused(&first, cx));
    assert_eq!(tab_count(&workbench, cx), 4);
}

#[gpui::test]
fn lands_by_request(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let edit = row_of(&all, "edit");
    let panel = panel(&workbench, cx);

    // Expanded, collapsed and expanded again before the first request
    // lands: its older failure doesn't replace the newer loading.
    let (read, old) = panel.update(cx, |panel, cx| {
        panel.toggle(edit, cx);
        let old = panel.requests;
        panel.toggle(edit, cx);
        panel.toggle(edit, cx);
        (panel.read, old)
    });
    panel.update(cx, |panel, cx| {
        panel.land_files(read, (edit, old), Err("old failure".to_owned()), cx)
    });
    let now = panel.read_with(cx, |panel, _| panel.lines());
    assert_eq!(under(&now, edit), ["Loading…"]);
    draw(cx);
    let listed = lines(&workbench, cx);
    assert_eq!(under(&listed, edit), ["M a.rs", "D d.rs", "A n.rs"]);
}

#[gpui::test]
fn clicks_by_row(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let (edit, rename) = (row_of(&all, "edit"), row_of(&all, "rename"));
    let panel = panel(&workbench, cx);

    // The commit rows are the same ones, whatever is listed above them,
    // and their IDs differ from their files'.
    let edit_line = panel.read_with(cx, |panel, _| panel.lines[edit]);
    panel.update(cx, |panel, cx| panel.toggle(rename, cx));
    draw(cx);
    let ids = panel.read_with(cx, |panel, _| {
        panel.lines.iter().map(|line| line.id()).collect::<Vec<_>>()
    });
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());

    // A click on a row that has since moved down acts on it.
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.click(edit_line, 1, window, cx)));
    draw(cx);
    let listed = lines(&workbench, cx);
    let (edit, rename) = (row_of(&listed, "edit"), row_of(&listed, "rename"));
    assert_eq!(under(&listed, edit), ["M a.rs", "D d.rs", "A n.rs"]);
    assert_eq!(under(&listed, rename), ["R a.rs → b.rs"]);
    assert_eq!(selected(&workbench, cx), Some(edit));
}

/// The middle of row `ix` of the panel's list, as last drawn, scrolled to
/// the top.
pub(super) fn row_center(
    panel: &Entity<GitLogPanel>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let list = panel.read_with(cx, |panel, _| panel.scroll.0.borrow().base_handle.bounds());
    point(
        list.left() + px(40.),
        list.top() + ROW_HEIGHT * (ix as f32 + 0.5),
    )
}

// With real mouse events, through gpui's pressed-state tracking and the
// rows' rendered IDs: a press on a row that the landing of an expansion
// replaces before the release clicks neither row.
#[gpui::test]
fn a_release_over_a_row_that_replaced_the_pressed_one_clicks_nothing(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let all = lines(&workbench, cx);
    let edit = row_of(&all, "edit");
    let panel = panel(&workbench, cx);
    let none = Modifiers::none();

    // A click on a commit row expands it, and again collapses it.
    let commit_row = row_center(&panel, edit, cx);
    cx.simulate_click(commit_row, none);
    draw(cx);
    let listed = lines(&workbench, cx);
    assert_eq!(under(&listed, edit), ["M a.rs", "D d.rs", "A n.rs"]);
    assert_eq!(selected(&workbench, cx), Some(edit));
    cx.simulate_click(commit_row, none);
    draw(cx);
    assert_eq!(lines(&workbench, cx), all);

    // `edit` loading, drawn without its files landing: `init` is below
    // the loading row.
    panel.update(cx, |panel, cx| panel.toggle(edit, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let loading = lines(&workbench, cx);
    assert_eq!(under(&loading, edit), ["Loading…"]);
    let pressed = edit + 2;
    assert!(loading[pressed].ends_with("  init"), "{loading:?}");

    // Pressed on `init`; the files land and are drawn, `a.rs`'s row where
    // `init`'s was; released there.
    let at = row_center(&panel, pressed, cx);
    cx.simulate_mouse_down(at, MouseButton::Left, none);
    draw(cx);
    let listed = lines(&workbench, cx);
    assert_eq!(listed[pressed], "D d.rs");
    cx.simulate_mouse_up(at, MouseButton::Left, none);
    draw(cx);
    assert_eq!(lines(&workbench, cx), listed, "init isn't expanded");
    assert_eq!(selected(&workbench, cx), Some(edit), "nor selected");
    assert_eq!(tab_count(&workbench, cx), 0, "nor a.rs opened");

    // An ordinary click on that file row opens it.
    cx.simulate_click(at, none);
    draw(cx);
    assert_eq!(selected(&workbench, cx), Some(pressed));
    assert_eq!(tab_count(&workbench, cx), 1);
    assert!(title(&workbench, cx).starts_with("d.rs ("));
}

#[path = "git_log_tab_names_tests.rs"]
mod tab_names;

#[path = "git_log_rail_tests.rs"]
mod rail;
