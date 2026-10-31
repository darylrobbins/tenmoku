//! The view of a Graph commit's change to all its files, on repositories
//! built with the `git` CLI in temporary directories.

use std::{fs, path::Path, process::Command, sync::Arc, time::Duration};

use fe_ui::{IconRef, IconSet, ROW_HEIGHT};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, VisualTestContext, point, px,
};

use super::{
    super::{
        FileColors,
        commit_diff::Texts,
        git_log_open_tests::{draw, lines, panel, row_center, row_of, tab_count, title, window},
        muted,
    },
    CellSeen, CommitDiffView, Drawn, HeaderSeen,
};
use crate::Workbench;

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

/// Row 5 of a.rs, a long one, its `5` the commit's `6`.
fn long_row(digit: u32) -> String {
    format!("let five = {digit}; {}", "x".repeat(300))
}

/// a.rs's 30 rows, `line <n>` but for row 5's [`long_row`], with `added`
/// after row 25.
fn a_rs(digit: u32, added: &[&str]) -> String {
    let mut rows: Vec<String> = (1..=30).map(|n| format!("line {n}")).collect();
    rows[4] = long_row(digit);
    for (at, row) in added.iter().enumerate() {
        rows.insert(25 + at, (*row).to_owned());
    }
    rows.iter().map(|row| format!("{row}\n")).collect()
}

/// b.rs's first row: characters drawn wider than its length says (in
/// the tests' text system, every character outside the basic plane is two
/// ems).
const WIDE: &str = concat!(
    "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
    "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
    "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
    "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
    "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
);

/// The files [`fixture`]'s `change` edits a row of, after b.rs.
const EDITED: [&str; 4] = ["d.rs", "e.rs", "f.rs", "g.rs"];

/// `base` (a.rs, c.rs, d.rs to g.rs, src/r.rs), then `change`: a.rs's row
/// 5 edited and two rows added after row 25, b.rs added with 200 rows (so
/// the files after it are read only once scrolled near: those whose reads
/// didn't start before it landed), c.rs deleted, row 2 of each of
/// [`EDITED`] edited and src/r.rs renamed to src/s.rs. The root, and the
/// two commits' ids.
fn fixture() -> (tempfile::TempDir, [String; 2]) {
    let dir = tempfile::tempdir().unwrap();
    let root = &dir.path().canonicalize().unwrap();
    git(root, &["init", "-q"]);
    let write = |file: &str, text: &str| fs::write(root.join(file), text).unwrap();
    write("a.rs", &a_rs(5, &[]));
    write("c.rs", "gone\n");
    for file in EDITED {
        write(file, "one\ntwo\n");
    }
    fs::create_dir(root.join("src")).unwrap();
    write("src/r.rs", "fn r() {}\n");
    let base = commit(root, "base");
    write("a.rs", &a_rs(6, &["added one", "added two"]));
    let mut b: String = (1..=200).map(|n| format!("b {n}\n")).collect();
    // A long row of wide characters.
    b.insert_str(0, &format!("{WIDE}\n"));
    write("b.rs", &b);
    fs::remove_file(root.join("c.rs")).unwrap();
    for file in EDITED {
        write(file, "one\n2\n");
    }
    git(root, &["mv", "src/r.rs", "src/s.rs"]);
    let change = commit(root, "change");
    (dir, [base, change])
}

/// A window on the fixture's root with the Graph shown.
fn graph(
    cx: &mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (Entity<Workbench>, VisualTestContext) {
    let root = dir.path().canonicalize().unwrap();
    window(cx, &root)
}

/// Right-clicks the commit `summary`'s row and chooses Open Changes.
fn open_changes(workbench: &Entity<Workbench>, summary: &str, cx: &mut VisualTestContext) {
    let ix = row_of(&lines(workbench, cx), summary);
    let at = row_center(&panel(workbench, cx), ix, cx);
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw(cx);
    let item = cx
        .debug_bounds("menu-item:Open Changes")
        .expect("the commit's menu offers Open Changes");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    draw(cx);
}

fn held(cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| Workbench::git_work_held(cx))
}

/// Lets every section read held go on, and those they start, until none
/// is held.
fn release_all(cx: &mut VisualTestContext) {
    while held(cx) > 0 {
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
    }
    draw(cx);
}

