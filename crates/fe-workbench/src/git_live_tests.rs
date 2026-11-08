//! Git kept live in a headless workbench: watcher batches handed to the
//! project panel as the watcher would, refreshing the Changes panel and the
//! open editors' diff bases, on repositories built with the `git` CLI in
//! temporary directories.

// What only its Unix tests use goes unused elsewhere.
#![cfg_attr(not(unix), allow(unused_imports))]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::row_changes::{RowChange, RowChangeKind};
use fe_git::{ChangeKind, FileStatus};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{DEBOUNCE, GitState};
use crate::{GitPanel, Launch, Workbench};

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

/// A repository with `files` committed, each holding three rows.
fn repo(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for file in files {
        write(root, file, "one\ntwo\nthree\n");
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    dir
}

/// A workbench for `root`, in a headless window, focused as the app
/// focuses it.
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
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Opens `path` in the active pane, and lets it load.
fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    editor(workbench, path, cx)
}

/// The open editor of `path`.
fn editor(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    })
}

fn row_changes(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<RowChange> {
    editor.read_with(cx, |editor, _| editor.model().row_changes().to_vec())
}

fn change(rows: std::ops::Range<u32>, kind: RowChangeKind) -> RowChange {
    RowChange { rows, kind }
}

/// Hands `changes` to the project panel as a watcher batch.
fn batch(workbench: &Entity<Workbench>, changes: Vec<FsChange>, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
}

/// Lets the debounce run out, and what it starts land.
fn settle(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(DEBOUNCE);
    draw(cx);
}

fn git_state(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitState> {
    workbench.read_with(cx, |workbench, _| workbench.git().clone())
}

/// Refreshes started, and those that read the status and of them those
/// off the main thread.
fn runs(git: &Entity<GitState>, cx: &mut VisualTestContext) -> (usize, (usize, usize)) {
    git.read_with(cx, |git, _| git.runs())
}

fn changes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    })
}

/// The Changes panel's rows: their letters and paths from the root.
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

#[gpui::test]
fn a_batch_naming_a_changed_file_lists_it_after_the_debounce(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", "b.rs"]);
    let (workbench, cx) = open(cx, dir.path());
    let (git, panel) = (git_state(&workbench, cx), changes(&workbench, cx));
    assert_eq!(
        runs(&git, cx).0,
        1,
        "the tree's opening runs, then nothing until something changes"
    );

    write(dir.path(), "a.rs", "one\nTWO\nthree\n");
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join("a.rs"))],
        cx,
    );
    draw(cx);
    assert_eq!(runs(&git, cx).0, 1, "not before the debounce");
    assert!(rows(&panel, cx).is_empty());

    settle(cx);
    assert_eq!(runs(&git, cx), (2, (2, 2)), "read off the main thread");
    assert_eq!(rows(&panel, cx), [("M", "a.rs".to_owned())]);

    // Fifty batches within the debounce, each restarting it.
    write(dir.path(), "b.rs", "changed\n");
    for i in 0..50 {
        let path = match i % 2 {
            0 => dir.path().join("b.rs"),
            _ => dir.path().join(".git/index"),
        };
        batch(&workbench, vec![FsChange::Path(path)], cx);
        cx.executor().advance_clock(DEBOUNCE / 60);
        draw(cx);
    }
    settle(cx);
    let (started, _) = runs(&git, cx);
    assert!(started - 2 <= 2, "{} runs for fifty batches", started - 2);
    assert_eq!(
        rows(&panel, cx),
        [("M", "a.rs".to_owned()), ("M", "b.rs".to_owned())]
    );

    // A rescan refreshes too.
    write(dir.path(), "c.rs", "new\n");
    batch(&workbench, vec![FsChange::Rescan], cx);
    settle(cx);
    assert_eq!(
        rows(&panel, cx),
        [
            ("M", "a.rs".to_owned()),
            ("M", "b.rs".to_owned()),
            ("U", "c.rs".to_owned()),
        ]
    );
}

