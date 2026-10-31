//! A Git Log commit's file rows: each file's icon from the installed icon
//! set, its name and dimmed directory, then at the right edge its line
//! counts and its change's coloured letter, on repositories built with the
//! `git` CLI in temporary directories, read off what the rows drew.

use std::{fs, path::Path, process::Command, sync::Arc};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_ui::{IconRef, IconSet};
use gpui::{
    Bounds, Entity, Focusable as _, Hsla, Pixels, TestAppContext, VisualTestContext, px, rgba,
};
use kiln_shell::{DockView as _, dock::MIN_DOCK_WIDTH};

use super::{CellSeen, Counts, GitLogPanel, muted};
use crate::{Launch, Workbench};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "commit.gpgsign=false"])
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

fn write(root: &Path, file: &str, contents: impl AsRef<[u8]>) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn commit(root: &Path, summary: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", summary]);
}

/// `base`, then `change`, the newest: `a.rs` edited (three lines added,
/// one removed), `src/new.rs` added, `gone.rs` deleted, `old.rs` renamed
/// `moved.rs` unchanged and the binary `bin.dat` changed.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, "a.rs", "1\n2\n3\n4\n");
    write(root, "old.rs", "kept\nas\nit\nwas\n");
    write(root, "gone.rs", "bye\n");
    write(root, "bin.dat", b"\0one\0");
    commit(root, "base");
    write(root, "a.rs", "1\nX\n3\n4\n5\n6\n");
    write(root, "src/new.rs", "new\n");
    fs::remove_file(root.join("gone.rs")).unwrap();
    git(root, &["mv", "old.rs", "moved.rs"]);
    write(root, "bin.dat", b"\0two\0");
    commit(root, "change");
    dir
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    frame(cx);
    cx.run_until_parked();
}

/// Draws a frame, letting nothing land first.
fn frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A window of a workbench rooted at `root` with the Git Log panel shown
/// and focused, and the panel.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<GitLogPanel>, VisualTestContext) {
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
    let run = RunCommand(
        Invocation::of("git.log")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        (workbench.dock().read(cx).git_log().cloned()).expect("the Git Log panel was made")
    });
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    (panel, vcx)
}

/// Expands the newest commit and draws.
fn expanded(cx: &mut TestAppContext, root: &Path) -> (Entity<GitLogPanel>, VisualTestContext) {
    let (panel, mut cx) = window(cx, root);
    panel.update_in(&mut cx, |panel, window, cx| panel.open_row(0, window, cx));
    draw(&mut cx);
    (panel, cx)
}