/// Opens `change`'s view with its sections' reads held: b.rs's lands
/// first, so the files after it are read only once scrolled near (those
/// whose reads had started land too). Reads started later stay held
/// until [`release_all`].
fn open_held(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    open_listed(workbench, cx);
    assert_eq!(
        held(cx),
        super::super::commit_diff::READS,
        "a.rs to d.rs are read"
    );
    cx.update(|_, cx| Workbench::release_git_work(1, cx));
    draw(cx);
    release_all(cx);
}

/// Opens `change`'s view with its reads held, letting its file listing
/// land: its first sections' reads are held.
fn open_listed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    open_changes(workbench, "change", cx);
    assert_eq!(held(cx), 1, "the listing is held");
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
}

/// Clicks the row `file` (`M a.rs`) under the commit `summary` in the
/// Graph, expanding the commit first if it isn't.
fn click_file(
    workbench: &Entity<Workbench>,
    summary: &str,
    file: &str,
    cx: &mut VisualTestContext,
) {
    let panel = panel(workbench, cx);
    let commit = row_of(&lines(workbench, cx), summary);
    let listed = |cx: &mut VisualTestContext| {
        let rows = lines(workbench, cx);
        let under = rows[commit + 1..]
            .iter()
            .take_while(|row| !row.contains("  "));
        under.clone().position(|row| row == file)
    };
    if listed(cx).is_none() {
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.open_row(commit, window, cx)));
        draw(cx);
    }
    let at = commit + 1 + listed(cx).unwrap_or_else(|| panic!("no {file} under {summary}"));
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    let at = row_center(&panel, at, cx);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    draw(cx);
}

/// The active tab's commit diff view.
fn active_view(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<CommitDiffView> {
    workbench
        .read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_item()?
                .downcast_ref::<CommitDiffView>()
                .cloned()
        })
        .expect("a commit diff view is the active tab")
}

fn focused(view: &Entity<CommitDiffView>, cx: &mut VisualTestContext) -> bool {
    let focus = view.read_with(cx, |view, cx| view.focus_handle(cx));
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

/// Each section's path, from the root.
fn paths(view: &Entity<CommitDiffView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| {
        let diff = view.diff().read(cx);
        (diff.sections().iter())
            .map(|section| diff.display_path(&section.path))
            .collect()
    })
}

/// The section of `path`'s index.
fn section(view: &Entity<CommitDiffView>, path: &str, cx: &mut VisualTestContext) -> usize {
    let paths = paths(view, cx);
    (paths.iter().position(|at| at == path)).unwrap_or_else(|| panic!("no {path}: {paths:?}"))
}

/// The rows drawn last.
fn drawn(view: &Entity<CommitDiffView>, cx: &mut VisualTestContext) -> Vec<(usize, Drawn)> {
    let seen = view.read_with(cx, |view, _| view.seen());
    let seen = seen.borrow();
    seen.rows
        .iter()
        .map(|(ix, row)| (*ix, row.clone()))
        .collect()
}

fn drawn_row(view: &Entity<CommitDiffView>, ix: usize, cx: &mut VisualTestContext) -> Drawn {
    let rows = drawn(view, cx);
    let row = rows.iter().find(|(at, _)| *at == ix);
    row.unwrap_or_else(|| panic!("row {ix} isn't drawn: {rows:?}"))
        .1
        .clone()
}

fn header_at(view: &Entity<CommitDiffView>, ix: usize, cx: &mut VisualTestContext) -> HeaderSeen {
    match drawn_row(view, ix, cx) {
        Drawn::Header(header) => header,
        other => panic!("row {ix} is {other:?}, not a header"),
    }
}

fn cells(
    view: &Entity<CommitDiffView>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> [Option<CellSeen>; 2] {
    match drawn_row(view, ix, cx) {
        Drawn::Pair(cells) => cells,
        other => panic!("row {ix} is {other:?}, not a diff row"),
    }
}

/// The middle of the view's row `ix` as drawn, its left side's if `left`
/// else its right's, scrolled to the top.
fn view_row(
    view: &Entity<CommitDiffView>,
    ix: usize,
    left: bool,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let list = view.read_with(cx, |view, _| view.list_bounds());
    let x = match left {
        true => list.left() + list.size.width / 4.,
        false => list.left() + list.size.width * 0.75,
    };
    point(x, list.top() + ROW_HEIGHT * (ix as f32 + 0.5))
}

fn click(selector: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let at = (cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(at.center(), Modifiers::none());
    draw(cx);
    draw(cx);
}

fn wheel(at: Point<Pixels>, delta: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(delta),
        ..ScrollWheelEvent::default()
    });
    draw(cx);
    draw(cx);
}