#[gpui::test]
fn an_opened_file_gets_its_head_text_as_its_diff_base(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", ".gitignore"]);
    write(dir.path(), ".gitignore", "*.log\n");
    git(dir.path(), &["commit", "-q", "-am", "ignore logs"]);
    write(dir.path(), "a.rs", "one\nTWO\nthree\n");
    write(dir.path(), "new.rs", "x\ny\n");
    write(dir.path(), "out.log", "x\ny\n");
    let elsewhere = tempfile::tempdir().unwrap();
    write(elsewhere.path(), "far.rs", "x\ny\n");
    let (workbench, cx) = open(cx, dir.path());
    let git = git_state(&workbench, cx);

    // Modified and tracked: its HEAD text, so the changed row is marked.
    let a = open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert_eq!(row_changes(&a, cx), [change(1..2, RowChangeKind::Modified)]);
    // Untracked: an empty base, so every row is added.
    let new = open_file(&workbench, &dir.path().join("new.rs"), cx);
    assert_eq!(row_changes(&new, cx), [change(0..2, RowChangeKind::Added)]);
    // Ignored, or outside the repository: none.
    let log = open_file(&workbench, &dir.path().join("out.log"), cx);
    assert!(row_changes(&log, cx).is_empty());
    let far = open_file(&workbench, &elsewhere.path().join("far.rs"), cx);
    assert!(row_changes(&far, cx).is_empty());

    // The first open read the status, and each file HEAD doesn't have read
    // it again: the untracked and the ignored ones. The one outside the
    // repository didn't.
    assert_eq!(runs(&git, cx), (3, (3, 3)));
    assert!(!git.read_with(cx, |git, _| git.is_running()));

    // An edit is diffed against the base as typed.
    a.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(0);
        editor.model_mut().insert("zero\n");
        cx.notify();
    });
    assert_eq!(
        row_changes(&a, cx),
        [
            change(0..1, RowChangeKind::Added),
            change(2..3, RowChangeKind::Modified)
        ]
    );
}

#[gpui::test]
fn a_commit_makes_the_new_head_the_open_editors_base(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", "b.rs"]);
    write(dir.path(), "a.rs", "one\nTWO\nthree\n");
    let (workbench, cx) = open(cx, dir.path());
    let a = open_file(&workbench, &dir.path().join("a.rs"), cx);
    let b = open_file(&workbench, &dir.path().join("b.rs"), cx);
    assert_eq!(row_changes(&a, cx), [change(1..2, RowChangeKind::Modified)]);
    assert!(row_changes(&b, cx).is_empty());

    git(dir.path(), &["commit", "-q", "-am", "two"]);
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join(".git/index"))],
        cx,
    );
    draw(cx);
    assert!(!row_changes(&a, cx).is_empty(), "not before the debounce");
    settle(cx);
    assert!(row_changes(&a, cx).is_empty(), "{:?}", row_changes(&a, cx));
    assert!(row_changes(&b, cx).is_empty());
    assert_eq!(rows(&changes(&workbench, cx), cx), []);
}

#[gpui::test]
fn a_batch_while_status_runs_runs_it_exactly_once_more(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs"]);
    let (workbench, cx) = open(cx, dir.path());
    let (git, panel) = (git_state(&workbench, cx), changes(&workbench, cx));
    git.update(cx, |git, _| git.hold_runs());

    write(dir.path(), "a.rs", "changed\n");
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join("a.rs"))],
        cx,
    );
    settle(cx);
    // The tree's opening ran, and this one waits to land.
    assert_eq!(runs(&git, cx), (2, (2, 2)));
    assert!(git.read_with(cx, |git, _| git.is_running()));

    // Batches while it runs, the debounce running out again.
    write(dir.path(), "b.rs", "new\n");
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join("b.rs"))],
        cx,
    );
    settle(cx);
    batch(&workbench, vec![FsChange::Rescan], cx);
    settle(cx);
    assert_eq!(runs(&git, cx).0, 2, "batches wait for the run");

    git.update(cx, |git, _| git.release_runs());
    draw(cx);
    settle(cx);
    assert_eq!(runs(&git, cx), (3, (3, 3)), "exactly one more");
    assert!(!git.read_with(cx, |git, _| git.is_running()));
    assert_eq!(
        rows(&panel, cx),
        [("M", "a.rs".to_owned()), ("U", "b.rs".to_owned())]
    );
}

