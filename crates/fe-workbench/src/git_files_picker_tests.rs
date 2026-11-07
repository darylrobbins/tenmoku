//! Go to Tracked File: a temp repository's tracked and untracked files but
//! not its ignored ones, a folder outside any repository, and a pick
//! opening its file.

use std::fs;
use std::path::Path;
use std::process::Command;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext};

use crate::{PickerDelegate as _, Workbench, tests::open_root, workbench::Palette};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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

/// A repository with `.gitignore` and `src/tracked.rs` added, `new.txt`
/// untracked and `debug.log` ignored.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    fs::write(root.join("src/tracked.rs"), "fn main() {}\n").unwrap();
    git(root, &["add", "."]);
    fs::write(root.join("new.txt"), "new\n").unwrap();
    fs::write(root.join("debug.log"), "log\n").unwrap();
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs `git.show_files` where focus is, in the workbench's context, and
/// waits for its list.
fn show_files(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of("git.show_files").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let settled = workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.git_files_picker(cx).expect("the picker shows");
        picker.read(cx).delegate.settled()
    });
    assert!(settled, "the list landed");
}

/// The picker's rows and its status line.
fn rows(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<String>, Option<SharedString>) {
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.git_files_picker(cx).expect("the picker shows");
        let picker = picker.read(cx);
        (picker.delegate.labels(), picker.delegate.status())
    })
}

#[gpui::test]
fn lists_tracked_and_untracked_but_not_ignored(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = open_root(cx, dir.path());
    show_files(&workbench, cx);
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.palette),
        Some(Palette::GitFiles)
    );
    let (labels, status) = rows(&workbench, cx);
    assert_eq!(labels, [".gitignore", "new.txt", "src/tracked.rs"]);
    assert_eq!(status.as_deref(), Some("3 files"));

    // Opened again, with a file added meanwhile (neither HEAD nor the index
    // changed), the new list lands in the kept one's place.
    cx.update(|window, cx| {
        (workbench.clone()).update(cx, |workbench, cx| workbench.dismiss_palette(window, cx))
    });
    fs::write(dir.path().join("later.txt"), "later\n").unwrap();
    show_files(&workbench, cx);
    assert_eq!(
        rows(&workbench, cx).0,
        [".gitignore", "later.txt", "new.txt", "src/tracked.rs"]
    );

    // Typing filters by the path.
    let picker = workbench.read_with(cx, |workbench, cx| workbench.git_files_picker(cx).unwrap());
    picker.update(cx, |picker, cx| {
        picker.delegate.update_matches("srtr".into(), cx)
    });
    draw(cx);
    assert_eq!(rows(&workbench, cx).0, ["src/tracked.rs"]);
}

#[gpui::test]
fn lists_files_under_a_root_inside_the_repo(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = open_root(cx, &dir.path().join("src"));
    show_files(&workbench, cx);
    assert_eq!(rows(&workbench, cx).0, ["tracked.rs"]);
}

#[gpui::test]
fn outside_a_repo_says_so(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    show_files(&workbench, cx);
    let (labels, status) = rows(&workbench, cx);
    assert_eq!(labels, Vec::<String>::new());
    assert_eq!(status.as_deref(), Some("Not a git repository"));
}

#[gpui::test]
fn picking_opens_the_file(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = open_root(cx, dir.path());
    show_files(&workbench, cx);
    let ix = (rows(&workbench, cx).0.iter())
        .position(|label| label == "src/tracked.rs")
        .unwrap();
    let picker = workbench.read_with(cx, |workbench, cx| workbench.git_files_picker(cx).unwrap());
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(ix, window, cx)));
    draw(cx);
    let (palette, shown) = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let shown = pane.active().and_then(|ix| pane.tabs()[ix].path.clone());
        (workbench.palette, shown)
    });
    assert_eq!(palette, None);
    let shown = shown.expect("a file is shown");
    assert_eq!(
        shown.canonicalize().unwrap(),
        dir.path().join("src/tracked.rs").canonicalize().unwrap()
    );
}

#[gpui::test]
fn a_refresh_keeps_the_selected_file(cx: &mut TestAppContext) {
    let dir = repo();
    let (workbench, cx) = open_root(cx, dir.path());
    show_files(&workbench, cx);
    let picker = workbench.read_with(cx, |workbench, cx| workbench.git_files_picker(cx).unwrap());
    picker.update(cx, |picker, cx| picker.select(2, cx));
    assert_eq!(rows(&workbench, cx).0[2], "src/tracked.rs");

    // A file listed before it lands with the next list.
    fs::write(dir.path().join("added.txt"), "added\n").unwrap();
    picker.update(cx, |picker, cx| picker.delegate.list(cx));
    draw(cx);
    let (labels, _) = rows(&workbench, cx);
    assert_eq!(
        labels,
        [".gitignore", "added.txt", "new.txt", "src/tracked.rs"]
    );
    assert_eq!(picker.read_with(cx, |picker, _| picker.selected()), 3);

    // With the selected file gone, the selection stays where it was.
    fs::remove_file(dir.path().join("src/tracked.rs")).unwrap();
    picker.update(cx, |picker, cx| picker.select(1, cx));
    fs::remove_file(dir.path().join("added.txt")).unwrap();
    picker.update(cx, |picker, cx| picker.delegate.list(cx));
    draw(cx);
    assert_eq!(rows(&workbench, cx).0, [".gitignore", "new.txt"]);
    assert_eq!(picker.read_with(cx, |picker, _| picker.selected()), 1);
}