fn colors(cx: &mut VisualTestContext) -> FileColors {
    cx.update(|_, cx| FileColors::new(&fe_ui::ui_theme(cx), cx))
}

/// Answers a `.rs` file with one asset, anything else with another.
struct Stub;

impl IconSet for Stub {
    fn file(&self, name: &str) -> IconRef {
        match name.ends_with(".rs") {
            true => IconRef::Asset("stub/rust.svg".into()),
            false => IconRef::Asset("stub/other.svg".into()),
        }
    }

    fn folder(&self, _: bool) -> IconRef {
        IconRef::Asset("stub/folder.svg".into())
    }
}

#[gpui::test]
fn opens(cx: &mut TestAppContext) {
    let (dir, [base, change]) = fixture();
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;
    assert_eq!(tab_count(&workbench, cx), 0);

    // Open Changes on a commit row: one tab, a section a file, focused.
    open_changes(&workbench, "change", cx);
    let changed = active_view(&workbench, cx);
    assert_eq!(
        title(&workbench, cx),
        format!("{} - change (8 files)", &change[..7])
    );
    let mut sorted = paths(&changed, cx);
    sorted.sort();
    let files = [
        "a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs", "g.rs", "src/s.rs",
    ];
    assert_eq!(sorted, files);
    assert_eq!(tab_count(&workbench, cx), 1);
    assert!(focused(&changed, cx));
    let first = paths(&changed, cx)[0].clone();
    assert_eq!(
        header_at(&changed, 0, cx).name,
        first,
        "the first section's header is drawn first"
    );

    // Another commit's is another tab.
    open_changes(&workbench, "base", cx);
    let based = active_view(&workbench, cx);
    assert_ne!(based, changed);
    assert_eq!(tab_count(&workbench, cx), 2);
    assert_eq!(
        title(&workbench, cx),
        format!("{} - base (7 files)", &base[..7])
    );

    // Asked again: the tab open, focused.
    open_changes(&workbench, "change", cx);
    assert_eq!(active_view(&workbench, cx), changed);
    assert_eq!(tab_count(&workbench, cx), 2);
    assert!(focused(&changed, cx));

    // The card's files-changed count opens it too: here, the one open.
    open_changes(&workbench, "base", cx);
    assert_eq!(active_view(&workbench, cx), based);
    let panel = panel(&workbench, cx);
    let row = row_center(&panel, row_of(&lines(&workbench, cx), "change"), cx);
    cx.simulate_mouse_move(row, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    draw(cx);
    let files = (cx.debug_bounds("commit-card-files")).expect("the card's file count is drawn");
    cx.simulate_mouse_move(files.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    cx.simulate_click(files.center(), Modifiers::none());
    draw(cx);
    draw(cx);
    assert_eq!(active_view(&workbench, cx), changed);
    assert_eq!(tab_count(&workbench, cx), 2);
    assert!(focused(&changed, cx));
}

#[gpui::test]
fn header(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;
    open_changes(&workbench, "change", cx);
    let view = active_view(&workbench, cx);
    let colors = colors(cx);
    // Every header in view.
    click("icon-button:commit-diff-collapse-all", cx);
    let header_of = |path: &str, cx: &mut VisualTestContext| {
        let ix = section(&view, path, cx);
        let row = view.read_with(cx, |view, _| view.header_row(ix)).unwrap();
        header_at(&view, row, cx)
    };
    let counts = |added: u32, removed: u32| {
        let mut counts = Vec::new();
        if added > 0 {
            counts.push((format!("+{added}"), muted(colors.added)));
        }
        if removed > 0 {
            counts.push((format!("-{removed}"), muted(colors.deleted)));
        }
        counts
    };
    let expected = |path: &str, dir: &str, letter: char, added, removed| HeaderSeen {
        collapsed: true,
        icon: fe_ui::file_icon_path(path.rsplit('/').next().unwrap()),
        name: path.rsplit('/').next().unwrap().to_owned(),
        dir: dir.to_owned(),
        dir_color: colors.muted,
        note: None,
        counts: counts(added, removed),
        letter,
        letter_color: colors.letter(letter),
    };

    // The icon, the name, the directory dimmed, the counts and the letter.
    assert_eq!(header_of("a.rs", cx), expected("a.rs", "", 'M', 3, 1));
    assert_eq!(header_of("b.rs", cx), expected("b.rs", "", 'A', 201, 0));
    assert_eq!(header_of("c.rs", cx), expected("c.rs", "", 'D', 0, 1));
    for file in EDITED {
        assert_eq!(header_of(file, cx), expected(file, "", 'M', 1, 1));
    }
    assert_eq!(
        header_of("src/s.rs", cx),
        expected("src/s.rs", "src ← src/r.rs", 'R', 0, 0)
    );
    assert_ne!(colors.letter('A'), colors.letter('D'));

    // The icon comes through the installed set.
    cx.update(|_, cx| fe_ui::set_icon_set(Arc::new(Stub), cx));
    draw(cx);
    assert_eq!(header_of("a.rs", cx).icon, "stub/rust.svg");
    cx.update(|_, cx| fe_ui::reset_icon_set(cx));
    draw(cx);
    assert_eq!(header_of("a.rs", cx).icon, fe_ui::file_icon_path("a.rs"));

    // A click on the chevron expands a section, and on the header
    // collapses it again; the others stay as they are.
    let d = section(&view, "d.rs", cx);
    let collapsed = |ix: usize, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| view.diff().read(cx).is_collapsed(ix))
    };
    click(&format!("commit-diff-chevron:{d}"), cx);
    assert!(!collapsed(d, cx));
    assert!(!header_of("d.rs", cx).collapsed);
    let a = section(&view, "a.rs", cx);
    assert!(collapsed(a, cx));
    let d_row = view.read_with(cx, |view, _| view.header_row(d)).unwrap();
    assert!(
        matches!(
            drawn_row(&view, d_row + 1, cx),
            Drawn::Pair(_) | Drawn::Hidden(_)
        ),
        "its diff shows under it"
    );
    click(&format!("commit-diff-header:{d}"), cx);
    assert!(collapsed(d, cx));
    assert!(header_of("d.rs", cx).collapsed);
    assert!(
        matches!(drawn_row(&view, d_row + 1, cx), Drawn::Header(_)),
        "the next header follows"
    );
}

