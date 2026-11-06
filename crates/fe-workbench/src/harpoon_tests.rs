//! Harpoon's marks in a window: adding and jumping, the list picker's
//! moves and removal, and a rootless window's having none.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::harpoon::{MarkList, marks_file};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::picker::Mode;

use super::{HarpoonPicker, NO_FOLDER, set_state_dir};
use crate::{Launch, Picker, Workbench};

/// A window of a workbench rooted at `root`, keeping its lists under
/// `state`, active and drawn.
fn window(
    cx: &mut TestAppContext,
    root: Option<&Path>,
    state: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
    let state = state.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_state_dir(cx, state);
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

/// Runs command `id` as from the workbench.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Writes each of `files` in `dir`, holding its own name.
fn write(dir: &Path, files: &[&str]) -> Vec<PathBuf> {
    (files.iter())
        .map(|file| {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("{file}\n")).unwrap();
            path
        })
        .collect()
}

fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

/// The marks in `root`'s list file under `state`, from the root.
fn marks(state: &Path, root: &Path) -> Vec<String> {
    let (list, error) = MarkList::load(&marks_file(state, root));
    assert!(error.is_none(), "{error:?}");
    (list.files().iter())
        .map(|file| file.to_string_lossy().into_owned())
        .collect()
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

/// The toasts shown since the last call, dismissing them.
fn said(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    });
    run("toast.dismiss_all", cx);
    shown
}

