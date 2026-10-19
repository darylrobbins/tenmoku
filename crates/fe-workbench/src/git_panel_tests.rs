//! The Changes panel in a headless workbench, on repositories built with
//! the `git` CLI in temporary directories.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_git::{ChangeKind, FileStatus};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{Action, GitPanel, Group, ShownRow};
use crate::{DockPanel, Launch, Workbench};

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

fn write(root: &Path, rela: &str, contents: &str) {
    let path = root.join(rela);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A repository with `files` committed.
fn repo(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for file in files {
        write(root, file, &format!("{file}\n"));
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    dir
}

/// A repository whose `a.rs` is modified, `b.rs` deleted and `c/d.rs`
/// untracked, and whose `e.rs` is as committed.
fn changed() -> tempfile::TempDir {
    let dir = repo(&["a.rs", "b.rs", "e.rs"]);
    write(dir.path(), "a.rs", "changed\n");
    fs::remove_file(dir.path().join("b.rs")).unwrap();
    write(dir.path(), "c/d.rs", "new\n");
    dir
}

/// A workbench for `root`, in a headless window, focused as the app
/// focuses it.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: Option<&Path>,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: root.map(Path::to_owned),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn changes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    })
}

fn rows(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Vec<(&'static str, String)> {
    panel.read_with(cx, |panel, _| {
        (panel.shown_rows().into_iter())
            .map(|row| {
                let path = match row.directory.as_str() {
                    "" => row.name,
                    directory => format!("{directory}/{}", row.name),
                };
                (row.letter, path)
            })
            .collect()
    })
}

fn message(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.message_text())
}

/// The active pane's tabs' paths.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane().read(cx).tabs().iter())
            .filter_map(|tab| tab.path.clone())
            .collect()
    })
}

/// The active pane's tabs' titles.
fn titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane().read(cx).tabs().iter())
            .map(|tab| tab.item().tab_title(cx).to_string())
            .collect()
    })
}

/// Clicks row `ix`'s "Open File" button, shown with the pointer over the
/// row.
fn click_open_file(panel: &Entity<GitPanel>, ix: usize, cx: &mut VisualTestContext) {
    let row = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_mouse_move(row.center(), None, Modifiers::none());
    draw(cx);
    let selector = panel.read_with(cx, |panel, _| {
        panel.button_selector(Some(ix), Group::Changes, Action::OpenFile)
    });
    let bounds = cx.debug_bounds(selector.leak()).expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn click(panel: &Entity<GitPanel>, ix: usize, cx: &mut VisualTestContext) {
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn focus(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
}

fn runs(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> (usize, (usize, usize)) {
    panel.read_with(cx, |panel, _| (panel.runs_started(), panel.runs_ran()))
}

#[gpui::test]
fn show_changes_lists_the_changed_files_sorted_with_their_status(cx: &mut TestAppContext) {
    let dir = changed();
    let (workbench, cx) = open(cx, Some(dir.path()));
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let panel = changes(&workbench, cx);
    // The tree's opening read the status before the panel shows.
    assert_eq!(runs(&panel, cx).0, 1);

    dispatch("git.show_changes", cx);
    assert_eq!(
        dock.read_with(cx, |dock, _| (
            dock.is_visible(),
            dock.active(),
            dock.title()
        )),
        (true, DockPanel::Changes, "Changes".into())
    );
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    assert_eq!(
        rows(&panel, cx),
        [
            ("M", "a.rs".to_owned()),
            ("D", "b.rs".to_owned()),
            ("U", "c/d.rs".to_owned()),
        ]
    );
    let shown = panel.read_with(cx, |panel, _| panel.shown_rows());
    assert_eq!(
        shown[2],
        ShownRow {
            letter: "U",
            name: "d.rs".into(),
            directory: "c".into(),
            dimmed: false,
        }
    );
    assert!(shown[1].dimmed, "the deleted row is dimmed");
    assert_eq!(message(&panel, cx), None);

    // Its rail button follows Search's.
    let icons = dock.read_with(cx, |dock, _| dock.rail_icons());
    assert_eq!(
        icons,
        [
            Icon::Files,
            Icon::Search,
            Icon::GitBranch,
            crate::dock::panel_icon(DockPanel::GitLog),
            Icon::FileTerminal
        ]
    );
    let (search, changes) = dock.read_with(cx, |dock, _| {
        let bounds = |panel| dock.rail_button_bounds(panel).expect("drawn");
        (bounds(DockPanel::Search), bounds(DockPanel::Changes))
    });
    assert!(changes.top() >= search.bottom(), "{search:?} {changes:?}");
    assert_eq!(DockPanel::ALL[2], DockPanel::Changes);
}

#[gpui::test]
fn clicking_a_row_opens_its_diff_and_its_button_its_file(cx: &mut TestAppContext) {
    let dir = changed();
    let (workbench, cx) = open(cx, Some(dir.path()));
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);
    assert!(tabs(&workbench, cx).is_empty());

    click(&panel, 0, cx);
    assert_eq!(titles(&workbench, cx), ["a.rs (Index ↔ Working Tree)"]);

    // A deleted file's compares the index with nothing.
    click(&panel, 1, cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected()), Some(1));
    let both = ["a.rs (Index ↔ Working Tree)", "b.rs (Index ↔ Working Tree)"];
    assert_eq!(titles(&workbench, cx), both);

    click_open_file(&panel, 0, cx);
    assert_eq!(tabs(&workbench, cx), [dir.path().join("a.rs")]);
    assert_eq!(titles(&workbench, cx).len(), 3);
}

#[gpui::test]
fn enter_opens_the_selected_rows_diff_and_alt_enter_its_file(cx: &mut TestAppContext) {
    let dir = changed();
    let (workbench, cx) = open(cx, Some(dir.path()));
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);

    // The first down selects the first row.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected()), Some(0));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(titles(&workbench, cx), ["a.rs (Index ↔ Working Tree)"]);

    focus(&panel, cx);
    cx.simulate_keystrokes("alt-enter");
    draw(cx);
    assert_eq!(tabs(&workbench, cx), [dir.path().join("a.rs")]);

    // A deleted file has none to open.
    focus(&panel, cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected()), Some(1));
    cx.simulate_keystrokes("alt-enter");
    draw(cx);
    assert_eq!(tabs(&workbench, cx), [dir.path().join("a.rs")]);
}

