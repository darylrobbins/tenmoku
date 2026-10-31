//! A Git Log commit's expanded rows: the graph rail running unbroken
//! through them, and a file shown as its name then its dimmed directory,
//! on a repository built with the `git` CLI in a temporary directory, read
//! off what the rows painted.

use std::fs;

use gpui::{Hsla, Pixels, Point, TestAppContext};

use super::{commit, draw, focus_panel, git, panel, press, window, write};

/// `base`, then `side` (adds `src/deep/t.rs`) beside `main` (edits
/// `a.rs`), merged by `merge`, which has a body and is checked out.
fn fixture(file: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, "a.rs", "one\n");
    commit(root, "base");
    git(root, &["checkout", "-q", "-b", "side"]);
    fs::create_dir_all(root.join("src/deep")).unwrap();
    write(root, &format!("src/deep/{file}"), "t\n");
    commit(root, "side");
    git(root, &["checkout", "-q", "main"]);
    write(root, "a.rs", "two\n");
    commit(root, "main");
    git(
        root,
        &["merge", "-q", "--no-ff", "-m", "merge\n\nbody line", "side"],
    );
    dir
}

/// Expands the newest commit and draws.
fn expanded(
    cx: &mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (
    gpui::Entity<super::super::GitLogPanel>,
    gpui::VisualTestContext,
) {
    let (workbench, mut vcx) = window(cx, dir.path());
    let panel = panel(&workbench, &mut vcx);
    panel.update(&mut vcx, |panel, cx| panel.select(0, cx));
    focus_panel(&workbench, &mut vcx);
    press("enter", &mut vcx);
    draw(&mut vcx);
    (panel, vcx)
}

/// Where a lane `colors` leaves row `ix` at its bottom edge, and where it
/// enters row `ix` at its top edge: the painted segments' ends there.
fn ends(seen: &super::super::RowsSeen, ix: usize, bottom: bool) -> Vec<(i32, i32, [u32; 4])> {
    let segments = seen
        .segments
        .get(&ix)
        .unwrap_or_else(|| panic!("row {ix} painted no lane"));
    let edge = |(from, to, _): &&(Point<Pixels>, Point<Pixels>, Hsla)| {
        if bottom { to.y } else { from.y }
    };
    let far = segments
        .iter()
        .map(|s| edge(&s))
        .fold(None, |far: Option<Pixels>, y| {
            Some(match far {
                Some(far) if (bottom && far >= y) || (!bottom && far <= y) => far,
                _ => y,
            })
        });
    let far = far.unwrap();
    let mut ends: Vec<_> = (segments.iter().filter(|s| edge(s) == far))
        .map(|(from, to, color)| {
            let end = if bottom { to } else { from };
            let color = [color.h, color.s, color.l, color.a].map(|c| (c * 1000.) as u32);
            (
                f32::from(end.x).round() as i32,
                f32::from(end.y).round() as i32,
                color,
            )
        })
        .collect();
    ends.sort();
    ends
}

#[gpui::test]
fn rail_is_unbroken_through_a_commits_expanded_rows(cx: &mut TestAppContext) {
    let dir = fixture("t.rs");
    let (panel, cx) = expanded(cx, &dir);
    let (lines, seen) = panel.read_with(&cx, |panel, _| (panel.lines(), panel.rows_seen()));
    // The commit, its file, then the next commit.
    assert_eq!(lines[1], "A src/deep/t.rs");
    let seen = seen.borrow();
    for ix in 0..2 {
        let (leaving, entering) = (ends(&seen, ix, true), ends(&seen, ix + 1, false));
        assert_eq!(leaving.len(), 2, "row {ix} carries both lanes: {leaving:?}");
        assert_eq!(leaving, entering, "rows {ix} and {} meet", ix + 1);
    }
    // The rows between draw a stroke the row's whole height.
    for ix in 1..2 {
        for (from, to, _) in &seen.segments[&ix] {
            assert_eq!(from.x, to.x);
            assert_eq!(to.y - from.y, fe_ui::ROW_HEIGHT);
        }
    }
}

#[gpui::test]
fn files_show_their_name_then_their_dimmed_directory(cx: &mut TestAppContext) {
    let dir = fixture("t.rs");
    let (panel, cx) = expanded(cx, &dir);
    let (muted, seen) = panel.read_with(&cx, |panel, _| (panel.muted_text(), panel.rows_seen()));
    let seen = seen.borrow();
    let cells = &seen.cells[&1];
    let texts: Vec<_> = cells.iter().map(|cell| cell.text.as_str()).collect();
    // The file's icon, then its name and directory; its counts and letter
    // at the right edge.
    assert_eq!(texts[1..], ["t.rs", "src/deep", "+1", "A"]);
    assert_eq!(cells[1].color, None, "the name keeps the row's colour");
    assert_eq!(cells[2].color, Some(muted), "the directory is dimmed");
    let (name, dir) = (cells[1].bounds.unwrap(), cells[2].bounds.unwrap());
    assert!(name.right() <= dir.left(), "the name comes first");
}

#[gpui::test]
fn a_long_file_name_ellipsizes_before_its_directory_goes(cx: &mut TestAppContext) {
    let long = format!("{}.rs", "n".repeat(120));
    let dir = fixture(&long);
    let (panel, cx) = expanded(cx, &dir);
    let seen = panel.read_with(&cx, |panel, _| panel.rows_seen());
    let seen = seen.borrow();
    let name = seen.cells[&1][1].bounds.unwrap();
    let list = panel.read_with(&cx, |panel, _| panel.scroll.0.borrow().base_handle.bounds());
    assert!(name.right() <= list.right(), "the name is cut to the row");
    assert!(name.size.width > gpui::px(40.), "the name keeps room");
}

#[path = "git_log_rail_painted_tests.rs"]
mod painted;