#[gpui::test]
fn a_refresh_for_a_root_replaced_is_never_shown(cx: &mut TestAppContext) {
    let first = repo(&["a.rs"]);
    let second = repo(&["x.rs"]);
    write(second.path(), "x.rs", "changed\n");
    let (workbench, cx) = open(cx, first.path());
    let a = open_file(&workbench, &first.path().join("a.rs"), cx);
    assert!(row_changes(&a, cx).is_empty(), "as committed");
    let old = git_state(&workbench, cx);
    old.update(cx, |git, _| git.hold_runs());

    // a.rs is untracked from now on: the held refresh would mark every row
    // added, and list it.
    git(first.path(), &["rm", "-q", "--cached", "a.rs"]);
    git(first.path(), &["commit", "-q", "-m", "untrack"]);
    batch(
        &workbench,
        vec![FsChange::Path(first.path().join(".git/index"))],
        cx,
    );
    settle(cx);
    assert_eq!(runs(&old, cx).0, 2);
    assert!(old.read_with(cx, |git, _| git.is_running()));

    let root = second.path().to_owned();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(root, window, cx)
    });
    draw(cx);
    // Even kept alive, it lands nothing.
    old.update(cx, |git, _| git.release_runs());
    draw(cx);
    settle(cx);
    assert!(!old.read_with(cx, |git, _| git.is_running()));

    // The new root's repository doesn't have a.rs: it has no base.
    assert!(row_changes(&a, cx).is_empty(), "{:?}", row_changes(&a, cx));
    let new = git_state(&workbench, cx);
    assert_ne!(new, old);
    assert_eq!(runs(&new, cx).0, 1);
    // Its Changes panel has only had the new root's statuses.
    dispatch("git.show_changes", cx);
    assert_eq!(
        rows(&changes(&workbench, cx), cx),
        [("M", "x.rs".to_owned())]
    );
}

#[gpui::test]
fn git_refresh_reloads_the_open_editors_bases(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs"]);
    write(dir.path(), "a.rs", "one\nTWO\nthree\n");
    let (workbench, cx) = open(cx, dir.path());
    let a = open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert!(!row_changes(&a, cx).is_empty());

    // As when the repository's `.git` is outside the root: no batch.
    git(dir.path(), &["commit", "-q", "-am", "two"]);
    dispatch("git.refresh", cx);
    assert!(row_changes(&a, cx).is_empty());
}

/// A repository whose `sub/c.rs` is modified and `sub/d.rs` deleted, and
/// whose `a.rs` is modified and `b.rs` deleted, with `link` a symlink to
/// `sub`.
#[cfg(unix)]
fn nested() -> tempfile::TempDir {
    let dir = repo(&["a.rs", "b.rs", "sub/c.rs", "sub/d.rs"]);
    std::os::unix::fs::symlink("sub", dir.path().join("link")).unwrap();
    write(dir.path(), "a.rs", "changed\n");
    fs::remove_file(dir.path().join("b.rs")).unwrap();
    write(dir.path(), "sub/c.rs", "changed\n");
    fs::remove_file(dir.path().join("sub/d.rs")).unwrap();
    dir
}