/// The drawn cells of each file row, keyed by the file's name.
fn rows(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> Vec<(String, Vec<Cell>)> {
    let seen = panel.read_with(cx, |panel, _| panel.rows_seen());
    let seen = seen.borrow();
    let rows = seen.cells.values().map(|cells| {
        let cells: Vec<_> = cells.iter().map(Cell::from).collect();
        (cells[1].text.clone(), cells)
    });
    rows.collect()
}

/// The cells of the row of the file called `name`.
fn row(rows: &[(String, Vec<Cell>)], name: &str) -> Vec<Cell> {
    let found = rows.iter().find(|(row, _)| row == name);
    (found.map(|(_, cells)| cells.clone())).unwrap_or_else(|| panic!("no row for {name}"))
}

/// What a test reads of a drawn cell.
#[derive(Clone, Debug)]
struct Cell {
    text: String,
    color: Option<Hsla>,
    flex_none: bool,
    right: bool,
    bounds: Bounds<Pixels>,
}

impl From<&CellSeen> for Cell {
    fn from(cell: &CellSeen) -> Self {
        Cell {
            text: cell.text.clone(),
            color: cell.color,
            flex_none: cell.flex_none,
            right: cell.right,
            bounds: cell.bounds.expect("the cell was drawn"),
        }
    }
}

fn texts(cells: &[Cell]) -> Vec<&str> {
    cells.iter().map(|cell| cell.text.as_str()).collect()
}

/// The editor palette's colours a file row takes: modified, added,
/// deleted and renamed.
fn palette(cx: &mut VisualTestContext) -> [Hsla; 4] {
    let palette = cx.update(|_, cx| fe_gpui::editor_palette(cx));
    let editor = &palette.editor;
    let hsla = |color: fe_config::theme::Color| rgba(color.to_rgba_u32()).into();
    [
        editor.warning,
        editor.change_added,
        editor.error,
        editor.information,
    ]
    .map(hsla)
}

/// The list's bounds.
fn list(panel: &Entity<GitLogPanel>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    panel.read_with(cx, |panel, _| panel.scroll.0.borrow().base_handle.bounds())
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
fn icon_name_dir(cx: &mut TestAppContext) {
    let dir = fixture();
    let (panel, mut cx) = expanded(cx, dir.path());
    let cx = &mut cx;
    let muted_text = panel.read_with(cx, |panel, _| panel.muted_text());

    // Lucide, the set installed by default.
    let all = rows(&panel, cx);
    assert_eq!(all.len(), 5, "every file has a row: {all:?}");
    let new = row(&all, "new.rs");
    assert_eq!(new[0].text, fe_ui::file_icon_path("new.rs").to_string());
    assert_eq!(
        new[0].text,
        fe_ui::LucideIcons.file("new.rs").path().to_string()
    );
    assert_eq!(new[1].text, "new.rs");
    assert_eq!(new[1].color, None, "the name keeps the row's colour");
    assert_eq!(new[2].text, "src");
    assert_eq!(new[2].color, Some(muted_text), "the directory is dimmed");
    let [icon, name, directory] = [0, 1, 2].map(|ix| new[ix].bounds);
    assert!(icon.size.width > px(0.), "the icon is drawn");
    assert!(icon.right() <= name.left(), "the icon comes first");
    assert!(
        name.right() <= directory.left(),
        "then the name, then its directory"
    );

    // A set installed later answers every row, through the seam.
    cx.update(|_, cx| fe_ui::set_icon_set(Arc::new(Stub), cx));
    draw(cx);
    let all = rows(&panel, cx);
    assert_eq!(row(&all, "new.rs")[0].text, "stub/rust.svg");
    assert_eq!(row(&all, "a.rs")[0].text, "stub/rust.svg");
    assert_eq!(row(&all, "bin.dat")[0].text, "stub/other.svg");
    cx.update(|_, cx| fe_ui::reset_icon_set(cx));
    draw(cx);
    let all = rows(&panel, cx);
    assert_eq!(
        row(&all, "bin.dat")[0].text,
        fe_ui::LucideIcons.file("bin.dat").path().to_string()
    );
}

#[gpui::test]
fn letter_right(cx: &mut TestAppContext) {
    let dir = fixture();
    let (panel, mut cx) = expanded(cx, dir.path());
    let cx = &mut cx;
    let [modified, added, deleted, renamed] = palette(cx);
    let colors = [modified, added, deleted, renamed];
    for (ix, color) in colors.iter().enumerate() {
        assert!(
            !colors[ix + 1..].contains(color),
            "four colours: {colors:?}"
        );
    }
    let list = list(&panel, cx);
    let all = rows(&panel, cx);
    let expected = [
        ("a.rs", "M", modified),
        ("bin.dat", "M", modified),
        ("new.rs", "A", added),
        ("gone.rs", "D", deleted),
        ("moved.rs", "R", renamed),
    ];
    for (name, letter, color) in expected {
        let cells = row(&all, name);
        let last = cells.last().unwrap();
        assert_eq!(
            last.text,
            letter,
            "{name}'s letter is last: {:?}",
            texts(&cells)
        );
        assert_eq!(last.color, Some(color), "{name}'s letter's colour");
        assert!(last.flex_none, "{name}'s letter neither grows nor shrinks");
        assert!(last.right, "{name}'s letter is aligned right");
        // At the right edge, inside the row's padding.
        let edge = list.right() - last.bounds.right();
        assert!(
            edge >= px(0.) && edge <= px(16.),
            "{name}'s letter is {edge:?} from the edge"
        );
        for cell in &cells[..cells.len() - 1] {
            assert!(
                cell.bounds.right() <= last.bounds.left(),
                "{name}: {cell:?} before the letter"
            );
        }
        // Nothing on the left is a letter any more: the icon comes first.
        assert!(
            cells[0].text.ends_with(".svg"),
            "{name} starts with its icon"
        );
        assert_eq!(cells[1].text, name);
    }
    // The rename's old path follows its directory.
    assert_eq!(row(&all, "moved.rs")[2].text, "← old.rs");
}

#[gpui::test]
fn counts(cx: &mut TestAppContext) {
    let dir = fixture();
    let (panel, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    let [_, added, deleted, _] = palette(cx);

    // Expanded, with its files landed but not its counts: none show, and
    // they weren't read in the expansion itself.
    panel.update(cx, |panel, cx| panel.toggle(0, cx));
    let (repo, id, read, request) = panel.read_with(cx, |panel, _| {
        let repo = panel.repo.clone().unwrap();
        (
            repo,
            panel.commits[0].commit.id.clone(),
            panel.read,
            panel.requests,
        )
    });
    let files = repo.commit_files(&id).map_err(|error| error.to_string());
    panel.update(cx, |panel, cx| {
        panel.land_files(read, (0, request), files, cx)
    });
    let loading = panel.read_with(cx, |panel, _| panel.counts.get(&0).cloned());
    assert_eq!(loading, Some(Counts::Loading(request)));
    frame(cx);
    let all = rows(&panel, cx);
    assert_eq!(all.len(), 5, "the files are listed: {all:?}");
    assert_eq!(texts(&row(&all, "a.rs"))[1..], ["a.rs", "", "M"]);

    // Once they land.
    draw(cx);
    let all = rows(&panel, cx);
    let a = row(&all, "a.rs");
    assert_eq!(texts(&a)[1..], ["a.rs", "", "+3", "-1", "M"]);
    assert_eq!(a[3].color, Some(muted(added)), "lines added, muted green");
    assert_eq!(a[4].color, Some(muted(deleted)), "lines removed, muted red");
    assert_ne!(muted(added), added, "muted");
    assert!(a[2].bounds.right() <= a[3].bounds.left());
    assert!(a[3].bounds.right() <= a[4].bounds.left());
    assert!(
        a[4].bounds.right() <= a[5].bounds.left(),
        "before the letter"
    );
    // A side with no lines shows nothing.
    assert_eq!(
        texts(&row(&all, "new.rs"))[1..],
        ["new.rs", "src", "+1", "A"]
    );
    assert_eq!(
        texts(&row(&all, "gone.rs"))[1..],
        ["gone.rs", "", "-1", "D"]
    );
    assert_eq!(
        texts(&row(&all, "moved.rs"))[1..],
        ["moved.rs", "← old.rs", "R"]
    );
    // A binary file shows none.
    assert_eq!(texts(&row(&all, "bin.dat"))[1..], ["bin.dat", "", "M"]);

    // Collapsed and expanded again, they're read again.
    panel.update(cx, |panel, cx| panel.toggle(0, cx));
    assert_eq!(panel.read_with(cx, |panel, _| panel.counts.len()), 0);
    panel.update(cx, |panel, cx| panel.toggle(0, cx));
    draw(cx);
    assert_eq!(
        texts(&row(&rows(&panel, cx), "a.rs"))[3..],
        ["+3", "-1", "M"]
    );
}

#[gpui::test]
fn narrow(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, "base.rs", "base\n");
    let lines = |prefix: &str| {
        (0..10_000)
            .map(|n| format!("{prefix}{n}\n"))
            .collect::<String>()
    };
    write(root, "big.rs", lines("a"));
    commit_at(root, "base", 10);
    write(root, "big.rs", lines("b"));
    let long_dir = format!("src/{}", "d".repeat(80));
    let long_name = format!("{}.rs", "n".repeat(120));
    write(root, &format!("{long_dir}/{long_name}"), "long\n");
    write(root, "short.rs", "short\n");
    commit_at(root, "long", 20);
    let (panel, mut cx) = expanded(cx, root);
    let cx = &mut cx;
    let list = list(&panel, cx);
    let all = rows(&panel, cx);
    let long = row(&all, &long_name);
    let short = row(&all, "short.rs");
    assert_eq!(texts(&long)[2..], [long_dir.as_str(), "+1", "A"]);
    let [name, directory, count, letter] = [1, 2, 3, 4].map(|ix| long[ix].bounds);
    // The name is cut short, keeping room, before its directory.
    assert!(name.right() <= list.right(), "the name is cut to the row");
    assert!(name.size.width > px(40.), "the name keeps room");
    assert!(
        name.right() <= directory.left(),
        "the name ellipsizes before the directory"
    );
    // The directory before the counts and the letter, which are whole.
    assert!(
        directory.right() <= count.left(),
        "the directory ends before the counts"
    );
    assert!(
        count.right() <= letter.left(),
        "and the counts before the letter"
    );
    assert!(
        letter.right() <= list.right(),
        "the letter is inside the row"
    );
    assert_eq!(
        letter.size, short[4].bounds.size,
        "the letter is never clipped"
    );
    assert_eq!(count.size, short[3].bounds.size, "nor the counts");
    let big = row(&all, "big.rs");
    assert_eq!(texts(&big)[3..], ["+10000", "-10000", "M"]);

    // At the dock's least width the counts give way too, the letter never.
    let workbench = panel.read_with(cx, |panel, _| panel.workbench.upgrade().unwrap());
    workbench.update(cx, |workbench, cx| {
        (workbench.dock()).update(cx, |dock, cx| {
            dock.dock_mut().set_size(MIN_DOCK_WIDTH);
            cx.notify();
        })
    });
    draw(cx);
    let narrow = self::list(&panel, cx);
    assert!(narrow.size.width <= MIN_DOCK_WIDTH, "{narrow:?}");
    let all_narrow = rows(&panel, cx);
    for (name, wide) in [
        (long_name.as_str(), &long),
        ("big.rs", &big),
        ("short.rs", &short),
    ] {
        let cells = row(&all_narrow, name);
        let (letter, before) = cells.split_last().unwrap();
        let whole = wide.last().unwrap();
        assert_eq!(letter.text, whole.text);
        assert_eq!(
            letter.bounds.size, whole.bounds.size,
            "{name}'s letter is whole"
        );
        assert!(
            letter.bounds.right() <= narrow.right(),
            "{name}'s letter is in the row"
        );
        for cell in before {
            assert!(
                cell.bounds.right() <= letter.bounds.left(),
                "{name}: {cell:?} ends before the letter"
            );
        }
    }
    let big = row(&all_narrow, "big.rs");
    let width = |cells: &[Cell], ix: usize| cells[ix].bounds.size.width;
    assert_eq!(width(&big, 1), px(0.), "big.rs's name went first");
    assert!(
        width(&big, 3) + width(&big, 4) > px(0.),
        "then the counts shrank, still shown"
    );

    // Eleven lanes through the commit, wider than the row: the rail and
    // the cells clip before the letter, which is still whole.
    for branch in 0..10 {
        git(
            root,
            &["checkout", "-q", "-b", &format!("b{branch}"), "HEAD~1"],
        );
        write(root, &format!("b{branch}.rs"), "b\n");
        commit_at(root, &format!("branch {branch}"), 100 + branch);
        git(root, &["checkout", "-q", "main"]);
    }
    let run = RunCommand(
        Invocation::of("git.log_all")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let lines = panel.read_with(cx, |panel, _| panel.lines());
    let at = (lines.iter().position(|line| line.ends_with("  long")))
        .unwrap_or_else(|| panic!("no row for long: {lines:?}"));
    panel.update_in(cx, |panel, window, cx| panel.open_row(at, window, cx));
    // Only the rows drawn now.
    let seen = panel.read_with(cx, |panel, _| panel.rows_seen());
    seen.borrow_mut().cells.clear();
    draw(cx);
    let commit = panel.read_with(cx, |panel, _| panel.lines[at].commit());
    let rail = panel.read_with(cx, |panel, _| panel.rail(commit).len());
    assert_eq!(rail, 11, "every lane runs through the commit's rows");
    let narrow = self::list(&panel, cx);
    let all_lanes = rows(&panel, cx);
    for (name, wide) in [
        (long_name.as_str(), &long),
        ("big.rs", &big),
        ("short.rs", &short),
    ] {
        let ix = (seen.borrow().cells.iter())
            .find(|(_, cells)| cells[1].text == name)
            .map(|(ix, _)| *ix)
            .unwrap();
        assert!(
            ix > at + 1 && ix <= at + 4,
            "{name} is under the commit, at {ix}"
        );
        let cells = row(&all_lanes, name);
        let (letter, whole) = (cells.last().unwrap(), wide.last().unwrap());
        assert_eq!(
            letter.bounds.size, whole.bounds.size,
            "{name}'s letter is whole"
        );
        assert!(
            letter.bounds.right() <= narrow.right(),
            "{name}'s letter is in the row"
        );
        let before = seen.borrow().before[&ix];
        assert!(
            before.right() <= letter.bounds.left(),
            "{name}: the rest clips before the letter"
        );
    }
}

/// Commits everything as `summary`, made `seconds` after the epoch.
fn commit_at(root: &Path, summary: &str, seconds: u64) {
    git(root, &["add", "-A"]);
    let date = format!("@{seconds} +0000");
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "commit.gpgsign=false", "commit", "-q", "-m", summary])
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