#[gpui::test]
fn a_root_outside_any_repository_is_not_a_git_repository(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.rs", "a\n");
    let (workbench, cx) = open(cx, Some(dir.path()));
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);
    assert_eq!(message(&panel, cx).as_deref(), Some("Not a git repository"));
    assert!(rows(&panel, cx).is_empty());
}

#[gpui::test]
fn no_folder_open_is_not_a_git_repository(cx: &mut TestAppContext) {
    let (workbench, cx) = open(cx, None);
    dispatch("git.show_changes", cx);
    dispatch("git.refresh", cx);
    let panel = changes(&workbench, cx);
    assert_eq!(message(&panel, cx).as_deref(), Some("Not a git repository"));
    assert_eq!(runs(&panel, cx).0, 0, "there's nothing to read");
}

#[gpui::test]
fn a_clean_repository_has_no_changes(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", "src/b.rs"]);
    let (workbench, cx) = open(cx, Some(dir.path()));
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);
    assert_eq!(message(&panel, cx).as_deref(), Some("No changes"));
    assert!(rows(&panel, cx).is_empty());
}

#[gpui::test]
fn only_files_under_the_root_are_listed(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", "sub/b.rs", "sub/deep/c.rs", "subway.rs"]);
    write(dir.path(), "a.rs", "changed\n");
    write(dir.path(), "subway.rs", "changed\n");
    write(dir.path(), "sub/b.rs", "changed\n");
    write(dir.path(), "sub/deep/c.rs", "changed\n");
    write(dir.path(), "top.rs", "new\n");
    let root = dir.path().join("sub");
    let (workbench, cx) = open(cx, Some(&root));
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);
    assert_eq!(
        rows(&panel, cx),
        [("M", "b.rs".to_owned()), ("M", "deep/c.rs".to_owned())]
    );
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.paths()),
        [root.join("b.rs"), root.join("deep/c.rs")]
    );
}