#[gpui::test]
#[cfg(unix)]
fn a_root_through_dot_dot_lists_its_changes(cx: &mut TestAppContext) {
    let dir = nested();
    let root = dir.path().join("sub/..");
    let (workbench, cx) = open(cx, &root);
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);
    assert_eq!(
        rows(&panel, cx),
        [
            ("M", "a.rs".to_owned()),
            ("D", "b.rs".to_owned()),
            ("U", "link".to_owned()),
            ("M", "sub/c.rs".to_owned()),
            ("D", "sub/d.rs".to_owned()),
        ]
    );
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.paths())[0],
        root.join("a.rs")
    );
}

#[gpui::test]
#[cfg(unix)]
fn a_root_through_a_symlink_in_the_repository_lists_its_changes(cx: &mut TestAppContext) {
    let dir = nested();
    let root = dir.path().join("link");
    let (workbench, cx) = open(cx, &root);
    dispatch("git.show_changes", cx);
    let panel = changes(&workbench, cx);
    assert_eq!(
        rows(&panel, cx),
        [("M", "c.rs".to_owned()), ("D", "d.rs".to_owned())]
    );
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.paths()),
        [root.join("c.rs"), root.join("d.rs")]
    );

    // An editor opened through the symlink gets its file's HEAD text.
    let c = open_file(&workbench, &root.join("c.rs"), cx);
    assert_eq!(row_changes(&c, cx), [change(0..1, RowChangeKind::Modified)]);
}

/// A repository with `a.rs` modified, opened through a symlink to it.
#[cfg(unix)]
fn aliased() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    git(&real, &["init", "-q"]);
    write(&real, "a.rs", "a\n");
    git(&real, &["add", "."]);
    git(&real, &["commit", "-q", "-m", "init"]);
    write(&real, "a.rs", "changed\n");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    (dir, dunce::canonicalize(&real).unwrap(), link)
}

fn set_statuses(panel: &Entity<GitPanel>, statuses: Vec<FileStatus>, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.set_statuses(statuses, cx));
}

#[gpui::test]
#[cfg(unix)]
fn statuses_given_before_the_panel_shows_still_let_it_read(cx: &mut TestAppContext) {
    let (dir, real, link) = aliased();
    let added = FileStatus {
        path: real.join("z.rs"),
        kind: ChangeKind::Added,
    };
    // Given before anything ran, the tree's opening's run included.
    let (_workbench, cx, panel) = open_held(cx, dir.path(), &link, |panel, cx| {
        panel.set_statuses(vec![added], cx)
    });
    panel.update(cx, |panel, _| panel.release_runs());
    draw(cx);

    dispatch("git.show_changes", cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.runs_started()), 1);
    assert_eq!(rows(&panel, cx), [("M", "a.rs".to_owned())]);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.paths()),
        [link.join("a.rs")]
    );
}

/// A workbench opened on `first`, then on `root`, whose Changes panel
/// `prepare` is given before anything for `root` has run, and whose status
/// runs for it, the first its tree's opening starts, wait for
/// [`GitPanel::release_runs`] once they've read the status; and that
/// panel.
#[cfg(unix)]
fn open_held<'a>(
    cx: &'a mut TestAppContext,
    first: &Path,
    root: &Path,
    prepare: impl FnOnce(&mut GitPanel, &mut gpui::Context<GitPanel>),
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    Entity<GitPanel>,
) {
    let (workbench, cx) = open(cx, first);
    let root = root.to_owned();
    let panel = workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(root, window, cx);
        // Its tree opens in the background: nothing has run.
        let panel = workbench.dock().read(cx).changes().clone();
        panel.update(cx, |panel, cx| {
            panel.hold_runs();
            prepare(panel, cx);
        });
        panel
    });
    draw(cx);
    (workbench, cx, panel)
}