#[gpui::test]
fn body(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;
    open_held(&workbench, cx);
    let view = active_view(&workbench, cx);
    assert_eq!(section(&view, "a.rs", cx), 0, "a.rs is first");
    let theme = view.read_with(cx, |view, cx| view.theme(cx));
    let same = |number: u32, text: &str| CellSeen {
        number,
        text: text.to_owned(),
        tint: None,
        words: Vec::new(),
        word_tint: None,
        offset: px(0.),
        text_x: Some(px(0.)),
    };
    let hidden = |ix: usize, cx: &mut VisualTestContext| match drawn_row(&view, ix, cx) {
        Drawn::Hidden(text) => text,
        other => panic!("row {ix} is {other:?}"),
    };

    // Unchanged rows away from the hunks folded; those near them on both
    // sides, numbered.
    assert_eq!(hidden(1, cx), "1 hidden line");
    assert_eq!(
        cells(&view, 2, cx),
        [Some(same(2, "line 2")), Some(same(2, "line 2"))]
    );
    assert_eq!(hidden(9, cx), "14 hidden lines");
    assert_eq!(
        cells(&view, 10, cx),
        [Some(same(23, "line 23")), Some(same(23, "line 23"))]
    );
    assert_eq!(hidden(18, cx), "2 hidden lines");

    // A modified pair: each row tinted, its changed word darker.
    let [left, right] = cells(&view, 5, cx);
    let (left, right) = (left.unwrap(), right.unwrap());
    assert_eq!((left.number, left.text.as_str()), (5, long_row(5).as_str()));
    assert_eq!(
        (left.tint, left.word_tint),
        (Some(theme.diff_removed_row), Some(theme.diff_removed_word))
    );
    assert_eq!(left.words, vec![11..12], "the 5");
    assert_eq!(
        (right.number, right.text.as_str()),
        (5, long_row(6).as_str())
    );
    assert_eq!(
        (right.tint, right.word_tint),
        (Some(theme.diff_added_row), Some(theme.diff_added_word))
    );
    assert_eq!(right.words, vec![11..12], "the 6");

    // Added rows beside the left side's filler, darker throughout.
    let [filler, added] = cells(&view, 13, cx);
    assert_eq!(filler, None);
    let added = added.unwrap();
    assert_eq!((added.number, added.text.as_str()), (26, "added one"));
    assert_eq!(added.words, vec![0..9]);
    assert_eq!(added.tint, Some(theme.diff_added_row));
    assert_eq!(
        cells(&view, 15, cx),
        [Some(same(26, "line 26")), Some(same(28, "line 26"))]
    );

    // A click on a fold shows its rows.
    let b_header = view.read_with(cx, |view, _| view.header_row(1)).unwrap();
    click("commit-diff-hidden:9", cx);
    assert_eq!(
        cells(&view, 9, cx),
        [Some(same(9, "line 9")), Some(same(9, "line 9"))]
    );
    assert_eq!(
        cells(&view, 22, cx),
        [Some(same(22, "line 22")), Some(same(22, "line 22"))]
    );
    let moved = view.read_with(cx, |view, _| view.header_row(1)).unwrap();
    assert_eq!(moved, b_header + 13);

    // A sideways scroll over a section moves both its sides, the rest of
    // it with them, and no other section.
    wheel(view_row(&view, 5, false, cx), point(px(-120.), px(0.)), cx);
    let [left, right] = cells(&view, 5, cx).map(Option::unwrap);
    assert_eq!([left.offset, right.offset], [px(120.); 2]);
    assert_eq!(
        [left.text_x, right.text_x],
        [Some(px(-120.)); 2],
        "both drawn scrolled"
    );
    let [left, right] = cells(&view, 2, cx).map(Option::unwrap);
    assert_eq!([left.text_x, right.text_x], [Some(px(-120.)); 2]);
    let [filler, added] = cells(&view, moved + 1, cx);
    assert_eq!(filler, None, "b.rs was added");
    assert_eq!(added.unwrap().text_x, Some(px(0.)), "b.rs stays");
    // Back past the start stops there.
    wheel(view_row(&view, 5, true, cx), point(px(500.), px(0.)), cx);
    let [left, right] = cells(&view, 5, cx).map(Option::unwrap);
    assert_eq!([left.offset, right.offset], [px(0.); 2]);
    // As far as the end of the section's widest row, whatever its line
    // numbers' width (a.rs's two digits, b.rs's three) and however wide its
    // characters (b.rs's first row's): its end at the left side's edge.
    let room = |ix: usize, side: usize, cx: &mut VisualTestContext| {
        let seen = view.read_with(cx, |view, _| view.seen());
        let room = seen.borrow().room.get(&(ix, side)).copied();
        room.unwrap_or_else(|| panic!("row {ix} side {side} isn't drawn"))
    };
    let wide = moved + 1;
    assert_eq!(cells(&view, wide, cx)[1].as_ref().unwrap().text, WIDE);
    for (row, side) in [(5, 0), (wide, 1)] {
        assert!(room(row, side, cx) < px(0.), "row {row} runs past its side");
        wheel(
            view_row(&view, row, side == 0, cx),
            point(px(-100_000.), px(0.)),
            cx,
        );
        let room = room(row, side, cx);
        // Its end in view, a few pixels at most from the edge, the layout
        // rounding the sides' widths.
        assert!(
            room >= px(0.) && room <= px(4.),
            "row {row} ends {room:?} from its edge"
        );
    }
    wheel(
        view_row(&view, 5, true, cx),
        point(px(100_000.), px(0.)),
        cx,
    );

    // The whole view scrolls up and down, the model reading the sections
    // it comes near.
    let g = section(&view, "g.rs", cx);
    let unread = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| {
            matches!(view.diff().read(cx).sections()[g].texts, Texts::Unread)
        })
    };
    assert!(unread(cx), "g.rs is far down");
    assert_eq!(view.read_with(cx, |view, _| view.viewport().start), 0);
    wheel(
        view_row(&view, 5, true, cx),
        point(px(0.), -ROW_HEIGHT * 200.),
        cx,
    );
    release_all(cx);
    let top = view.read_with(cx, |view, _| view.viewport().start);
    assert!(top >= 150, "scrolled down to {top}");
    // Drawn from there, past row 0, which the list draws to measure.
    let rows: Vec<usize> = drawn(&view, cx).iter().map(|(ix, _)| *ix).collect();
    assert_eq!(rows.iter().find(|ix| **ix > 0), Some(&top), "{rows:?}");
    assert!(!unread(cx), "g.rs is read once near");
}

