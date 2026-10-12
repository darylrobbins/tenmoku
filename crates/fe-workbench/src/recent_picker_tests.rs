//! Open Recent: what's recorded in a temp state dir's `recent.toml`, the
//! picker over it, the new-window confirm, clearing, and focus with vim.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::recent::{self, Entry, RecentList};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{set_recent_file, tilde};
use crate::{Launch, Workbench, workbench::Palette};

/// A window of a workbench rooted at `root`, keeping the recent list in
/// `file`, active and drawn.
fn window(
    cx: &mut TestAppContext,
    root: Option<&Path>,
    file: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
    let file = file.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_recent_file(cx, file);
    });
    let launch = Launch {
        root: root.map(Path::to_owned),
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
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs command `id` where focus is, in the contexts `frames`.
fn run_in(id: &str, frames: &[&str], cx: &mut VisualTestContext) {
    let context = (frames.iter()).fold(ContextStack::new(), |stack, frame| {
        stack.with(ContextFrame::new(*frame))
    });
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn run(id: &str, cx: &mut VisualTestContext) {
    run_in(id, &["workbench"], cx);
}

/// Writes each of `files` in `dir`, holding its own name.
fn write(dir: &Path, files: &[&str]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|file| {
            fs::write(dir.join(file), format!("{file}\n")).unwrap();
            dir.join(file)
        })
        .collect()
}

fn list(file: &Path) -> RecentList {
    let (list, error) = RecentList::load(file);
    assert!(error.is_none(), "{error:?}");
    list
}

/// The picker's rows: each label, and whether it's a folder.
fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.recent_picker().read(cx);
        assert!(picker.delegate.loaded(), "the list is read");
        picker.delegate.labels()
    })
}

fn label(path: &Path, folder: bool) -> (String, bool) {
    (tilde(path, std::env::home_dir().as_deref()), folder)
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

fn root(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned))
}

/// The active pane's files, and its active tab's.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<PathBuf>, Option<PathBuf>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let paths = (pane.tabs().iter())
            .filter_map(|tab| tab.path.clone())
            .collect();
        let active = pane.active().and_then(|ix| pane.tabs()[ix].path.clone());
        (paths, active)
    })
}

#[gpui::test]
fn records(cx: &mut TestAppContext) {
    let (state, a, b) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let file = state.path().join("recent.toml");
    let paths = write(a.path(), &["one.txt", "two.txt"]);
    let (workbench, mut vcx) = window(cx, Some(a.path()), &file);
    let cx = &mut vcx;
    assert_eq!(list(&file).folders, [a.path()]);

    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for path in &paths {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
        draw(cx);
    }
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(b.path().to_owned(), window, cx)
        })
    });
    draw(cx);

    let recorded = list(&file);
    assert_eq!(recorded.folders, [b.path(), a.path()]);
    assert_eq!(recorded.files, [paths[1].clone(), paths[0].clone()]);

    // Opening a file again moves it to the front, once.
    let first = paths[0].clone();
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.close_active(window, cx);
            pane.close_active(window, cx);
            pane.open(first, window, cx);
        })
    });
    draw(cx);
    assert_eq!(list(&file).files, [paths[0].clone(), paths[1].clone()]);
}

#[gpui::test]
fn picker(cx: &mut TestAppContext) {
    let (state, dirs) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = state.path().join("recent.toml");
    let (alpha, beta, gone) = (
        dirs.path().join("alpha"),
        dirs.path().join("the_beta"),
        dirs.path().join("gone"),
    );
    for dir in [&alpha, &beta, &gone] {
        fs::create_dir(dir).unwrap();
    }
    let files = write(dirs.path(), &["notes.txt", "to_do.txt", "deleted.txt"]);
    // Oldest first, so each lands at the front.
    for entry in [
        Entry::Folder(&gone),
        Entry::Folder(&beta),
        Entry::Folder(&alpha),
        Entry::File(&files[2]),
        Entry::File(&files[1]),
        Entry::File(&files[0]),
    ] {
        recent::record(&file, entry).unwrap();
    }
    fs::remove_dir(&gone).unwrap();
    fs::remove_file(&files[2]).unwrap();
    let (workbench, mut vcx) = window(cx, None, &file);
    let cx = &mut vcx;

    run("workbench.open_recent", cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Recent));
    assert_eq!(
        rows(&workbench, cx),
        [
            label(&alpha, true),
            label(&beta, true),
            label(&files[0], false),
            label(&files[1], false),
        ]
    );

    // Filtered by the query, as a subsequence, ignoring case: `_` is in
    // no temp dir's name.
    cx.simulate_input("_DO");
    draw(cx);
    assert_eq!(rows(&workbench, cx), [label(&files[1], false)]);

    // Enter on a file opens it in the active pane.
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        (vec![files[1].clone()], Some(files[1].clone()))
    );
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a tab is shown");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // Enter on a folder re-roots the window.
    run("workbench.open_recent", cx);
    cx.simulate_input("_beta");
    draw(cx);
    assert_eq!(rows(&workbench, cx), [label(&beta, true)]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(root(&workbench, cx).as_deref(), Some(beta.as_path()));
    assert_eq!(tabs(&workbench, cx).0, [files[1].clone()]);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    // And the folder is the most recent now.
    assert_eq!(list(&file).folders[0], beta);
}