#[gpui::test]
#[cfg(unix)]
fn statuses_given_while_the_panel_first_reads_list_once_it_has(cx: &mut TestAppContext) {
    let (dir, real, link) = aliased();
    let (_workbench, cx, panel) = open_held(cx, dir.path(), &link, |_, _| {});
    dispatch("git.show_changes", cx);
    // The tree's opening's run, the first.
    assert!(panel.read_with(cx, |panel, _| panel.is_running()));
    assert_eq!(panel.read_with(cx, |panel, _| panel.runs_started()), 1);

    let statuses = vec![
        FileStatus {
            path: real.join("z.rs"),
            kind: ChangeKind::Added,
        },
        FileStatus {
            path: real.join("a.rs"),
            kind: ChangeKind::Conflicted,
        },
    ];
    set_statuses(&panel, statuses, cx);
    panel.update(cx, |panel, _| panel.release_runs());
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.is_running()));
    // The older run's list isn't shown, but where it found the root is.
    assert_eq!(
        rows(&panel, cx),
        [("C", "a.rs".to_owned()), ("A", "z.rs".to_owned())]
    );
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.paths()),
        [link.join("a.rs"), link.join("z.rs")]
    );
}

/// Runs `git` in `dir` and returns what it printed, trimmed.
fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[gpui::test]
fn a_run_landing_while_a_batch_is_due_leaves_the_refresh_to_its_debounce(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs"]);
    let (workbench, cx) = open(cx, dir.path());
    let (git, panel) = (git_state(&workbench, cx), changes(&workbench, cx));
    git.update(cx, |git, _| git.hold_runs());
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join("a.rs"))],
        cx,
    );
    settle(cx);
    // The tree's opening ran, and this one waits to land.
    assert_eq!(runs(&git, cx).0, 2);

    // One batch's debounce runs out while the run does, asking for one
    // more; another's is still to when the run lands.
    batch(&workbench, vec![FsChange::Rescan], cx);
    settle(cx);
    write(dir.path(), "b.rs", "new\n");
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join("b.rs"))],
        cx,
    );
    cx.executor().advance_clock(DEBOUNCE / 2);
    git.update(cx, |git, _| git.release_runs());
    draw(cx);
    assert_eq!(runs(&git, cx).0, 2, "the latest batch's debounce stands");

    cx.executor().advance_clock(DEBOUNCE / 2);
    draw(cx);
    settle(cx);
    assert_eq!(runs(&git, cx).0, 3, "exactly one more");
    assert_eq!(rows(&panel, cx), [("U", "b.rs".to_owned())]);
}

#[gpui::test]
fn files_created_after_the_last_refresh_get_their_bases_without_a_batch(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", ".gitignore"]);
    write(dir.path(), ".gitignore", "*.log\n");
    git(dir.path(), &["commit", "-q", "-am", "ignore logs"]);
    let (workbench, cx) = open(cx, dir.path());
    let state = git_state(&workbench, cx);
    open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert_eq!(runs(&state, cx).0, 1);

    // Untracked, with no final line break: every row is added.
    write(dir.path(), "new.rs", "hello");
    let new = open_file(&workbench, &dir.path().join("new.rs"), cx);
    assert_eq!(row_changes(&new, cx), [change(0..1, RowChangeKind::Added)]);
    assert_eq!(runs(&state, cx).0, 2, "the statuses didn't list it");
    // Added, likewise.
    write(dir.path(), "staged.rs", "x\ny");
    git(dir.path(), &["add", "staged.rs"]);
    let staged = open_file(&workbench, &dir.path().join("staged.rs"), cx);
    assert_eq!(
        row_changes(&staged, cx),
        [change(0..2, RowChangeKind::Added)]
    );
    // Ignored: none.
    write(dir.path(), "late.log", "x\n");
    let log = open_file(&workbench, &dir.path().join("late.log"), cx);
    assert!(row_changes(&log, cx).is_empty());
    assert!(!state.read_with(cx, |git, _| git.is_running()));
}

#[gpui::test]
fn binary_new_files_have_no_base(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs"]);
    write(dir.path(), "blob.bin", "a\0b\nc\n");
    write(dir.path(), "staged.bin", "a\0b\nc\n");
    git(dir.path(), &["add", "staged.bin"]);
    let (workbench, cx) = open(cx, dir.path());
    let untracked = open_file(&workbench, &dir.path().join("blob.bin"), cx);
    let staged = open_file(&workbench, &dir.path().join("staged.bin"), cx);
    assert!(row_changes(&untracked, cx).is_empty());
    assert!(row_changes(&staged, cx).is_empty());
}

