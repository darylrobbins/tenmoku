//! Blame annotations in the gutter column, on the parent's fixture and
//! repositories like it, against what `git blame` and `git log` say.

use std::{fs, ops::Range, path::Path};

use fe_gpui::EditorView;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use super::{
    DAY, EDIT_REST, caret_row, diff, dispatch, ex, fixture, git, git_blame, git_env, open, popover,
    settle, sides, tab_count, tab_editor, title, wait, window,
};
use crate::workbench::git_blame::pin_now;

/// The clock's time, whatever [`super::super::now`] is pinned to.
fn real_now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.unwrap().as_secs() as i64
}

/// Pins the blame's clock at a time, until dropped.
struct Pin;

impl Pin {
    fn at(time: i64) -> Self {
        pin_now(Some(time));
        Pin
    }
}

impl Drop for Pin {
    fn drop(&mut self) {
        pin_now(None);
    }
}

/// Commits everything in `dir` as `author` at `time`, two hours east of
/// UTC: the commit's id.
fn commit_at(dir: &Path, message: &str, author: &str, time: i64) -> String {
    let date = format!("@{time} +0200");
    let env = [
        ("GIT_AUTHOR_DATE", date.as_str()),
        ("GIT_COMMITTER_DATE", date.as_str()),
    ];
    let author = format!("{author} <{}@example.com>", author.to_lowercase());
    git(dir, &["add", "."]);
    git_env(
        dir,
        &["commit", "-q", "--author", &author, "-m", message],
        &env,
    );
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// `time`'s day two hours east of UTC: `YYYY-MM-DD`.
fn day_of(time: i64) -> String {
    let leap = |year: i64| (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mut days = (time + 7200).div_euclid(DAY);
    let mut year = 1970;
    while days >= if leap(year) { 366 } else { 365 } {
        days -= if leap(year) { 366 } else { 365 };
        year += 1;
    }
    let february = if leap(year) { 29 } else { 28 };
    let mut month = 1;
    for len in [31, february, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] {
        if days < len {
            break;
        }
        days -= len;
        month += 1;
    }
    format!("{year:04}-{month:02}-{:02}", days + 1)
}

/// The day `git blame --porcelain` says `row` of `a.rs` was authored.
fn blamed_day(dir: &Path, row: u32) -> String {
    day_of(git_blame(dir, "a.rs", row).2)
}

/// One entry: its buffer rows, text and tint.
type Entry = (Range<u32>, String, Option<f32>);

/// `editor`'s annotations' entries, at the buffer's version now, which
/// they're for; `None` if it has none.
fn entries(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Vec<Entry>> {
    editor.read_with(cx, |editor, _| {
        let annotations = editor.gutter_annotations()?;
        let snapshot = editor.model().snapshot();
        assert_eq!(
            annotations.version,
            snapshot.version(),
            "for the version now"
        );
        let text = snapshot.text();
        let rows = |range: Range<usize>| {
            let first = text.offset_to_point(range.start).row;
            let last = text.offset_to_point(range.end).row;
            let end = if last > first && range.end == text.row_start(last) {
                last
            } else {
                last + 1
            };
            first..end
        };
        Some(
            (annotations.entries.iter())
                .map(|entry| {
                    let range = snapshot.resolve_range(&entry.rows);
                    (rows(range), entry.text.to_string(), entry.heat)
                })
                .collect(),
        )
    })
}

/// The entry showing at `row`, and its index.
fn entry_at(editor: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> (usize, Entry) {
    let entries = entries(editor, cx).expect("annotations");
    (entries.into_iter().enumerate())
        .find(|(_, (rows, ..))| rows.contains(&row))
        .unwrap_or_else(|| panic!("an entry at row {row}"))
}

/// The caret row of the active diff view's right side.
fn diff_row(workbench: &Entity<crate::Workbench>, cx: &mut VisualTestContext) -> u32 {
    let [_, right] = sides(&diff(workbench, cx), cx);
    caret_row(&right, cx)
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<crate::Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.toasts().read(cx);
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn entry(rows: Range<u32>, text: String, heat: f32) -> Entry {
    (rows, text, Some(heat))
}

#[gpui::test]
fn toggling_annotates_each_run(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    fs::write(root.join("new.rs"), "untracked\n").unwrap();
    // Both commits a month and more ago: dates.
    let _pin = Pin::at(real_now() + 60 * DAY);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    assert_eq!(entries(&tab, cx), None, "off by default");

    dispatch("git.toggle_blame_annotations", cx);
    let (ann, bob) = (blamed_day(root, 0), blamed_day(root, 2));
    assert_eq!(blamed_day(root, 13), ann);
    let annotated = vec![
        entry(0..2, format!("{ann} Ann"), 0.),
        entry(2..3, format!("{bob} Bob"), 1.),
        entry(3..14, format!("{ann} Ann"), 0.),
    ];
    assert_eq!(entries(&tab, cx), Some(annotated.clone()));

    // Another tab on the buffer isn't annotated.
    dispatch("pane.split_right", cx);
    let split = tab_editor(&workbench, cx);
    assert_ne!(split, tab);
    let shared =
        cx.update(|_, cx| (split.read(cx).model()).shares_buffer_with(tab.read(cx).model()));
    assert!(shared);
    assert_eq!(entries(&split, cx), None);
    assert_eq!(entries(&tab, cx), Some(annotated.clone()));

    // Again, on the editor annotated: they go.
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    let first = panes[0].clone();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.activate(first, window, cx))
    });
    settle(cx);
    assert_eq!(tab_editor(&workbench, cx), tab);
    assert_eq!(entries(&tab, cx), Some(annotated));
    dispatch("git.toggle_blame_annotations", cx);
    assert_eq!(entries(&tab, cx), None);
    assert_eq!(entries(&split, cx), None);

    // An untracked file: why, and none.
    open(&workbench, &root.join("new.rs"), cx);
    let untracked = tab_editor(&workbench, cx);
    dispatch("git.toggle_blame_annotations", cx);
    assert_eq!(toasts(&workbench, cx), ["Not tracked by git"]);
    assert_eq!(entries(&untracked, cx), None);
}

#[gpui::test]
fn edits_and_head_moves_reblame(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let _pin = Pin::at(real_now() + 60 * DAY);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    dispatch("git.toggle_blame_annotations", cx);
    let (ann, bob) = (blamed_day(root, 0), blamed_day(root, 2));

    // Row 5 typed into: not committed, so blank, once blamed again.
    cx.simulate_keystrokes("5 j A ! escape");
    wait(EDIT_REST, cx);
    assert_eq!(
        entries(&tab, cx),
        Some(vec![
            entry(0..2, format!("{ann} Ann"), 0.),
            entry(2..3, format!("{bob} Bob"), 1.),
            entry(3..5, format!("{ann} Ann"), 0.),
            entry(6..14, format!("{ann} Ann"), 0.),
        ])
    );

    // Committed as Cid, newest: after a git refresh, Cid's.
    ex("w", cx);
    commit_at(root, "Shout row five", "Cid", real_now() - DAY);
    dispatch("git.refresh", cx);
    settle(cx);
    let cid = blamed_day(root, 5);
    assert_eq!(
        entry_at(&tab, 5, cx).1,
        entry(5..6, format!("{cid} Cid"), 1.)
    );
    assert_eq!(
        entry_at(&tab, 2, cx).1,
        entry(2..3, format!("{bob} Bob"), 0.5)
    );

    // Closed, its annotations go; a.rs opened again has none.
    dispatch("workbench.close_tab", cx);
    assert_eq!(entries(&tab, cx), None);
    open(&workbench, &root.join("a.rs"), cx);
    let reopened = tab_editor(&workbench, cx);
    settle(cx);
    assert_eq!(entries(&reopened, cx), None);
}

#[gpui::test]
fn hover_and_click_open_the_commit(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    dispatch("git.toggle_blame_annotations", cx);

    // Hovering Bob's entry: git.blame_line's popover for row 2.
    let (ix, _) = entry_at(&tab, 2, cx);
    let hovered = tab.read_with(cx, |tab, _| {
        let annotations = tab.gutter_annotations().unwrap();
        (annotations.hover)(ix, 2).map(|content| content.text)
    });
    cx.simulate_keystrokes("2 j");
    dispatch("git.blame_line", cx);
    assert!(hovered.is_some());
    assert_eq!(popover(&tab, cx), hovered);

    // What git.open_line_commit opens for row 2, closed again.
    dispatch("git.open_line_commit", cx);
    let opened = title(&workbench, cx);
    let opened_row = diff_row(&workbench, cx);
    dispatch("workbench.close_tab", cx);
    assert_eq!(tab_editor(&workbench, cx), tab);

    // Clicked, with the caret elsewhere: the same.
    cx.simulate_keystrokes("g g");
    let click = |cx: &mut VisualTestContext| {
        let bounds = tab.read_with(cx, |tab, _| tab.annotation_bounds(2));
        cx.simulate_click(
            bounds.expect("row 2 in the column").center(),
            Modifiers::none(),
        );
        settle(cx);
    };
    click(cx);
    let shown = diff(&workbench, cx);
    assert_eq!(title(&workbench, cx), opened);
    assert_eq!(diff_row(&workbench, cx), opened_row);
    assert_eq!(tab_count(&workbench, cx), 2);

    // Again: that one, shown.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    settle(cx);
    click(cx);
    assert_eq!(diff(&workbench, cx), shown);
    assert_eq!(tab_count(&workbench, cx), 2);
}

#[gpui::test]
fn ex_forms(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    let count = |cx: &mut VisualTestContext| entries(&tab, cx).map(|entries| entries.len());

    for line in ["Git blame", "G blame"] {
        ex(line, cx);
        assert_eq!(count(cx), Some(3), ":{line}");
        ex(line, cx);
        assert_eq!(count(cx), None, ":{line}");
    }

    // Line blame keeps its own.
    ex("Gitsigns toggle_current_line_blame", cx);
    settle(cx);
    let line = super::annotations(&tab, cx);
    assert_eq!(line, [(0, super::expected(root, 0))]);
    assert_eq!(count(cx), None);
}

#[gpui::test]
fn recent_commits_show_relative_age(cx: &mut TestAppContext) {
    let now = real_now();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let rows = |changed: &[(usize, &str)]| {
        let mut rows: Vec<String> = (0..4).map(|row| format!("row {row}\n")).collect();
        for &(row, text) in changed {
            rows[row] = format!("{text}\n");
        }
        rows.concat()
    };
    fs::write(root.join("a.rs"), rows(&[])).unwrap();
    commit_at(root, "Old", "Ann", now - 40 * DAY);
    fs::write(root.join("a.rs"), rows(&[(1, "edge")])).unwrap();
    commit_at(root, "Edge", "Bob", now - 30 * DAY);
    fs::write(root.join("a.rs"), rows(&[(1, "edge"), (2, "new")])).unwrap();
    let recent = commit_at(root, "New", "Cid", now - 3 * DAY);
    // A row not committed yet, on disk.
    let text = rows(&[(1, "edge"), (2, "new")]) + "uncommitted\n";
    fs::write(root.join("a.rs"), text).unwrap();
    let _pin = Pin::at(now);
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    dispatch("git.toggle_blame_annotations", cx);

    let (old, edge) = (day_of(now - 40 * DAY), day_of(now - 30 * DAY));
    assert_eq!(
        entries(&tab, cx),
        Some(vec![
            entry(0..1, format!("{old} Ann"), 0.),
            entry(1..2, format!("{edge} Bob"), 0.5),
            entry(2..3, "3 days ago Cid".to_owned(), 1.),
            entry(3..4, format!("{old} Ann"), 0.),
        ])
    );
    assert_eq!(super::super::ago(3 * DAY), "3 days ago");

    // The popover has the recent one's full date.
    let full = git(
        root,
        &[
            "log",
            "-1",
            "--format=%ad",
            "--date=format:%Y-%m-%d %H:%M %z",
            &recent,
        ],
    );
    let (ix, _) = entry_at(&tab, 2, cx);
    let hovered = tab.read_with(cx, |tab, _| {
        (tab.gutter_annotations().unwrap().hover)(ix, 2).map(|content| content.text)
    });
    let hovered = hovered.expect("a popover");
    assert!(
        hovered.contains(&format!("{} (3 days ago)", full.trim())),
        "{hovered}"
    );
}

#[test]
fn long_authors_are_cut() {
    let commit = |author: &str| fe_git::CommitInfo {
        id: "c".repeat(40),
        short: "c".repeat(7),
        author: author.to_owned(),
        email: "a@b".to_owned(),
        time: 0,
        offset_minutes: 0,
        summary: "s".to_owned(),
        message: "s\n".to_owned(),
    };
    let label = |author: &str| super::super::column::label(&commit(author), 40 * DAY);
    assert_eq!(label("Ann"), "1970-01-01 Ann");
    assert_eq!(
        label(&"x".repeat(20)),
        format!("1970-01-01 {}", "x".repeat(20))
    );
    assert_eq!(
        label(&"é".repeat(21)),
        format!("1970-01-01 {}…", "é".repeat(19))
    );
}

#[gpui::test]
fn line_blame_and_annotations_blame_once(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    dispatch("git.toggle_line_blame", cx);
    settle(cx);
    let blames = super::counted(cx).blames;

    // Line blame's blame is shown, not made again.
    dispatch("git.toggle_blame_annotations", cx);
    assert_eq!(entries(&tab, cx).map(|entries| entries.len()), Some(3));
    assert_eq!(super::counted(cx).blames, blames);

    // An edit is blamed once, for both.
    cx.simulate_keystrokes("A ! escape");
    settle(cx);
    assert_eq!(super::counted(cx).blames, blames + 1);
    assert_eq!(entry_at(&tab, 1, cx).1.0, 1..2);
    assert_eq!(
        super::annotations(&tab, cx),
        [(0, "Not Committed Yet".to_owned())]
    );

    // So is a HEAD moved.
    ex("w", cx);
    commit_at(root, "Shout row zero", "Cid", real_now() - 2 * DAY - 3600);
    dispatch("git.refresh", cx);
    settle(cx);
    assert_eq!(super::counted(cx).blames, blames + 2);
    assert!(entry_at(&tab, 0, cx).1.1.ends_with(" Cid"));
    assert_eq!(
        super::annotations(&tab, cx),
        [(0, super::expected(root, 0))]
    );
}