#[gpui::test]
fn navigation(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;
    open_held(&workbench, cx);
    let view = active_view(&workbench, cx);
    let files = [
        "a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs", "g.rs", "src/s.rs",
    ];
    assert_eq!(paths(&view, cx), files, "the commit's file order");
    let [a, b, c] = [0, 1, 2];
    let g = 6;
    let current = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.current_change());
    let shown = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.viewport());
    let hunk_row = |section, hunk, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.hunk_row(section, hunk))
            .unwrap()
    };
    let texts_unread = |ix: usize, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| {
            matches!(view.diff().read(cx).sections()[ix].texts, Texts::Unread)
        })
    };
    let next = |cx: &mut VisualTestContext| {
        click("icon-button:commit-diff-next", cx);
        release_all(cx);
    };
    let previous = |cx: &mut VisualTestContext| {
        click("icon-button:commit-diff-previous", cx);
        release_all(cx);
    };

    // Hunk to hunk in order, within a section and across them.
    next(cx);
    assert_eq!(current(cx), Some((a, Some(0))));
    assert!(shown(cx).contains(&hunk_row(a, 0, cx)));
    next(cx);
    assert_eq!(current(cx), Some((a, Some(1))));
    next(cx);
    assert_eq!(current(cx), Some((b, Some(0))));
    assert!(shown(cx).contains(&hunk_row(b, 0, cx)));

    // On through the rest, those not read yet read first and then gone to
    // their first hunk.
    assert!(texts_unread(g, cx), "g.rs is far down");
    for (section, file) in files.iter().enumerate().take(g + 1).skip(c) {
        next(cx);
        assert_eq!(current(cx), Some((section, Some(0))), "{file}");
        assert!(shown(cx).contains(&hunk_row(section, 0, cx)));
    }
    // The rename changes nothing: on past it, wrapping to the first.
    next(cx);
    assert_eq!(current(cx), Some((a, Some(0))));
    assert!(shown(cx).contains(&hunk_row(a, 0, cx)));
    // And back, wrapping to the last.
    previous(cx);
    assert_eq!(current(cx), Some((g, Some(0))));
    previous(cx);
    let f = g - 1;
    assert_eq!(current(cx), Some((f, Some(0))));

    // A collapsed section it lands in is expanded; the others stay.
    click("icon-button:commit-diff-collapse-all", cx);
    let collapsed = |ix: usize, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| view.diff().read(cx).is_collapsed(ix))
    };
    next(cx);
    assert_eq!(current(cx), Some((g, Some(0))));
    assert!(!collapsed(g, cx));
    assert!(collapsed(f, cx) && collapsed(a, cx) && collapsed(b, cx));
    assert!(shown(cx).contains(&hunk_row(g, 0, cx)));
    next(cx);
    assert_eq!(current(cx), Some((a, Some(0))));
    assert!(!collapsed(a, cx));
    next(cx);
    next(cx);
    assert_eq!(current(cx), Some((b, Some(0))));
    assert!(!collapsed(b, cx) && collapsed(c, cx));

    // A file clicked in the Graph scrolls the open view to its section,
    // expanding it, and shows the tab.
    let panel = panel(&workbench, cx);
    let commit = row_of(&lines(&workbench, cx), "change");
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.open_row(commit, window, cx)));
    draw(cx);
    let rows = lines(&workbench, cx);
    let file = |name: &str| commit + rows[commit..].iter().position(|row| row == name).unwrap();
    let c_row = file("D c.rs");
    cx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    let at = row_center(&panel, c_row, cx);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    draw(cx);
    assert_eq!(active_view(&workbench, cx), view);
    assert_eq!(tab_count(&workbench, cx), 1, "no other tab opened");
    assert!(!collapsed(c, cx));
    // As near the top as the end of the rows lets it be.
    let header = view.read_with(cx, |view, _| view.header_row(c)).unwrap();
    assert!(shown(cx).contains(&header), "{header} in {:?}", shown(cx));
    assert_eq!(current(cx), Some((c, None)));
    // Enter on one does the same; next change goes on from there.
    let a_row = file("M a.rs");
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.open_row(a_row, window, cx)));
    draw(cx);
    draw(cx);
    let header = view.read_with(cx, |view, _| view.header_row(a)).unwrap();
    assert_eq!(shown(cx).start, header);
    next(cx);
    assert_eq!(current(cx), Some((a, Some(0))));
}

