//! Git status in the project tree: a changed file's name in its status
//! letter's colour with the letter at the row's end, and a folder holding
//! changes in the colour of the highest-precedence status beneath it with a
//! dot, all from the Changes panel's list.

use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_git::{ChangeKind, FileStatus};
use fe_gpui::{EditorSettings, EditorTheme};
use fe_project::NodeId;
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};

use super::DrawnStatus;
use crate::{DockPanel, GitPanel, Launch, ProjectPanel, Workbench, git_panel};

thread_local! {
    /// Whether panels made from now on list ignored entries too.
    pub(crate) static SHOW_IGNORED: Cell<bool> = const { Cell::new(false) };
}

fn write(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{relative}\n")).unwrap();
}

/// A temporary folder holding `files`.
fn folder(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in files {
        write(dir.path(), file);
    }
    dir
}

/// A workbench rooted at `root` in a headless window, drawn once its tree
/// has opened.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
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
    draw(cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    workbench.read_with(cx, |workbench, _| workbench.panel().clone())
}

fn changes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    })
}

/// Lists `statuses`, by path from `root`, in the Changes panel, then draws.
fn set_statuses(
    workbench: &Entity<Workbench>,
    root: &Path,
    statuses: &[(&str, ChangeKind)],
    cx: &mut VisualTestContext,
) {
    let statuses = (statuses.iter())
        .map(|(path, kind)| FileStatus {
            path: root.join(path),
            kind: *kind,
        })
        .collect();
    let changes = changes(workbench, cx);
    changes.update(cx, |changes, cx| changes.set_statuses(statuses, cx));
    draw(cx);
}

/// What a row with no status is built with.
const PLAIN: DrawnStatus = (None, None);

/// What row `name` was built with in the last frame.
fn drawn(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) -> DrawnStatus {
    let drawn = panel.read_with(cx, |panel, _| panel.drawn_status());
    let row = drawn.into_iter().rev().find(|(drawn, _)| drawn == name);
    row.unwrap_or_else(|| panic!("{name} drawn")).1
}

/// What a row with status `kind` is built with: its badge, and label and
/// badge in the Changes panel's colour for its letter.
fn shown(
    badge: &'static str,
    kind: ChangeKind,
    panel: &Entity<ProjectPanel>,
    cx: &mut VisualTestContext,
) -> DrawnStatus {
    let color: Hsla = panel.read_with(cx, |panel, _| git_panel::letter_color(kind, &panel.theme));
    (Some(color), Some((badge.to_owned(), color)))
}

/// Whether row `name`'s badge was drawn.
fn badge_drawn(name: &str, cx: &mut VisualTestContext) -> bool {
    let selector: &'static str = format!("tree-badge:{name}").leak();
    cx.debug_bounds(selector).is_some()
}

/// Clicks directory `name`'s row, which expands it once listed.
fn expand(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| {
        let tree = panel.tree().expect("open");
        let id: NodeId = (tree.rows(0..tree.row_count()))
            .find(|row| row.name == name)
            .expect("a row")
            .id;
        panel.click(id, cx);
    });
    draw(cx);
}

#[gpui::test]
fn changed_files_show_their_letter_in_their_status_colour(cx: &mut TestAppContext) {
    SHOW_IGNORED.set(true);
    let dir = folder(&[
        "a.rs",
        "n.rs",
        "u.rs",
        "x.rs",
        "e.rs",
        ".gitignore",
        "i.log",
    ]);
    fs::write(dir.path().join(".gitignore"), "i.log\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    SHOW_IGNORED.set(false);
    let panel = panel(&workbench, cx);
    use ChangeKind::*;
    set_statuses(
        &workbench,
        dir.path(),
        &[
            ("a.rs", Modified),
            ("n.rs", Added),
            ("u.rs", Untracked),
            ("x.rs", Conflicted),
            // Tracked, though it matches the ignore file.
            ("i.log", Modified),
        ],
        cx,
    );

    for (name, badge, kind) in [
        ("a.rs", "M", Modified),
        ("n.rs", "A", Added),
        ("u.rs", "U", Untracked),
        ("x.rs", "C", Conflicted),
    ] {
        assert_eq!(drawn(&panel, name, cx), shown(badge, kind, &panel, cx));
        assert!(badge_drawn(name, cx), "{name}");
    }
    let ignored = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().unwrap();
        let mut rows = tree.rows(0..tree.row_count());
        rows.find(|row| row.name == "i.log").map(|row| row.ignored)
    });
    assert_eq!(ignored, Some(true), "the ignored file has a row");
    // Neither the unchanged file nor the ignored one shows a status.
    for name in ["e.rs", "i.log"] {
        assert_eq!(drawn(&panel, name, cx), PLAIN, "{name}");
        assert!(!badge_drawn(name, cx), "{name}");
    }
}

