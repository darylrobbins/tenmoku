//! A file clicked once in the project panel opens in the pane's preview
//! tab, in italics, which the next one clicked replaces; a double-click on
//! its row, an edit, or turning `workbench.preview_tabs` off opens it to
//! stay. A read for a preview that something has superseded by the time it
//! lands is dropped, and one opened to stay is never made a preview.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::Settings;
use fe_gpui::{SettingsEvent, commands::RunCommand};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext,
    VisualTestContext,
};
use kiln_shell::PaneView as _;

use super::Pane;
use crate::{Launch, Workbench, loader};

fn project(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for name in files {
        std::fs::write(root.join(name), format!("{name}\n")).unwrap();
    }
    (dir, root)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    // On by default, but the test window turns it off.
    workbench.update(cx, |workbench, _| workbench.set_preview_tabs(true));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Each tab's file name, with `*`s round it if it's drawn in italics, and
/// the active tab's index.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (Vec<String>, Option<usize>) {
    let pane = pane(workbench, cx);
    cx.update(|_, cx| {
        let pane = pane.read(cx);
        let drawn = pane.pane().tabs(cx);
        let names = (pane.tabs().iter().zip(drawn))
            .map(|(tab, drawn)| {
                let path = tab.path.as_deref().expect("a file's tab");
                let name = path.file_name().unwrap().to_string_lossy();
                if drawn.is_preview() {
                    format!("*{name}*")
                } else {
                    name.into_owned()
                }
            })
            .collect();
        (names, pane.active())
    })
}

/// Clicks the project panel's row named `name` `clicks` times, as one
/// click, a double-click, and so on.
fn click_row(workbench: &Entity<Workbench>, name: &str, clicks: usize, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let ix = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        (tree.rows(0..tree.row_count()))
            .position(|row| row.name.to_string_lossy() == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    });
    let at = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn").center());
    for click_count in 1..=clicks {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
}

#[gpui::test]
fn a_click_opens_a_preview_that_the_next_click_replaces(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt", "c.txt"]);
    let (workbench, cx) = open_root(cx, &root);

    click_row(&workbench, "a.txt", 1, cx);
    assert_eq!(tabs(&workbench, cx), (vec!["*a.txt*".into()], Some(0)));

    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["*b.txt*".into()], Some(0)),
        "b replaces a"
    );
    let editors = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(editors, 1, "a's editor is closed");

    // Clicked again, the preview just shows.
    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(tabs(&workbench, cx), (vec!["*b.txt*".into()], Some(0)));
}

#[gpui::test]
fn a_double_click_opens_a_file_to_stay(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt", "c.txt"]);
    let (workbench, cx) = open_root(cx, &root);

    click_row(&workbench, "a.txt", 2, cx);
    assert_eq!(tabs(&workbench, cx), (vec!["a.txt".into()], Some(0)));

    // A preview, kept by a double-click once it has landed.
    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["a.txt".into(), "*b.txt*".into()], Some(1))
    );
    click_row(&workbench, "b.txt", 2, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["a.txt".into(), "b.txt".into()], Some(1))
    );

    click_row(&workbench, "c.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (
            vec!["a.txt".into(), "b.txt".into(), "*c.txt*".into()],
            Some(2)
        )
    );
}

#[gpui::test]
fn an_edit_keeps_the_preview(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (workbench, cx) = open_root(cx, &root);

    click_row(&workbench, "a.txt", 1, cx);
    let editor = pane(&workbench, cx).read_with(cx, |pane, _| pane.active_editor().cloned());
    let editor = editor.expect("a's editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    // Typed or, with vim on, deleted: an edit either way.
    cx.simulate_input("x");
    draw(cx);
    assert!(editor.read_with(cx, |editor, _| editor.model().is_dirty()));
    assert_eq!(tabs(&workbench, cx), (vec!["a.txt".into()], Some(0)));

    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["a.txt".into(), "*b.txt*".into()], Some(1)),
        "the edited file stays open"
    );
}

#[gpui::test]
fn keep_editor_keeps_the_preview(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (workbench, cx) = open_root(cx, &root);

    click_row(&workbench, "a.txt", 1, cx);
    let run = RunCommand(
        Invocation::of("workbench.keep_editor")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    assert_eq!(tabs(&workbench, cx), (vec!["a.txt".into()], Some(0)));

    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["a.txt".into(), "*b.txt*".into()], Some(1))
    );
}

#[gpui::test]
fn with_preview_tabs_off_a_click_opens_a_file_to_stay(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let mut settings = Settings::default();
    settings.workbench.preview_tabs = false;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));

    click_row(&workbench, "a.txt", 1, cx);
    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["a.txt".into(), "b.txt".into()], Some(1))
    );
}

/// Opens each of `opens` in the active pane, `true` for a look, and cancels
/// the reads they start, for the test to land with [`land`] in its order.
fn open_held(workbench: &Entity<Workbench>, opens: &[(&Path, bool)], cx: &mut VisualTestContext) {
    let pane = pane(workbench, cx);
    open_held_in(&pane, opens, cx);
}

/// [`open_held`] in `pane`.
fn open_held_in(pane: &Entity<Pane>, opens: &[(&Path, bool)], cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            for &(path, preview) in opens {
                if preview {
                    pane.open_preview(path.to_owned(), window, cx);
                } else {
                    pane.open(path.to_owned(), window, cx);
                }
            }
            pane.reads.clear();
        })
    });
}

/// Lands the read of `path` in the active pane.
fn land(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = pane(workbench, cx);
    land_in(&pane, path, cx);
}

