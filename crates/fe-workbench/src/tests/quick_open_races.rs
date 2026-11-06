//! Quick open across the transitions between a frame and the next: the
//! window deactivating with the palette open, Enter before a query's
//! results land, and results moving under a pressed row.

// What only its Unix tests use goes unused elsewhere.
#![cfg_attr(not(unix), allow(unused_imports))]

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
use std::{ffi::OsString, fs, path::PathBuf};

use fe_buffer::Buffer;
use fe_project::{FinderMatch, FsChange};
use gpui::{
    Entity, Focusable as _, Keystroke, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext,
};

use super::{open, panel};
use crate::{Launch, Picker, QuickOpen, Workbench};

/// A project with a few named files.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "cafe.rs", "src.rs", "lib.rs"] {
        fs::write(dir.path().join(name), "").unwrap();
    }
    dir
}

/// A workbench rooted at `root` with `root/a.txt` open, its editor
/// focused, and the window active.
fn workbench_at<'a>(
    cx: &'a mut TestAppContext,
    root: &std::path::Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((root.join("a.txt"), Some(Buffer::new("a").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (workbench, cx)
}

fn picker(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Picker<QuickOpen>> {
    workbench.read_with(cx, |workbench, _| workbench.quick_open().clone())
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.palette_open())
}

fn input_focused(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| picker.focus_handle(cx).is_focused(window))
}

fn paths(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> Vec<String> {
    picker.read_with(cx, |picker, _| {
        let matches = picker.delegate.matches();
        matches.iter().map(|found| found.path.clone()).collect()
    })
}

fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.tabs()
            .iter()
            .map(|tab| tab.path.clone().unwrap())
            .collect()
    })
}

fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active()?];
        tab.path.clone()
    })
}

/// Where row `ix` was drawn, near its start, as rows are only as wide as
/// their text.
fn row(picker: &Entity<Picker<QuickOpen>>, ix: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let bounds = picker.read_with(cx, |picker, _| picker.row_bounds(ix).expect("drawn"));
    bounds.origin + gpui::point(gpui::px(4.), bounds.size.height / 2.)
}

/// Presses each of `keys` without running the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    for key in keys.split(' ') {
        cx.update(|window, cx| {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
        });
    }
}

#[gpui::test]
fn deactivating_the_window_hands_focus_back(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = workbench_at(cx, dir.path());
    let picker = picker(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");
    assert!(shown(&workbench, cx));
    assert!(input_focused(&picker, cx));

    cx.deactivate_window();
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();

    // Typing reaches the editor focused before the palette opened.
    cx.simulate_input("x");
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let text = editor
        .unwrap()
        .read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "xa");
    // And Cmd-P opens the palette, focused, without a click.
    cx.simulate_keystrokes("cmd-p");
    assert!(shown(&workbench, cx));
    assert!(input_focused(&picker, cx));
}

#[gpui::test]
fn enter_before_the_results_land_opens_nothing(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, root);
    let picker = picker(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("rs");
    let rs = paths(&picker, cx);
    assert!(
        !rs.is_empty() && !rs.contains(&"a.txt".to_owned()),
        "{rs:?}"
    );

    // The rows are still `rs`'s: Enter is refused, the palette stays.
    press("cmd-backspace a . t x t enter", cx);
    assert!(shown(&workbench, cx));
    assert!(input_focused(&picker, cx));
    assert_eq!(tabs(&workbench, cx), [root.join("a.txt")]);

    // Once they land, Enter opens the input's match.
    cx.run_until_parked();
    assert_eq!(paths(&picker, cx)[0], "a.txt");
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("lib");
    assert_eq!(paths(&picker, cx), ["lib.rs"]);
    cx.simulate_keystrokes("enter");
    assert!(!shown(&workbench, cx));
    assert_eq!(active_path(&workbench, cx), Some(root.join("lib.rs")));
}