#[gpui::test]
fn folders_show_the_highest_status_beneath_them(cx: &mut TestAppContext) {
    let dir = folder(&[
        "c/d/e.rs", "m/a.rs", "m/x.rs", "h/a.rs", "h/x.rs", "p/q/r.rs", "p/q/t.rs", "p/s.rs",
        "g/k.rs", "plain.rs",
    ]);
    let (workbench, cx) = open(cx, dir.path());
    let panel = panel(&workbench, cx);
    use ChangeKind::*;
    set_statuses(
        &workbench,
        dir.path(),
        &[
            ("c/d/e.rs", Untracked),
            ("m/a.rs", Modified),
            ("m/x.rs", Conflicted),
            // The highest first, this time.
            ("h/a.rs", Conflicted),
            ("h/x.rs", Modified),
            ("p/q/r.rs", Modified),
            ("p/q/t.rs", Untracked),
            ("p/s.rs", Conflicted),
            // Deleted, so it has no row.
            ("g/gone.rs", Deleted),
        ],
        cx,
    );

    assert_eq!(drawn(&panel, "c", cx), shown("•", Untracked, &panel, cx));
    assert_eq!(drawn(&panel, "m", cx), shown("•", Conflicted, &panel, cx));
    assert_eq!(drawn(&panel, "h", cx), shown("•", Conflicted, &panel, cx));
    assert_eq!(drawn(&panel, "p", cx), shown("•", Conflicted, &panel, cx));
    assert_eq!(drawn(&panel, "g", cx), shown("•", Deleted, &panel, cx));
    assert_eq!(drawn(&panel, "plain.rs", cx), PLAIN);
    assert!(badge_drawn("c", cx));

    expand(&panel, "c", cx);
    assert_eq!(drawn(&panel, "c", cx), shown("•", Untracked, &panel, cx));
    assert_eq!(drawn(&panel, "d", cx), shown("•", Untracked, &panel, cx));
    expand(&panel, "d", cx);
    assert_eq!(drawn(&panel, "e.rs", cx), shown("U", Untracked, &panel, cx));
    expand(&panel, "p", cx);
    assert_eq!(drawn(&panel, "q", cx), shown("•", Modified, &panel, cx));
    expand(&panel, "m", cx);
    assert_eq!(drawn(&panel, "a.rs", cx), shown("M", Modified, &panel, cx));
    assert_eq!(
        drawn(&panel, "x.rs", cx),
        shown("C", Conflicted, &panel, cx)
    );

    set_statuses(&workbench, dir.path(), &[], cx);
    let drawn = panel.read_with(cx, |panel, _| panel.drawn_status());
    assert!(drawn.len() >= 13, "{drawn:?}");
    assert!(
        drawn.iter().all(|(_, status)| *status == PLAIN),
        "{drawn:?}"
    );
    for name in ["c", "d", "e.rs", "m", "a.rs", "x.rs", "g", "p", "q"] {
        assert!(!badge_drawn(name, cx), "{name}");
    }
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
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
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[gpui::test]
fn a_repository_shows_in_the_tree_without_the_changes_panel(cx: &mut TestAppContext) {
    let dir = folder(&["a.rs", "b.rs"]);
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    fs::write(dir.path().join("a.rs"), "changed\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let panel = panel(&workbench, cx);

    assert_eq!(
        drawn(&panel, "a.rs", cx),
        shown("M", ChangeKind::Modified, &panel, cx)
    );
    assert_eq!(drawn(&panel, "b.rs", cx), PLAIN);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let active = dock.read_with(cx, |dock, _| dock.active());
    assert_eq!(active, DockPanel::Explorer, "Changes never showed");
    let changes = changes(&workbench, cx);
    let runs = changes.read_with(cx, |changes, _| changes.runs_started());
    assert_eq!(runs, 1, "the tree's opening read the status");
    let paths: Vec<PathBuf> = changes.read_with(cx, |changes, _| changes.paths());
    assert_eq!(paths, [dir.path().join("a.rs")]);
}

#[gpui::test]
fn the_tree_opening_reads_nothing_more_once_something_has(cx: &mut TestAppContext) {
    let first = folder(&["a.rs"]);
    let dir = folder(&["a.rs"]);
    git(dir.path(), &["init", "-q"]);
    let (workbench, cx) = open(cx, first.path());
    let root = dir.path().to_owned();
    let changes = workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(root, window, cx);
        // Read before the new tree opens, as for an editor opened first.
        let changes = workbench.dock().read(cx).changes().clone();
        changes.update(cx, |changes, cx| changes.refresh(cx));
        changes
    });
    draw(cx);
    let panel = panel(&workbench, cx);
    assert!(
        panel.read_with(cx, |panel, _| panel.tree().is_some()),
        "opened"
    );
    let runs = changes.read_with(cx, |changes, _| changes.runs_started());
    assert_eq!(runs, 1, "the opening didn't read it again");
    assert_eq!(
        drawn(&panel, "a.rs", cx),
        shown("U", ChangeKind::Untracked, &panel, cx)
    );
}