#[gpui::test]
fn new_window_and_clear(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = state.path().join("recent.toml");
    let files = write(dir.path(), &["a.txt"]);
    recent::record(&file, Entry::File(&files[0])).unwrap();
    recent::record(&file, Entry::Folder(dir.path())).unwrap();
    let asked = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let asked = asked.clone();
        crate::set_window_opener(cx, move |root, _| asked.borrow_mut().push(root));
    });
    let (workbench, mut vcx) = window(cx, None, &file);
    let cx = &mut vcx;

    // The command, on the selected folder.
    run("workbench.open_recent", cx);
    assert_eq!(
        rows(&workbench, cx),
        [label(dir.path(), true), label(&files[0], false)]
    );
    run_in(super::OPEN_IN_NEW_WINDOW, &["workbench", "picker"], cx);
    assert_eq!(*asked.borrow(), [Some(dir.path().to_owned())]);
    assert_eq!(palette(&workbench, cx), None);
    // This window keeps its root.
    assert_eq!(root(&workbench, cx), None);

    // The folder row's button, as clicked.
    run("workbench.open_recent", cx);
    let button = cx
        .debug_bounds("icon-button:recent-new-window-0")
        .expect("the folder row has a new-window button");
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    assert_eq!(*asked.borrow(), vec![Some(dir.path().to_owned()); 2]);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(root(&workbench, cx), None);
    assert_eq!(tabs(&workbench, cx).0, Vec::<PathBuf>::new());

    // The command on a file opens it here, as Enter would.
    run("workbench.open_recent", cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    run_in(super::OPEN_IN_NEW_WINDOW, &["workbench", "picker"], cx);
    assert_eq!(asked.borrow().len(), 2);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        (vec![files[0].clone()], Some(files[0].clone()))
    );

    run("workbench.clear_recent", cx);
    assert_eq!(list(&file), RecentList::default());
    run("workbench.open_recent", cx);
    assert_eq!(rows(&workbench, cx), []);
    let status = workbench.read_with(cx, |workbench, cx| {
        use crate::PickerDelegate as _;
        workbench.recent_picker().read(cx).delegate.status()
    });
    assert!(status.is_some(), "the empty list says so");
    cx.simulate_keystrokes("escape");
    draw(cx);
}

#[gpui::test]
fn clear_while_loading(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = state.path().join("recent.toml");
    recent::record(&file, Entry::Folder(dir.path())).unwrap();
    let (workbench, mut vcx) = window(cx, None, &file);
    let cx = &mut vcx;

    // Cleared before the list read for the picker lands.
    let dispatch = |id: &str, cx: &mut VisualTestContext| {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    };
    dispatch("workbench.open_recent", cx);
    dispatch("workbench.clear_recent", cx);
    draw(cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Recent));
    assert_eq!(rows(&workbench, cx), []);
    assert_eq!(list(&file), RecentList::default());
}

#[gpui::test]
fn vim(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = state.path().join("recent.toml");
    let files = write(dir.path(), &["first.txt", "the_second.txt"]);
    recent::record(&file, Entry::File(&files[1])).unwrap();
    let (workbench, mut vcx) = window(cx, Some(dir.path()), &file);
    let cx = &mut vcx;
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(files[0].clone(), window, cx)));
    draw(cx);
    let first = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let first = first.expect("a tab is shown");
    assert!(cx.update(|window, cx| first.focus_handle(cx).is_focused(window)));
    assert_eq!(first.read_with(cx, |e, _| e.vim_mode()), Some("normal"));

    // Opened from normal mode and escaped: back in the editor, in normal
    // mode, and j typed into the query rather than moving.
    run("workbench.open_recent", cx);
    cx.simulate_keystrokes("j");
    draw(cx);
    let query = workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.recent_picker().read(cx);
        picker.input().read(cx).text().to_string()
    });
    assert_eq!(query, "j");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(palette(&workbench, cx), None);
    assert!(cx.update(|window, cx| first.focus_handle(cx).is_focused(window)));
    assert_eq!(first.read_with(cx, |e, _| e.vim_mode()), Some("normal"));

    // A file chosen opens in normal mode.
    run("workbench.open_recent", cx);
    cx.simulate_input("_second");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let second = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let second = second.expect("a tab is shown");
    assert_ne!(second, first);
    assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)));
    assert_eq!(second.read_with(cx, |e, _| e.vim_mode()), Some("normal"));

    // A tab already open, left in insert or visual mode, is chosen from
    // the other in normal mode: it's normal again.
    for (key, mode) in [("i", "insert"), ("v", "visual")] {
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(second.read_with(cx, |e, _| e.vim_mode()), Some(mode));
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(files[0].clone(), window, cx)));
        draw(cx);
        assert!(cx.update(|window, cx| first.focus_handle(cx).is_focused(window)));
        assert_eq!(first.read_with(cx, |e, _| e.vim_mode()), Some("normal"));
        run("workbench.open_recent", cx);
        cx.simulate_input("_second");
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        let active = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        assert_eq!(active.as_ref(), Some(&second));
        assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)));
        assert_eq!(second.read_with(cx, |e, _| e.vim_mode()), Some("normal"));
    }
}
