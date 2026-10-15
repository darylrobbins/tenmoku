use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;

use fe_project::fixture;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{click_row, open_root, panel};
use crate::Workbench;

/// A project of `fixture` files with these names and contents at its root.
const FILES: [(&str, &str); 3] = [
    ("a.txt", "alpha\n"),
    ("b.rs", "fn b() {}\n"),
    ("c.md", "# c\n"),
];

fn project() -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 1).unwrap();
    let paths = FILES
        .iter()
        .map(|(name, text)| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

/// The open tabs' paths, and the active tab's index.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<PathBuf>, Option<usize>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let paths = pane
            .tabs()
            .iter()
            .map(|tab| tab.path.clone().unwrap())
            .collect();
        (paths, pane.active())
    })
}

fn click_tab(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let bounds = workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).tab_bounds(ix).expect("drawn")
    });
    cx.simulate_click(bounds.center(), Modifiers::none());
}

#[gpui::test]
fn clicking_a_file_opens_a_focused_tab(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);

    // The click's events run the read to completion.
    click_row(&panel, "b.rs", cx);
    assert_eq!(tabs(&workbench, cx), (vec![paths[1].clone()], Some(0)));
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.model().path(), Some(paths[1].as_path()));
        assert_eq!(editor.model().text().to_string(), FILES[1].1);
    });
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused);
    assert_eq!(cx.window_title().as_deref(), Some("b.rs — fastedit"));
}

#[gpui::test]
fn opening_an_open_file_activates_its_tab(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "a.txt", cx);
    click_row(&panel, "b.rs", cx);
    assert_eq!(tabs(&workbench, cx), (paths[..2].to_vec(), Some(1)));

    click_row(&panel, "a.txt", cx);
    assert_eq!(tabs(&workbench, cx), (paths[..2].to_vec(), Some(0)));
}

#[gpui::test]
fn switching_and_closing_tabs(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    for (name, _) in FILES {
        click_row(&panel, name, cx);
    }
    assert_eq!(tabs(&workbench, cx), (paths.clone(), Some(2)));
    let active = |cx: &mut VisualTestContext| tabs(&workbench, cx).1;

    click_tab(&workbench, 0, cx);
    assert_eq!(active(cx), Some(0));
    cx.simulate_keystrokes("cmd-shift-]");
    assert_eq!(active(cx), Some(1));
    cx.simulate_keystrokes("cmd-shift-[ cmd-shift-[");
    assert_eq!(active(cx), Some(2), "wraps to the last tab");
    cx.simulate_keystrokes("cmd-shift-]");
    assert_eq!(active(cx), Some(0), "wraps to the first tab");

    // The middle tab closes to its right neighbour, the last to its left.
    click_tab(&workbench, 1, cx);
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(
        tabs(&workbench, cx),
        (vec![paths[0].clone(), paths[2].clone()], Some(1))
    );
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(tabs(&workbench, cx), (vec![paths[0].clone()], Some(0)));
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(tabs(&workbench, cx), (vec![], None));
    workbench.read_with(cx, |workbench, cx| {
        assert!(workbench.active_editor(cx).is_none(), "the empty pane");
    });
    assert_eq!(cx.window_title().as_deref(), Some("fastedit"));

    // A dirty tab stays open.
    click_row(&panel, "c.md", cx);
    cx.simulate_input("x");
    assert_eq!(cx.window_title().as_deref(), Some("c.md ● — fastedit"));
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(tabs(&workbench, cx), (vec![paths[2].clone()], Some(0)));
}

fn pane_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.focus_handle(cx).is_focused(window))
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
}

#[gpui::test]
fn keys_work_while_a_file_loads_or_failed(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "a.txt", cx);
    click_row(&panel, "b.rs", cx);

    // A read in progress shows in the tab's place, with the pane focused, and
    // the key is handled before the read lands.
    open_file(&workbench, &paths[2], cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    assert!(editor.is_none(), "loading");
    assert!(pane_focused(&workbench, cx));
    cx.simulate_keystrokes("cmd-shift-[");
    assert_eq!(
        tabs(&workbench, cx),
        (paths.clone(), Some(0)),
        "c.md lands unshown"
    );

    // So does a read error, here from reading a directory.
    open_file(&workbench, &dir.path().join(fixture::dir_name(0)), cx);
    cx.run_until_parked();
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    assert!(editor.is_none(), "the error shows");
    assert!(pane_focused(&workbench, cx));
    cx.simulate_keystrokes("cmd-shift-]");
    assert_eq!(tabs(&workbench, cx), (paths.clone(), Some(1)));
    assert!(!pane_focused(&workbench, cx), "the editor is focused again");
}

#[gpui::test]
fn the_latest_read_is_shown_whichever_lands_first(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let (a, b) = (&paths[0], &paths[1]);
    let buffer = |text: &str| Ok(Buffer::new(text).into());
    // (first to land, its result, second to land, its result), with a
    // opened before b; then the tabs and the active one's text.
    type Read = (usize, std::io::Result<crate::loader::Opened>);
    let cases: [([Read; 2], Vec<&PathBuf>, Option<&str>); 3] = [
        ([(0, buffer("a")), (1, buffer("b"))], vec![a, b], Some("b")),
        ([(1, buffer("b")), (0, buffer("a"))], vec![b, a], Some("b")),
        (
            [(1, Err(std::io::Error::other("no"))), (0, buffer("a"))],
            vec![a],
            None,
        ),
    ];
    for (reads, tabs_after, shown) in cases {
        let (workbench, cx) = open_root(cx, dir.path());
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.open(a.clone(), window, cx);
                pane.open(b.clone(), window, cx);
                // Landing by hand drops the reads `open` started.
                for (ix, result) in reads {
                    pane.file_read(paths[ix].clone(), result, window, cx);
                }
            })
        });
        cx.run_until_parked();
        let expected: Vec<PathBuf> = tabs_after.into_iter().cloned().collect();
        assert_eq!(tabs(&workbench, cx).0, expected);
        let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        let text = editor
            .map(|editor| editor.read_with(cx, |editor, _| editor.model().text().to_string()));
        assert_eq!(text.as_deref(), shown);
        if shown.is_some() {
            assert!(!pane_focused(&workbench, cx), "the editor is focused");
        } else {
            assert!(pane_focused(&workbench, cx));
        }
    }
}

#[gpui::test]
fn an_older_read_landing_in_an_empty_pane_is_focused(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let (a, b) = (&paths[0], &paths[1]);
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    // b, opened last, lands and is closed while a is still being read.
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open(a.clone(), window, cx);
            pane.open(b.clone(), window, cx);
            pane.file_read(b.clone(), Ok(Buffer::new("b").into()), window, cx);
            pane.close_active(window, cx);
            pane.file_read(a.clone(), Ok(Buffer::new("a").into()), window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(tabs(&workbench, cx), (vec![a.clone()], Some(0)));
    assert!(!pane_focused(&workbench, cx), "the editor is focused");
    cx.simulate_input("x");
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let text = editor
        .unwrap()
        .read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "xa");
}