#[gpui::test]
fn targets_hold_while_sections_land(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;
    let files = [
        "a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs", "g.rs", "src/s.rs",
    ];
    open_listed(&workbench, cx);
    let view = active_view(&workbench, cx);
    assert_eq!(paths(&view, cx), files);
    let [d, e] = [3, 4];
    let current = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.current_change());
    let shown = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.viewport());
    let collapsed = |ix: usize, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| view.diff().read(cx).is_collapsed(ix))
    };
    let next = |cx: &mut VisualTestContext| click("icon-button:commit-diff-next", cx);

    // A hunk gone to before the files above it land stays in view as they
    // do: d.rs's read lands first, then b.rs's 201 rows above it.
    assert_eq!(held(cx), super::super::commit_diff::READS);
    cx.update(|_, cx| Workbench::release_git_work(3, cx));
    draw(cx);
    click_file(&workbench, "change", "M d.rs", cx);
    assert_eq!(current(cx), Some((d, None)));
    view.update(cx, |view, cx| view.go_to_change(true, cx));
    draw(cx);
    assert_eq!(current(cx), Some((d, Some(0))));
    let before = view.read_with(cx, |view, _| view.hunk_row(d, 0)).unwrap();
    for _ in 0..3 {
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
        draw(cx);
        let row = view.read_with(cx, |view, _| view.hunk_row(d, 0)).unwrap();
        assert!(shown(cx).contains(&row), "{row} in {:?}", shown(cx));
    }
    let after = view.read_with(cx, |view, _| view.hunk_row(d, 0)).unwrap();
    assert!(
        after > before + 200,
        "b.rs landed above it: {before} to {after}"
    );

    let waiting = |ix: usize, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| {
            let texts = &view.diff().read(cx).sections()[ix].texts;
            matches!(texts, Texts::Unread | Texts::Loading(_))
        })
    };

    // A scroll by hand while navigation waits for a section's texts: they
    // land without taking the view back.
    assert!(waiting(e, cx), "e.rs isn't read yet");
    next(cx);
    assert_eq!(current(cx), Some((e, None)), "waiting for e.rs's texts");
    assert!(shown(cx).start > 0);
    wheel(
        view_row(&view, 0, true, cx),
        point(px(0.), ROW_HEIGHT * 1000.),
        cx,
    );
    release_all(cx);
    assert!(!waiting(e, cx));
    assert_eq!(shown(cx).start, 0, "the view stays where it was scrolled");
    assert_eq!(current(cx), Some((e, None)), "nor is e.rs's hunk gone to");

    // Collapsing the sections while navigation waits for one's texts keeps
    // it collapsed when they land: f.rs, after the files are read again.
    let f = e + 1;
    view.update(cx, |view, cx| {
        view.diff().update(cx, |diff, cx| diff.reload(cx))
    });
    draw(cx);
    assert_eq!(held(cx), 1, "the listing is held");
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert!(waiting(f, cx));
    next(cx);
    assert_eq!(current(cx), Some((f, None)), "waiting for f.rs's texts");
    // The reads ahead of its landing, then its own under way.
    for _ in 0..super::super::commit_diff::READS {
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
    }
    let loading = view.read_with(cx, |view, cx| {
        matches!(view.diff().read(cx).sections()[f].texts, Texts::Loading(_))
    });
    assert!(loading, "f.rs's read is under way");
    click("icon-button:commit-diff-collapse-all", cx);
    release_all(cx);
    assert!(!waiting(f, cx));
    assert!(collapsed(f, cx), "the collapse stands");
    assert_eq!(current(cx), Some((f, None)));
    click("icon-button:commit-diff-expand-all", cx);
    release_all(cx);

    // A scroll by hand lets the target go: the rows changing don't take
    // the view back to it.
    next(cx);
    release_all(cx);
    let top = shown(cx).start;
    assert!(top > 0);
    wheel(
        view_row(&view, 0, true, cx),
        point(px(0.), ROW_HEIGHT * 1000.),
        cx,
    );
    view.update(cx, |view, cx| view.relayout(cx));
    draw(cx);
    assert_eq!(shown(cx).start, 0, "scrolled up from {top}");

    // A file clicked in the Graph before the view has its files is gone
    // to once they're read, expanded, as the sections above it land.
    open_changes(&workbench, "base", cx);
    let based = active_view(&workbench, cx);
    assert_ne!(based, view);
    assert_eq!(held(cx), 1, "its listing is held");
    click_file(&workbench, "base", "A g.rs", cx);
    assert_eq!(active_view(&workbench, cx), based);
    release_all(cx);
    let g = (paths(&based, cx).iter().position(|path| path == "g.rs")).unwrap();
    let header = based.read_with(cx, |view, _| view.header_row(g)).unwrap();
    let shown = based.read_with(cx, |view, _| view.viewport());
    assert!(shown.contains(&header), "{header} in {shown:?}");
    assert!(!based.read_with(cx, |view, cx| view.diff().read(cx).is_collapsed(g)));
    assert_eq!(
        based.read_with(cx, |view, _| view.current_change()),
        Some((g, None))
    );
}

