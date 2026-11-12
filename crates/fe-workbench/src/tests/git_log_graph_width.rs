//! The Git Log's graph cells: three lanes wide by default, and wider only
//! for a row that reaches further, which pushes out its own summary alone;
//! an expanded commit's file rows and rail follow its width, and the file
//! letters keep their room at the right edge.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_git::graph::GraphRow;
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px};

use crate::{Launch, Workbench, workbench::git_commands::git_log::GitLogPanel};

const LANE: f32 = 12.;

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str], date: u64) {
    let date = format!("@{date} +0000");
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit(root: &Path, file: &str, summary: &str, date: u64) {
    fs::write(root.join(file), "x\n").unwrap();
    git(root, &["add", "."], date);
    git(root, &["commit", "-q", "-m", summary], date);
}

/// Three commits in a line.
fn linear() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"], 1);
    for (ix, name) in ["a", "b", "c"].into_iter().enumerate() {
        commit(root, &format!("{name}.rs"), name, 10 + ix as u64);
    }
    dir
}

/// `base` and `long`, then six branches off `base` made after them, so the
/// newest rows reach few lanes and the older ones five or more.
fn branchy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"], 1);
    commit(root, "base.rs", "base", 10);
    commit(root, "long.rs", "long", 20);
    for branch in 0..6u64 {
        let name = format!("b{branch}");
        git(root, &["checkout", "-q", "-b", &name, "HEAD~1"], 30);
        commit(
            root,
            &format!("{name}.rs"),
            &format!("branch {branch}"),
            100 + branch,
        );
        git(root, &["checkout", "-q", "main"], 30);
    }
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The Git Log panel listing every branch of `root`, drawn.
fn open(cx: &mut TestAppContext, root: &Path) -> (Entity<GitLogPanel>, VisualTestContext) {
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
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(&mut vcx);
    vcx.update(|window, cx| {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of("git.log_all").unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(&mut vcx);
    let panel = workbench.read_with(&vcx, |workbench, cx| {
        workbench
            .git_log_panel(cx)
            .expect("the dock has its Git Log")
    });
    (panel, vcx)
}

/// How many lanes row `row` reaches across.
fn reach(row: &GraphRow) -> usize {
    let lines = row.top.iter().chain(&row.bottom);
    let widest = lines.map(|line| line.from.max(line.to)).max();
    widest.unwrap_or(0).max(row.column) + 1
}

fn graph(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> Vec<GraphRow> {
    panel.read_with(cx, |panel, _| {
        panel.graph().into_iter().map(|row| row.1).collect()
    })
}

/// The width of each listed row's graph cell or rail, by list index.
fn widths(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> Vec<(usize, Pixels)> {
    let seen = panel.read_with(cx, |panel, _| panel.rows_seen());
    let seen = seen.borrow();
    seen.graph_cells
        .iter()
        .map(|(ix, bounds)| (*ix, bounds.size.width))
        .collect()
}

fn width_of(widths: &[(usize, Pixels)], ix: usize) -> Pixels {
    widths
        .iter()
        .find(|(row, _)| *row == ix)
        .expect("row drawn")
        .1
}

#[gpui::test]
fn a_linear_history_puts_every_summary_at_three_lanes(cx: &mut TestAppContext) {
    let dir = linear();
    let (panel, mut cx) = open(cx, dir.path());
    let cx = &mut cx;
    let rows = graph(&panel, cx);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| reach(row) == 1));
    let widths = widths(&panel, cx);
    assert_eq!(widths.len(), 3);
    for (ix, width) in widths {
        assert_eq!(width, px(3. * LANE), "row {ix}");
    }
}

#[gpui::test]
fn a_row_reaching_a_fifth_lane_pushes_only_its_own_summary_out(cx: &mut TestAppContext) {
    let dir = branchy();
    let (panel, mut cx) = open(cx, dir.path());
    let cx = &mut cx;
    let rows = graph(&panel, cx);
    let widths = widths(&panel, cx);
    assert_eq!(widths.len(), rows.len());
    assert!(rows.iter().any(|row| reach(row) >= 5), "a row reaches five");
    assert!(
        rows.iter().any(|row| reach(row) <= 3),
        "a row reaches three or fewer"
    );
    for (ix, row) in rows.iter().enumerate() {
        let lanes = reach(row).max(3) as f32;
        assert_eq!(width_of(&widths, ix), px(lanes * LANE), "row {ix}");
    }
    let mut distinct: Vec<_> = widths.iter().map(|(_, width)| f32::from(*width)).collect();
    distinct.dedup();
    assert!(distinct.len() > 1, "rows differ: {distinct:?}");
}

#[gpui::test]
fn an_expanded_commits_file_rows_and_rail_follow_that_commits_width(cx: &mut TestAppContext) {
    let dir = branchy();
    let (panel, mut cx) = open(cx, dir.path());
    let cx = &mut cx;
    let rows = graph(&panel, cx);
    let wide = rows.iter().position(|row| reach(row) >= 5).unwrap();
    let narrow = rows.iter().position(|row| reach(row) <= 3).unwrap();
    let (high, low) = (wide.max(narrow), wide.min(narrow));
    // The later commit first, so the earlier one's row index stays put.
    for commit in [high, low] {
        panel.update_in(cx, |panel, window, cx| panel.open_row(commit, window, cx));
        draw(cx);
    }
    let widths = widths(&panel, cx);
    // Each commit's file row sits right under it, before the next
    // commit's own row, which sits one row lower for each file above.
    let under = |commit: usize| {
        if commit == low {
            commit + 1
        } else {
            commit + 2
        }
    };
    for commit in [wide, narrow] {
        let expected = px(reach(&rows[commit]).max(3) as f32 * LANE);
        assert_eq!(
            width_of(&widths, under(commit)),
            expected,
            "commit {commit}'s file row"
        );
    }
}

#[gpui::test]
fn the_file_letters_room_at_the_right_edge_is_unchanged(cx: &mut TestAppContext) {
    let dir = branchy();
    let (panel, mut cx) = open(cx, dir.path());
    let cx = &mut cx;
    let rows = graph(&panel, cx);
    let wide = rows.iter().position(|row| reach(row) >= 5).unwrap();
    let narrow = rows.iter().position(|row| reach(row) <= 3).unwrap();
    let (high, low) = (wide.max(narrow), wide.min(narrow));
    for commit in [high, low] {
        panel.update_in(cx, |panel, window, cx| panel.open_row(commit, window, cx));
        draw(cx);
    }
    let seen = panel.read_with(cx, |panel, _| panel.rows_seen());
    let seen = seen.borrow();
    // Rows shift as the lower commit opens, so only its file row and the
    // other's, one lower, are the current ones.
    let letters: Vec<_> = [low + 1, high + 2]
        .into_iter()
        .map(|ix| {
            seen.cells[&ix]
                .last()
                .and_then(|cell| cell.bounds)
                .expect("a letter")
        })
        .collect();
    for letter in &letters {
        assert_eq!(letter.size.width, px(12.), "the letter's room");
    }
    assert_eq!(letters[0].right(), letters[1].right(), "one right edge");
}