#[gpui::test]
#[cfg(unix)]
fn a_file_in_a_deleted_directory_through_an_aliased_root_keeps_its_head_text(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    git(&real, &["init", "-q"]);
    write(&real, "sub/deep/x.rs", "one\ntwo\nthree\n");
    git(&real, &["add", "."]);
    git(&real, &["commit", "-q", "-m", "init"]);
    write(&real, "sub/deep/x.rs", "one\nTWO\nthree\n");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let (workbench, cx) = open(cx, &link);
    let path = link.join("sub/deep/x.rs");
    let x = open_file(&workbench, &path, cx);
    let modified = [change(1..2, RowChangeKind::Modified)];
    assert_eq!(row_changes(&x, cx), modified);

    fs::remove_dir_all(real.join("sub")).unwrap();
    batch(&workbench, vec![FsChange::Path(link.join("sub"))], cx);
    settle(cx);
    assert_eq!(row_changes(&x, cx), modified, "still against HEAD's text");
}

#[gpui::test]
fn a_base_that_failed_to_read_is_read_again_at_the_next_refresh(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs"]);
    write(dir.path(), "a.rs", "one\nTWO\nthree\n");
    // HEAD's blob for a.rs, moved away so it can't be read.
    let id = git_output(dir.path(), &["rev-parse", "HEAD:a.rs"]);
    let object = dir
        .path()
        .join(".git/objects")
        .join(&id[..2])
        .join(&id[2..]);
    let aside = dir.path().join("blob");
    fs::rename(&object, &aside).unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let a = open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert!(row_changes(&a, cx).is_empty(), "no base yet");
    let git = git_state(&workbench, cx);
    // The tree's opening read the status, and a.rs, whose HEAD text it then
    // couldn't find, had it read again.
    assert_eq!(runs(&git, cx).0, 2, "not read again until a refresh");

    // HEAD and the statuses are as they were, but the base is read again.
    fs::rename(&aside, &object).unwrap();
    batch(
        &workbench,
        vec![FsChange::Path(dir.path().join("a.rs"))],
        cx,
    );
    settle(cx);
    assert_eq!(row_changes(&a, cx), [change(1..2, RowChangeKind::Modified)]);
}

#[gpui::test]
fn a_file_ignored_since_the_last_refresh_has_no_base(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", ".gitignore"]);
    write(dir.path(), "late.txt", "x\n");
    let (workbench, cx) = open(cx, dir.path());
    let git = git_state(&workbench, cx);
    open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert_eq!(runs(&git, cx).0, 1, "its statuses list late.txt untracked");

    write(dir.path(), ".gitignore", "*.txt\n");
    let late = open_file(&workbench, &dir.path().join("late.txt"), cx);
    assert!(
        row_changes(&late, cx).is_empty(),
        "{:?}",
        row_changes(&late, cx)
    );
    assert_eq!(runs(&git, cx).0, 2);
}

#[gpui::test]
#[cfg(unix)]
fn a_file_through_a_symlink_whose_target_is_gone_keeps_its_head_text(cx: &mut TestAppContext) {
    let dir = repo(&["sub/deep/x.rs"]);
    std::os::unix::fs::symlink("sub", dir.path().join("link")).unwrap();
    let (workbench, cx) = open(cx, dir.path());
    open_file(&workbench, &dir.path().join("sub/deep/x.rs"), cx);

    // The link dangles now, but x.rs is still in HEAD as sub/deep/x.rs.
    fs::remove_dir_all(dir.path().join("sub")).unwrap();
    let path = dir.path().join("link/deep/x.rs");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    let x = editor(&workbench, &path, cx);
    // An empty buffer against HEAD's three rows.
    assert_eq!(row_changes(&x, cx), [change(0..0, RowChangeKind::Deleted)]);
}