#[gpui::test]
fn reopening_shows_no_stale_rows(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, root);
    let picker = picker(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("lib");
    assert_eq!(paths(&picker, cx), ["lib.rs"]);
    cx.simulate_keystrokes("escape");

    press("cmd-p", cx);
    assert!(shown(&workbench, cx));
    assert_eq!(paths(&picker, cx), Vec::<String>::new());
    press("enter", cx);
    assert!(shown(&workbench, cx));
    assert_eq!(tabs(&workbench, cx), [root.join("a.txt")]);

    cx.run_until_parked();
    assert_eq!(paths(&picker, cx).len(), 4, "the empty query's rows");
}

#[gpui::test]
fn a_row_moved_under_the_mouse_isnt_opened(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, root);
    let picker = picker(&workbench, cx);
    let panel = panel(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("rs");
    let before = paths(&picker, cx);
    let pressed = row(&picker, 0, cx);
    cx.simulate_mouse_down(pressed, MouseButton::Left, Modifiers::none());

    // A file that ranks first pushes the pressed row down.
    fs::write(root.join("rs"), "").unwrap();
    let changes = vec![FsChange::Path(root.join("rs"))];
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    cx.run_until_parked();
    let after = paths(&picker, cx);
    assert_eq!(after[0], "rs", "{after:?}");
    assert_eq!(after[1], before[0]);
    assert!(row(&picker, 1, cx).y > pressed.y, "redrawn");

    cx.simulate_mouse_up(pressed, MouseButton::Left, Modifiers::none());
    assert!(shown(&workbench, cx), "neither file opened");
    assert_eq!(tabs(&workbench, cx), [root.join("a.txt")]);

    // A row that stays under the mouse opens the file pressed, though
    // the results changed.
    let pressed = row(&picker, 1, cx);
    cx.simulate_mouse_down(pressed, MouseButton::Left, Modifiers::none());
    fs::write(root.join("a_longer_name.rs"), "").unwrap();
    let changes = vec![FsChange::Path(root.join("a_longer_name.rs"))];
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    cx.run_until_parked();
    assert_eq!(paths(&picker, cx)[1], before[0], "still under the mouse");
    cx.simulate_mouse_up(pressed, MouseButton::Left, Modifiers::none());
    assert!(!shown(&workbench, cx));
    assert_eq!(active_path(&workbench, cx), Some(root.join(&before[0])));
}

/// A match for `caf<byte>.rs`, a name that isn't UTF-8 for bytes from
/// 0x80, all of which display as `caf\u{fffd}.rs`.
#[cfg(unix)]
fn not_utf8(byte: u8) -> FinderMatch {
    let relative = PathBuf::from(OsString::from_vec([b"caf", &[byte][..], b".rs"].concat()));
    FinderMatch {
        path: relative.to_string_lossy().into_owned(),
        relative,
        score: 1,
        indices: Vec::new(),
    }
}

/// Names that display alike once made UTF-8 are distinct rows. APFS
/// refuses such names, so they're given as matches.
#[gpui::test]
#[cfg(unix)]
fn names_alike_once_lossy_are_distinct_rows(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, root);
    let picker = picker(&workbench, cx);
    let show = |matches: Vec<FinderMatch>, cx: &mut VisualTestContext| {
        picker.update(cx, |picker, cx| {
            picker.delegate.show_matches(matches);
            picker.matches_updated(false, cx);
        });
        cx.run_until_parked();
    };
    let (x80, x81) = (not_utf8(0x80), not_utf8(0x81));
    assert_eq!(x80.path, x81.path);
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("caf");

    // One replaced by the other between mouse-down and mouse-up: neither
    // opens.
    show(vec![x80.clone()], cx);
    let pressed = row(&picker, 0, cx);
    cx.simulate_mouse_down(pressed, MouseButton::Left, Modifiers::none());
    show(vec![x81.clone()], cx);
    cx.simulate_mouse_up(pressed, MouseButton::Left, Modifiers::none());
    assert!(shown(&workbench, cx), "neither file opened");
    assert_eq!(tabs(&workbench, cx), [root.join("a.txt")]);

    // Both shown: a click opens the one clicked.
    show(vec![x80, x81.clone()], cx);
    let clicked = row(&picker, 1, cx);
    cx.simulate_click(clicked, Modifiers::none());
    assert!(!shown(&workbench, cx));
    assert_eq!(active_path(&workbench, cx), Some(root.join(&x81.relative)));
}