/// [`land`] in `pane`.
fn land_in(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) {
    let result = loader::open(path.to_owned());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.file_read(path.to_owned(), result, window, cx)
        })
    });
    draw(cx);
}

/// The file of the editor that has focus, if one has.
fn focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let pane = pane(workbench, cx);
    cx.update(|window, cx| {
        let editor = pane.read(cx).active_editor()?.clone();
        let editor = editor.read(cx);
        (editor.focus_handle(cx).is_focused(window)).then(|| {
            let path = editor.model().path().unwrap();
            path.file_name().unwrap().to_string_lossy().into_owned()
        })
    })
}

#[gpui::test]
fn a_superseded_preview_read_is_dropped_whichever_lands_first(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    for first in [&a, &b] {
        let mut cx = cx.clone();
        let (workbench, cx) = open_root(&mut cx, &root);
        open_held(&workbench, &[(&a, true), (&b, true)], cx);
        let second = if first == &a { &b } else { &a };
        land(&workbench, first, cx);
        land(&workbench, second, cx);
        assert_eq!(
            tabs(&workbench, cx),
            (vec!["*b.txt*".into()], Some(0)),
            "{} landing first",
            first.display()
        );
        assert_eq!(focused(&workbench, cx).as_deref(), Some("b.txt"));
    }
}

#[gpui::test]
fn a_preview_read_is_dropped_once_a_tab_is_shown(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "x.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    click_row(&workbench, "x.txt", 2, cx);
    open_held(&workbench, &[(&root.join("a.txt"), true)], cx);
    let pane = pane(&workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    land(&workbench, &root.join("a.txt"), cx);
    assert_eq!(tabs(&workbench, cx), (vec!["x.txt".into()], Some(0)));
    assert_eq!(focused(&workbench, cx).as_deref(), Some("x.txt"));
}

#[gpui::test]
fn an_open_to_stay_during_a_read_is_for_good(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let a = root.join("a.txt");
    open_held(&workbench, &[(&a, true), (&a, false), (&a, true)], cx);
    land(&workbench, &a, cx);
    assert_eq!(tabs(&workbench, cx), (vec!["a.txt".into()], Some(0)));

    click_row(&workbench, "b.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["a.txt".into(), "*b.txt*".into()], Some(1))
    );
}

#[gpui::test]
fn a_preview_waits_behind_a_save_as_as_a_preview(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt", "c.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let held = |name: &str| {
        let path = root.join(name);
        let identity = crate::open::file_identity(&path);
        crate::open::reserve(fe_buffer::Buffer::new("").id(), path, identity)
    };

    let saving = held("a.txt");
    click_row(&workbench, "a.txt", 1, cx);
    assert_eq!(tabs(&workbench, cx), (vec![], None), "waiting");
    drop(saving);
    draw(cx);
    assert_eq!(tabs(&workbench, cx), (vec!["*a.txt*".into()], Some(0)));

    // Kept by a double-click while it waits, it opens to stay.
    let saving = held("b.txt");
    click_row(&workbench, "b.txt", 1, cx);
    click_row(&workbench, "b.txt", 2, cx);
    drop(saving);
    draw(cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["*a.txt*".into(), "b.txt".into()], Some(1))
    );
    click_row(&workbench, "c.txt", 1, cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec!["*c.txt*".into(), "b.txt".into()], Some(0))
    );
}

/// Each of `pane`'s tabs' file names, `*`s round a preview's.
fn pane_tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let pane = pane.read(cx);
        let drawn = pane.pane().tabs(cx);
        (pane.tabs().iter().zip(drawn))
            .map(|(tab, drawn)| {
                let name = tab.path.as_deref().unwrap().file_name().unwrap();
                let name = name.to_string_lossy();
                if drawn.is_preview() {
                    format!("*{name}*")
                } else {
                    name.into_owned()
                }
            })
            .collect()
    })
}

#[gpui::test]
fn an_open_to_stay_pending_is_for_good_when_another_pane_has_the_file(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let first = pane(&workbench, cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.split(crate::Direction::Right, window, cx)
        })
    });
    let second = pane(&workbench, cx);
    assert_ne!(first, second);

    // Opened to stay in the second pane, while the first opens it too.
    open_held_in(&second, &[(&a, false)], cx);
    cx.update(|window, cx| first.update(cx, |pane, cx| pane.open(a.clone(), window, cx)));
    draw(cx);
    assert_eq!(pane_tabs(&first, cx), ["a.txt"]);

    // A click on it in the second pane shares the first's buffer.
    cx.update(|window, cx| second.update(cx, |pane, cx| pane.open_preview(a.clone(), window, cx)));
    draw(cx);
    assert_eq!(pane_tabs(&second, cx), ["a.txt"], "still to stay");
    land_in(&second, &a, cx);
    cx.update(|window, cx| second.update(cx, |pane, cx| pane.open_preview(b.clone(), window, cx)));
    draw(cx);
    assert_eq!(pane_tabs(&second, cx), ["a.txt", "*b.txt*"]);
}

#[gpui::test]
fn a_file_the_host_is_reading_opens_to_stay(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let pane = pane(&workbench, cx);
    pane.update(cx, |pane, cx| pane.opening(a.clone(), cx));
    open_held_in(&pane, &[(&a, true)], cx);
    land_in(&pane, &a, cx);
    assert_eq!(pane_tabs(&pane, cx), ["a.txt"]);

    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open_preview(b.clone(), window, cx)));
    draw(cx);
    assert_eq!(pane_tabs(&pane, cx), ["a.txt", "*b.txt*"]);
}