#[gpui::test]
fn a_commit_of_another_worktree_is_its_own_tab(cx: &mut TestAppContext) {
    let (dir, [_, change]) = fixture();
    let root = dir.path().canonicalize().unwrap();
    let other = tempfile::tempdir().unwrap();
    let other = other.path().canonicalize().unwrap().join("other");
    git(
        &root,
        &["worktree", "add", "-q", other.to_str().unwrap(), "HEAD"],
    );
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;

    // The same commit, opened from the other worktree first.
    let repo = Arc::new(fe_git::Repo::discover(&other).unwrap().unwrap());
    let entry = panel(&workbench, cx).read_with(cx, |panel, _| {
        (panel.commits.iter().find(|entry| entry.commit.id == change)).cloned()
    });
    let entry = entry.expect("the Graph lists the commit");
    let elsewhere = workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_commit_diff(repo, entry, window, cx)
    });
    draw(cx);
    assert_eq!(tab_count(&workbench, cx), 1);

    // From this one's Graph: its own tab, which a file click scrolls.
    open_changes(&workbench, "change", cx);
    let here = active_view(&workbench, cx);
    assert_ne!(here, elsewhere);
    assert_eq!(tab_count(&workbench, cx), 2);
    let workdir = |view: &Entity<CommitDiffView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, cx| view.diff().read(cx).workdir().to_owned())
    };
    assert_eq!(workdir(&here, cx), root);
    assert_eq!(workdir(&elsewhere, cx), other);
    click_file(&workbench, "change", "M g.rs", cx);
    assert_eq!(active_view(&workbench, cx), here);
    let g = (paths(&here, cx).iter().position(|path| path == "g.rs")).unwrap();
    let header = here.read_with(cx, |view, _| view.header_row(g)).unwrap();
    assert!(
        here.read_with(cx, |view, _| view.viewport())
            .contains(&header)
    );
}