fn list(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<HarpoonPicker>>> {
    workbench.read_with(cx, |workbench, cx| workbench.harpoon_list(cx))
}

/// The list picker's rows, each slot, file and whether it's missing, and
/// the selected row.
fn rows(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<(usize, String, bool)>, usize) {
    let picker = list(workbench, cx).expect("the list shows");
    picker.read_with(cx, |picker, _| {
        assert!(picker.delegate.loaded(), "the list is read");
        (picker.delegate.labels(), picker.selected())
    })
}

fn row(slot: usize, file: &str, missing: bool) -> (usize, String, bool) {
    (slot, file.to_owned(), missing)
}

#[gpui::test]
fn add_and_jump_open_the_marked_files(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = dir.path();
    let files = write(root, &["a.txt", "b.txt", "sub/c.txt"]);
    let (workbench, mut vcx) = window(cx, Some(root), state.path());
    let cx = &mut vcx;

    // Three marks, in the order added; a second add of one keeps one slot.
    for (file, said_then) in files.iter().zip([
        "Harpoon: marked a.txt as 1",
        "Harpoon: marked b.txt as 2",
        "Harpoon: marked sub/c.txt as 3",
    ]) {
        open(&workbench, file, cx);
        run("workbench.harpoon_add", cx);
        assert_eq!(said(&workbench, cx), [said_then]);
    }
    assert_eq!(marks(state.path(), root), ["a.txt", "b.txt", "sub/c.txt"]);
    open(&workbench, &files[0], cx);
    run("workbench.harpoon_add", cx);
    assert_eq!(said(&workbench, cx), ["Harpoon: a.txt is already 1"]);
    assert_eq!(marks(state.path(), root), ["a.txt", "b.txt", "sub/c.txt"]);

    // Jumping to 2 activates b's tab, open already: no second tab.
    assert_eq!(tabs(&workbench, cx).1.as_ref(), Some(&files[0]));
    run("workbench.harpoon_jump2", cx);
    assert_eq!(
        tabs(&workbench, cx),
        (files.clone(), Some(files[1].clone()))
    );

    // With its tab closed, it opens.
    run("workbench.close_tab", cx);
    assert!(!tabs(&workbench, cx).0.contains(&files[1]));
    run("workbench.harpoon_jump2", cx);
    let (opened, active) = tabs(&workbench, cx);
    assert_eq!(active.as_ref(), Some(&files[1]));
    assert_eq!(opened.iter().filter(|path| **path == files[1]).count(), 1);

    // A file that's gone is said, and stays marked; nothing opens.
    fs::remove_file(&files[2]).unwrap();
    let before = tabs(&workbench, cx);
    run("workbench.harpoon_jump3", cx);
    assert_eq!(said(&workbench, cx), ["Harpoon: file missing: sub/c.txt"]);
    assert_eq!(tabs(&workbench, cx), before);
    assert_eq!(marks(state.path(), root), ["a.txt", "b.txt", "sub/c.txt"]);

    // An empty slot is said.
    run("workbench.harpoon_jump4", cx);
    assert_eq!(said(&workbench, cx), ["Harpoon: nothing marked as 4"]);
    assert_eq!(tabs(&workbench, cx), before);

    // Removing the current file closes its gap.
    open(&workbench, &files[0], cx);
    run("workbench.harpoon_remove_current", cx);
    assert_eq!(said(&workbench, cx), ["Harpoon: unmarked a.txt"]);
    assert_eq!(marks(state.path(), root), ["b.txt", "sub/c.txt"]);
    run("workbench.harpoon_jump1", cx);
    assert_eq!(tabs(&workbench, cx).1.as_ref(), Some(&files[1]));
}

#[gpui::test]
fn the_list_picker_reorders_and_removes(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = dir.path();
    let files = write(root, &["a.txt", "b.txt", "sub/d.txt"]);
    let marked = ["a.txt", "b.txt", "sub/c.txt", "sub/d.txt"].map(PathBuf::from);
    let file = marks_file(state.path(), root);
    MarkList::from_files(marked).save(&file).unwrap();
    let (workbench, mut vcx) = window(cx, Some(root), state.path());
    let cx = &mut vcx;

    // The slots in order, a missing file's flagged.
    run("workbench.harpoon_toggle_list", cx);
    let (shown, selected) = rows(&workbench, cx);
    assert_eq!(
        shown,
        [
            row(1, "a.txt", false),
            row(2, "b.txt", false),
            row(3, "sub/c.txt", true),
            row(4, "sub/d.txt", false),
        ]
    );
    assert_eq!(selected, 0);

    // ctrl-j moves the selected row down, ctrl-k up, each written back,
    // the row staying selected.
    cx.simulate_keystrokes("ctrl-j");
    draw(cx);
    let (shown, selected) = rows(&workbench, cx);
    assert_eq!(shown[..2], [row(1, "b.txt", false), row(2, "a.txt", false)]);
    assert_eq!(selected, 1);
    assert_eq!(
        marks(state.path(), root),
        ["b.txt", "a.txt", "sub/c.txt", "sub/d.txt"]
    );
    cx.simulate_keystrokes("ctrl-j ctrl-j");
    draw(cx);
    assert_eq!(
        marks(state.path(), root),
        ["b.txt", "sub/c.txt", "sub/d.txt", "a.txt"]
    );
    assert_eq!(rows(&workbench, cx).1, 3);
    // At the end, nothing moves.
    cx.simulate_keystrokes("ctrl-j");
    draw(cx);
    assert_eq!(rows(&workbench, cx).1, 3);
    cx.simulate_keystrokes("ctrl-k");
    draw(cx);
    assert_eq!(
        marks(state.path(), root),
        ["b.txt", "sub/c.txt", "a.txt", "sub/d.txt"]
    );
    assert_eq!(rows(&workbench, cx).1, 2);

    // The remove key drops the selected row.
    cx.simulate_keystrokes("up ctrl-d");
    draw(cx);
    assert_eq!(marks(state.path(), root), ["b.txt", "a.txt", "sub/d.txt"]);
    let (shown, _) = rows(&workbench, cx);
    assert_eq!(
        shown,
        [
            row(1, "b.txt", false),
            row(2, "a.txt", false),
            row(3, "sub/d.txt", false),
        ]
    );

    // Filtered, a move still goes by the whole list's slots.
    cx.simulate_input("d.t");
    draw(cx);
    assert_eq!(rows(&workbench, cx).0, [row(3, "sub/d.txt", false)]);
    cx.simulate_keystrokes("ctrl-k");
    draw(cx);
    assert_eq!(marks(state.path(), root), ["b.txt", "sub/d.txt", "a.txt"]);
    assert_eq!(rows(&workbench, cx).0, [row(2, "sub/d.txt", false)]);

    // Enter opens the row's file, the list closing.
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(tabs(&workbench, cx).1.as_ref(), Some(&files[2]));

    // With vim on, `d d` in the picker's normal mode removes.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    run("workbench.harpoon_toggle_list", cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    let picker = list(&workbench, cx).expect("the list shows");
    assert_eq!(
        picker.read_with(cx, |picker, _| picker.mode()),
        Mode::Normal
    );
    cx.simulate_keystrokes("j d");
    draw(cx);
    assert_eq!(marks(state.path(), root), ["b.txt", "sub/d.txt", "a.txt"]);
    cx.simulate_keystrokes("d");
    draw(cx);
    assert_eq!(marks(state.path(), root), ["b.txt", "a.txt"]);
    assert_eq!(rows(&workbench, cx).1, 1);

    // The command hides it again.
    run("workbench.harpoon_toggle_list", cx);
    assert!(list(&workbench, cx).is_none());
}

#[gpui::test]
fn rootless_window_has_no_marks(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, None, state.path());
    let cx = &mut vcx;
    open(&workbench, &files[0], cx);
    assert_eq!(tabs(&workbench, cx).1.as_ref(), Some(&files[0]));

    for id in [
        "workbench.harpoon_add",
        "workbench.harpoon_toggle_list",
        "workbench.harpoon_jump1",
        "workbench.harpoon_remove_current",
    ] {
        run(id, cx);
        assert_eq!(said(&workbench, cx), [NO_FOLDER], "{id}");
        assert!(list(&workbench, cx).is_none(), "{id}");
    }
    assert_eq!(fs::read_dir(state.path()).unwrap().count(), 0);
}