#[gpui::test]
fn the_panel_and_the_watcher_share_one_status_run(cx: &mut TestAppContext) {
    let dir = repo(&["sub/a.rs"]);
    write(dir.path(), "sub/a.rs", "changed\n");
    let root = dir.path().join("sub");
    let (workbench, cx) = open(cx, &root);
    let (state, panel) = (git_state(&workbench, cx), changes(&workbench, cx));
    state.update(cx, |git, _| git.hold_runs());
    batch(&workbench, vec![FsChange::Path(root.join("a.rs"))], cx);
    settle(cx);
    // The tree's opening ran, and this one waits to land.
    assert_eq!(runs(&state, cx).0, 2);

    // Committed with `.git` outside the root: no batch. Showing the panel
    // reads nothing more while the run's in flight; a refresh waits for it.
    git(dir.path(), &["commit", "-q", "-am", "commit"]);
    dispatch("git.show_changes", cx);
    dispatch("git.refresh", cx);
    assert_eq!(runs(&state, cx).0, 2, "one runner");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.runs_started()),
        2,
        "the panel's runs are the state's"
    );

    state.update(cx, |git, _| git.release_runs());
    draw(cx);
    assert_eq!(runs(&state, cx), (3, (3, 3)));
    assert_eq!(rows(&panel, cx), [], "the newer run's list, not the older");
    assert_eq!(
        panel
            .read_with(cx, |panel, _| panel.message_text())
            .as_deref(),
        Some("No changes")
    );
}

#[gpui::test]
fn an_older_run_never_replaces_a_newer_list(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs"]);
    write(dir.path(), "a.rs", "changed\n");
    let (workbench, cx) = open(cx, dir.path());
    let (git, panel) = (git_state(&workbench, cx), changes(&workbench, cx));
    git.update(cx, |git, _| git.hold_runs());
    dispatch("git.show_changes", cx);
    // The tree's opening read the status already: this run waits to land.
    dispatch("git.refresh", cx);
    assert!(panel.read_with(cx, |panel, _| panel.is_running()));

    // A newer list, given while the run reads.
    set_statuses(&panel, vec![], cx);
    git.update(cx, |git, _| git.release_runs());
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.is_running()));
    assert_eq!(rows(&panel, cx), []);
}

#[gpui::test]
fn files_opened_while_status_runs_get_their_bases_from_a_fresh_one(cx: &mut TestAppContext) {
    let dir = repo(&["a.rs", ".gitignore"]);
    write(dir.path(), "soon.txt", "x\n");
    let (workbench, cx) = open(cx, dir.path());
    let state = git_state(&workbench, cx);
    open_file(&workbench, &dir.path().join("a.rs"), cx);
    assert_eq!(runs(&state, cx).0, 1);

    // A refresh reads the status, and waits to land: soon.txt untracked,
    // new.rs not there yet.
    state.update(cx, |git, _| git.hold_runs());
    batch(&workbench, vec![FsChange::Rescan], cx);
    settle(cx);
    assert!(state.read_with(cx, |git, _| git.is_running()));

    // Meanwhile soon.txt is ignored, new.rs created, and both opened.
    write(dir.path(), ".gitignore", "*.txt\n");
    write(dir.path(), "new.rs", "x\ny\n");
    let soon = open_file(&workbench, &dir.path().join("soon.txt"), cx);
    let new = open_file(&workbench, &dir.path().join("new.rs"), cx);
    state.update(cx, |git, _| git.release_runs());
    draw(cx);

    assert!(
        row_changes(&soon, cx).is_empty(),
        "{:?}",
        row_changes(&soon, cx)
    );
    assert_eq!(row_changes(&new, cx), [change(0..2, RowChangeKind::Added)]);
    assert_eq!(runs(&state, cx).0, 3, "a fresh status for them");
}

#[path = "git_conflicts_tests.rs"]
mod conflicts;
