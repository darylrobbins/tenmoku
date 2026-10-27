//! Files and folders dropped on the window, simulated as the platform's
//! `FileDropEvent`s over the test window.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_gpui::{EditorSettings, EditorTheme, set_vim_enabled};
use gpui::{
    Entity, ExternalPaths, FileDropEvent, Focusable as _, Pixels, Point, TestAppContext,
    VisualTestContext,
};

use crate::{Launch, Pane, RootChanged, Workbench};

/// A window of a workbench rooted at `root`, active and drawn.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
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
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
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

/// Drops `paths` at `position`: the drag enters, moves over, and is let go.
fn drop_at(paths: &[PathBuf], position: Point<Pixels>, cx: &mut VisualTestContext) {
    draw(cx);
    cx.simulate_mouse_move(position, None, gpui::Modifiers::none());
    let paths = ExternalPaths(paths.iter().cloned().collect());
    cx.simulate_event(FileDropEvent::Entered { position, paths });
    cx.simulate_event(FileDropEvent::Pending { position });
    draw(cx);
    cx.simulate_event(FileDropEvent::Submit { position });
    cx.simulate_event(FileDropEvent::Exited);
    draw(cx);
}

/// [`drop_at`] without letting the executor run: the drop is handled and
/// its reads are still in flight, for the test to step.
fn drop_unparked(
    workbench: &Entity<Workbench>,
    paths: &[PathBuf],
    position: Point<Pixels>,
    cx: &mut VisualTestContext,
) {
    draw(cx);
    let paths = ExternalPaths(paths.iter().cloned().collect());
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.drop_external(&paths, position, window, cx)
        })
    });
}

/// Steps the executor until `ready`, without letting it run to idle.
fn step_until(cx: &mut VisualTestContext, mut ready: impl FnMut(&mut VisualTestContext) -> bool) {
    for _ in 0..10_000 {
        if ready(cx) {
            return;
        }
        cx.executor().tick();
    }
    panic!("never ready");
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

/// The pane's tabs as file names, and its active tab's index.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> (Vec<String>, Option<usize>) {
    pane.read_with(cx, |pane, _| {
        let names = (pane.tabs().iter())
            .map(|tab| crate::pane::file_name(tab.path.as_deref()))
            .collect();
        (names, pane.active())
    })
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[gpui::test]
fn files(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    // a.txt in both panes, the left one active.
    let first = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| first.update(cx, |pane, cx| pane.open(paths[0].clone(), window, cx)));
    draw(cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.activate(left.clone(), window, cx)
        })
    });
    draw(cx);
    assert_eq!(tabs(&right, cx), (names(&["a.txt"]), Some(0)));

    let on_right = right.read_with(cx, |pane, _| pane.bounds().center());
    drop_at(
        &[paths[0].clone(), paths[1].clone(), paths[2].clone()],
        on_right,
        cx,
    );

    assert_eq!(
        tabs(&right, cx),
        (names(&["a.txt", "b.txt", "c.txt"]), Some(2))
    );
    assert_eq!(tabs(&left, cx), (names(&["a.txt"]), Some(0)));
    let active = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(active, right);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a tab is shown");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn folder(cx: &mut TestAppContext) {
    let (a, b, c) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let launch = write(a.path(), &["launch.txt"]);
    let inside = write(b.path(), &["inside.txt"]);
    let (workbench, mut vcx) = window(cx, a.path());
    let cx = &mut vcx;
    let changed = Rc::new(RefCell::new(Vec::new()));
    let _subscription = cx.update(|_, cx| {
        let changed = changed.clone();
        cx.subscribe(&workbench, move |_, RootChanged(root), _| {
            changed.borrow_mut().push(root.clone())
        })
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let centre = pane.read_with(cx, |pane, _| pane.bounds().center());

    // Folders first, a file after, and a path that isn't there.
    let missing = a.path().join("missing");
    let dropped = [
        missing,
        b.path().to_owned(),
        launch[0].clone(),
        c.path().to_owned(),
    ];
    drop_at(&dropped, centre, cx);
    assert_eq!(*changed.borrow(), [b.path().to_owned()]);
    let root = workbench.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned));
    assert_eq!(root.as_deref(), Some(b.path()));
    assert_eq!(tabs(&pane, cx), (names(&["launch.txt"]), Some(0)));

    // A folder and a file inside it.
    changed.borrow_mut().clear();
    drop_at(&[c.path().to_owned(), inside[0].clone()], centre, cx);
    assert_eq!(*changed.borrow(), [c.path().to_owned()]);
    assert_eq!(
        tabs(&pane, cx),
        (names(&["launch.txt", "inside.txt"]), Some(1))
    );
}

#[gpui::test]
fn vim(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let centre = pane.read_with(cx, |pane, _| pane.bounds().center());
    drop_at(&paths, centre, cx);

    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a tab is shown");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
}

#[gpui::test]
fn vim_open_file_in_insert_mode(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(paths[0].clone(), window, cx)));
    draw(cx);
    cx.simulate_keystrokes("i");
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a tab is shown");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("insert")
    );
    let centre = pane.read_with(cx, |pane, _| pane.bounds().center());
    drop_at(&paths, centre, cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}

#[gpui::test]
fn vim_each_dropped_file_in_normal_mode(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(paths[0].clone(), window, cx)));
    draw(cx);
    cx.simulate_keystrokes("i");
    let first = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let first = first.expect("a tab is shown");
    let centre = pane.read_with(cx, |pane, _| pane.bounds().center());
    drop_at(&paths, centre, cx);
    assert_eq!(
        first.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
    assert_eq!(tabs(&pane, cx), (names(&["a.txt", "b.txt"]), Some(1)));
}

#[gpui::test]
fn navigating_as_a_file_lands_ends_the_batch(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["x.txt", "a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(paths[0].clone(), window, cx)));
    draw(cx);
    let centre = pane.read_with(cx, |pane, _| pane.bounds().center());
    drop_unparked(&workbench, &paths[1..], centre, cx);
    // a.txt has landed; the batch hasn't resumed.
    step_until(cx, |cx| {
        pane.read_with(cx, |pane, _| pane.position(&paths[1]).is_some())
    });
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    assert_eq!(tabs(&pane, cx), (names(&["x.txt", "a.txt"]), Some(0)));
}

#[gpui::test]
fn overlapping_drops_sharing_a_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let centre = pane.read_with(cx, |pane, _| pane.bounds().center());
    drop_unparked(&workbench, &paths[..2], centre, cx);
    drop_unparked(&workbench, &paths[1..], centre, cx);
    draw(cx);
    // The later drop wins; b.txt is open once.
    let (names, active) = tabs(&pane, cx);
    assert_eq!(names.iter().filter(|name| *name == "b.txt").count(), 1);
    assert_eq!(names.last().map(String::as_str), Some("c.txt"));
    assert_eq!(active, Some(names.len() - 1));
}

#[gpui::test]
fn a_read_landing_after_the_pane_closed_keeps_the_focus(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let paths = write(dir.path(), &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    let first = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| first.update(cx, |pane, cx| pane.open(paths[0].clone(), window, cx)));
    draw(cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let on_right = right.read_with(cx, |pane, _| pane.bounds().center());
    drop_unparked(&workbench, &paths[1..], on_right, cx);
    // The drop made the right pane active; closing it leaves its read in
    // flight.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.close_active_pane(window, cx))
    });
    draw(cx);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&left));
    let active = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(active, left);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a tab is shown");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    // Only the tab the split copied: b.txt never landed.
    assert_eq!(tabs(&right, cx).0, names(&["a.txt"]));
}