#[gpui::test]
fn sideways_scroll_follows_the_geometry(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let (workbench, mut cx) = graph(cx, &dir);
    let cx = &mut cx;
    let full = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(gpui::size(full.width / 2., full.height));
    draw(cx);
    open_held(&workbench, cx);
    let view = active_view(&workbench, cx);
    let offset = |cx: &mut VisualTestContext| cells(&view, 5, cx)[0].as_ref().unwrap().offset;

    // As far right as a narrow window lets a.rs's long row go.
    wheel(
        view_row(&view, 5, true, cx),
        point(px(-100_000.), px(0.)),
        cx,
    );
    let narrow = offset(cx);
    assert!(narrow > px(0.));

    // A wider window shows more of it: the scroll draws less far, and the
    // next turn back moves from there at once.
    cx.simulate_resize(full);
    draw(cx);
    let wide = offset(cx);
    assert!(wide < narrow, "{wide:?} < {narrow:?}");
    wheel(view_row(&view, 5, true, cx), point(px(20.), px(0.)), cx);
    assert_eq!(offset(cx), wide - px(20.));

    // Measured again in another font, at the same size.
    let a = view.read_with(cx, |view, cx| {
        view.diff().read(cx).sections()[0].path.clone()
    });
    let theme = view.read_with(cx, |view, cx| view.theme(cx));
    let measured = view.read_with(cx, |view, _| view.measured(&a));
    assert_eq!(measured, Some((theme.font_family.clone(), theme.font_size)));
    let mut other = theme.clone();
    other.font_family = "Some Other Mono".into();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let diff = view.diff().clone();
            let diff = diff.read(cx);
            let Texts::Read(diffed) = &diff.sections()[0].texts else {
                panic!("a.rs is read");
            };
            view.widest(&a, diffed, &other, window);
        })
    });
    let measured = view.read_with(cx, |view, _| view.measured(&a));
    assert_eq!(measured, Some((other.font_family.clone(), other.font_size)));
}