/// Runs command `id` as from the workbench, leaving what it starts pending.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

#[gpui::test]
fn only_the_latest_jump_lands_and_only_if_nothing_moved(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = dir.path();
    let files = write(root, &["a.txt", "b.txt", "c.txt"]);
    let marked = ["a.txt", "b.txt", "c.txt"].map(PathBuf::from);
    MarkList::from_files(marked)
        .save(&marks_file(state.path(), root))
        .unwrap();
    let (workbench, mut vcx) = window(cx, Some(root), state.path());
    let cx = &mut vcx;
    for file in &files {
        open(&workbench, file, cx);
    }

    // Two jumps asked before either lands: the later wins, whichever's
    // read finishes last.
    dispatch("workbench.harpoon_jump3", cx);
    dispatch("workbench.harpoon_jump1", cx);
    draw(cx);
    assert_eq!(
        tabs(&workbench, cx),
        (files.clone(), Some(files[0].clone()))
    );

    // A tab chosen while a jump is pending stays chosen.
    dispatch("workbench.harpoon_jump2", cx);
    open(&workbench, &files[2], cx);
    assert_eq!(
        tabs(&workbench, cx),
        (files.clone(), Some(files[2].clone()))
    );

    // A jump with nothing in between lands.
    run("workbench.harpoon_jump2", cx);
    assert_eq!(
        tabs(&workbench, cx),
        (files.clone(), Some(files[1].clone()))
    );
}

#[gpui::test]
fn the_list_picker_checks_the_disk_on_enter(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = dir.path();
    let files = write(root, &["a.txt", "b.txt"]);
    let marked = ["a.txt", "b.txt"].map(PathBuf::from);
    MarkList::from_files(marked)
        .save(&marks_file(state.path(), root))
        .unwrap();
    let (workbench, mut vcx) = window(cx, Some(root), state.path());
    let cx = &mut vcx;
    run("workbench.harpoon_toggle_list", cx);
    assert_eq!(
        rows(&workbench, cx).0,
        [row(1, "a.txt", false), row(2, "b.txt", false)]
    );

    // Gone since the list was read: Enter says so, and flags its row; the
    // picker stays, and nothing opens.
    fs::remove_file(&files[0]).unwrap();
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(said(&workbench, cx), ["Harpoon: file missing: a.txt"]);
    assert_eq!(
        rows(&workbench, cx).0,
        [row(1, "a.txt", true), row(2, "b.txt", false)]
    );
    assert_eq!(tabs(&workbench, cx).0, Vec::<PathBuf>::new());

    // Back again: Enter opens it, and the picker closes.
    fs::write(&files[0], "a.txt\n").unwrap();
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(
        tabs(&workbench, cx),
        (vec![files[0].clone()], Some(files[0].clone()))
    );
}

#[cfg(unix)]
#[gpui::test]
fn marks_reach_a_tab_through_a_symlinked_root(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let real = dir.path().join("real");
    let files = write(&real, &["a.txt", "b.txt"]);
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let (workbench, mut vcx) = window(cx, Some(&alias), state.path());
    let cx = &mut vcx;

    // Tabs opened by the real path are marked from the root's.
    for file in &files {
        open(&workbench, file, cx);
        run("workbench.harpoon_add", cx);
    }
    assert_eq!(marks(state.path(), &alias), ["a.txt", "b.txt"]);

    // A jump activates the tab open under the other spelling.
    run("workbench.harpoon_jump1", cx);
    assert_eq!(
        tabs(&workbench, cx),
        (files.clone(), Some(files[0].clone()))
    );

    // So does Enter in the list.
    run("workbench.harpoon_toggle_list", cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert!(list(&workbench, cx).is_none());
    assert_eq!(
        tabs(&workbench, cx),
        (files.clone(), Some(files[1].clone()))
    );
}