#[gpui::test]
fn a_refresh_while_status_runs_runs_once_more_off_the_main_thread(cx: &mut TestAppContext) {
    let dir = changed();
    let (workbench, cx) = open(cx, Some(dir.path()));
    let panel = changes(&workbench, cx);
    // The tree's opening read the status.
    assert_eq!(runs(&panel, cx), (1, (1, 1)));
    let listed = [
        ("M", "a.rs".to_owned()),
        ("D", "b.rs".to_owned()),
        ("U", "c/d.rs".to_owned()),
    ];
    panel.update(cx, |panel, _| panel.hold_runs());
    dispatch("git.show_changes", cx);
    dispatch("git.refresh", cx);
    // The second run has read the status, and waits to land.
    assert_eq!(runs(&panel, cx), (2, (2, 2)));
    assert!(panel.read_with(cx, |panel, _| panel.is_running()));
    assert_eq!(rows(&panel, cx), listed);

    write(dir.path(), "f.rs", "newer\n");
    dispatch("git.refresh", cx);
    dispatch("git.refresh", cx);
    assert_eq!(runs(&panel, cx).0, 2, "refreshes wait for the run");

    panel.update(cx, |panel, _| panel.release_runs());
    draw(cx);
    assert_eq!(
        runs(&panel, cx),
        (3, (3, 3)),
        "exactly one more, off the main thread"
    );
    assert!(!panel.read_with(cx, |panel, _| panel.is_running()));
    // The third run's result, which has the newer file.
    assert_eq!(
        rows(&panel, cx),
        [
            ("M", "a.rs".to_owned()),
            ("D", "b.rs".to_owned()),
            ("U", "c/d.rs".to_owned()),
            ("U", "f.rs".to_owned()),
        ]
    );

    // A refresh with none running runs at once.
    dispatch("git.refresh", cx);
    assert_eq!(runs(&panel, cx).0, 4);
}

#[gpui::test]
fn a_run_older_than_the_list_set_lands_unshown(cx: &mut TestAppContext) {
    let dir = changed();
    let (workbench, cx) = open(cx, Some(dir.path()));
    let panel = changes(&workbench, cx);
    panel.update(cx, |panel, _| panel.hold_runs());
    dispatch("git.show_changes", cx);
    // The tree's opening read the status already: this run waits to land.
    dispatch("git.refresh", cx);
    assert!(panel.read_with(cx, |panel, _| panel.is_running()));

    let root = dir.path().to_owned();
    let elsewhere = tempfile::tempdir().unwrap();
    panel.update(cx, |panel, cx| {
        let statuses = vec![
            FileStatus {
                path: root.join("z.rs"),
                kind: ChangeKind::Added,
            },
            FileStatus {
                path: root.join("e.rs"),
                kind: ChangeKind::Conflicted,
            },
            FileStatus {
                path: elsewhere.path().join("x.rs"),
                kind: ChangeKind::Modified,
            },
        ];
        panel.set_statuses(statuses, cx);
    });
    let set = [("C", "e.rs".to_owned()), ("A", "z.rs".to_owned())];
    assert_eq!(rows(&panel, cx), set);

    panel.update(cx, |panel, _| panel.release_runs());
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.is_running()));
    assert_eq!(rows(&panel, cx), set, "the older run's result isn't shown");
}

#[gpui::test]
fn opening_another_folder_reads_its_repository(cx: &mut TestAppContext) {
    let first = changed();
    let second = repo(&["x.rs"]);
    write(second.path(), "x.rs", "changed\n");
    let (workbench, cx) = open(cx, Some(first.path()));
    dispatch("git.show_changes", cx);
    let old = changes(&workbench, cx);
    assert_eq!(rows(&old, cx).len(), 3);

    let root = second.path().to_owned();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(root, window, cx)
    });
    draw(cx);
    let new = changes(&workbench, cx);
    assert_ne!(new, old);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Changes
    );
    assert_eq!(rows(&new, cx), [("M", "x.rs".to_owned())]);

    click(&new, 0, cx);
    assert_eq!(titles(&workbench, cx), ["x.rs (Index ↔ Working Tree)"]);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let diff = pane
            .active_item()?
            .downcast_ref::<fe_gpui::DiffView>()?
            .clone();
        let left = diff.read(cx).left().read(cx).model().text().to_string();
        Some(left)
    });
    assert_eq!(
        diff.as_deref(),
        Some("x.rs\n"),
        "the index's text in the new repository"
    );
}
