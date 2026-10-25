//! Vim's tab commands on the status bar's line: `:tabn`, `:tabp`,
//! `:tabfirst`, `:tablast`, `:tabnew`, `:tabc` and `:tabo` over the active
//! pane's editor tabs, as the tab commands run them.

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::super::super::vim_host_tests::{active, dirty, draw, ex, message, window, write};
use crate::Workbench;

/// Each pane's tabs, by file name.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
                    .collect()
            })
            .collect()
    })
}

/// The shown tab's file name.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        fe_gpui::pane_item::file_name(editor.model().path())
    })
}

/// Types `:{line}` and has it run, its message, if any, read.
fn line(line: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    ex(line, cx);
    draw(cx);
    message(workbench, cx).map(|message| message.to_string())
}

fn four(cx: &mut TestAppContext, root: &std::path::Path) -> (Entity<Workbench>, VisualTestContext) {
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"]);
    window(cx, root, &["a.txt", "b.txt", "c.txt", "d.txt"])
}

#[gpui::test]
fn cycle(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = four(cx, dir.path());
    let cx = &mut vcx;
    assert_eq!(shown(&workbench, cx), "d.txt");

    // Wraps, as workbench.next_tab does.
    assert_eq!(line("tabn", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "a.txt");
    assert_eq!(line("tabp", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "d.txt");
    line("tabp 2", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    line("tabn 3", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "c.txt");
    line("2tabn", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    // A whole turn lands where it began; a count past the end wraps.
    line("tabn 4", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "d.txt");
    line("tabp 6", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    line("tabN", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "a.txt");
    // `:tabn N` past the last goes to the last.
    line("tabn 9", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "d.txt");

    line("tabfirst", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "a.txt");
    line("tablast", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "d.txt");
    line("tabfir", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "a.txt");
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt", "d.txt"]]);
}

#[gpui::test]
fn new_close(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = four(cx, dir.path());
    let cx = &mut vcx;

    // An untitled tab after the active one, shown.
    line("tabfirst", &workbench, cx);
    assert_eq!(line("tabnew", &workbench, cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        [["a.txt", "untitled", "b.txt", "c.txt", "d.txt"]]
    );
    assert_eq!(shown(&workbench, cx), "untitled");

    // :tabc closes it, showing the one after.
    assert_eq!(line("tabc", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt", "d.txt"]]);

    // :tabnew {file} reads it as :e does.
    line("tabfirst", &workbench, cx);
    line("tabnew e.txt", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "e.txt");
    assert_eq!(
        tabs(&workbench, cx),
        [["a.txt", "e.txt", "b.txt", "c.txt", "d.txt"]]
    );
    assert!(tabs(&workbench, cx)[0].contains(&"e.txt".to_owned()));

    // A tab with changes stays open under :tabc, goes under :tabc!.
    cx.simulate_keystrokes("x");
    assert!(dirty(&active(&workbench, cx), cx));
    line("tabc", &workbench, cx);
    assert_eq!(shown(&workbench, cx), "e.txt");
    assert!(tabs(&workbench, cx)[0].contains(&"e.txt".to_owned()));
    line("tabc!", &workbench, cx);
    assert!(!tabs(&workbench, cx)[0].contains(&"e.txt".to_owned()));
    assert_eq!(tabs(&workbench, cx)[0].len(), 4);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("e.txt")).unwrap(),
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap()
    );

    // :tabo closes the others, but keeps one with changes.
    line("tabfirst", &workbench, cx);
    line("tabn 3", &workbench, cx);
    cx.simulate_keystrokes("x");
    line("tabfirst", &workbench, cx);
    let answer = line("tabo", &workbench, cx);
    assert_eq!(
        answer.as_deref(),
        Some("E445: Other tab page contains changes")
    );
    assert_eq!(tabs(&workbench, cx), [["a.txt", "c.txt"]]);
    assert_eq!(shown(&workbench, cx), "a.txt");
    line("tabo!", &workbench, cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
}

#[gpui::test]
fn pane_scope(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = four(cx, dir.path());
    let cx = &mut vcx;

    // Another pane's tabs are untouched.
    line("sp e.txt", &workbench, cx);
    let before = tabs(&workbench, cx);
    assert_eq!(before.len(), 2);
    line("tabnew", &workbench, cx);
    let after = tabs(&workbench, cx);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], ["e.txt", "untitled"]);
    line("tabo", &workbench, cx);
    assert_eq!(
        tabs(&workbench, cx),
        [before[0].clone(), vec!["untitled".to_owned()]]
    );

    // One tab: a message, not a panic, and the tab stays.
    assert_eq!(
        line("tabo", &workbench, cx).as_deref(),
        Some("Already only one tab page")
    );
    assert_eq!(line("tabn", &workbench, cx), None);
    assert_eq!(line("tabp", &workbench, cx), None);
    assert_eq!(line("tabfirst", &workbench, cx), None);
    assert_eq!(line("tablast", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx)[1], ["untitled"]);
}

#[gpui::test]
fn superseded_read_keeps_its_place(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = four(cx, dir.path());
    write(dir.path(), &["f.txt"]);
    let cx = &mut vcx;
    line("tabfirst", &workbench, cx);

    // Another file is opened before the first lands: both still come.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_after(dir.path().join("e.txt"), 1, window, cx);
            pane.open(dir.path().join("f.txt"), window, cx);
        })
    });
    draw(cx);
    assert_eq!(
        tabs(&workbench, cx),
        [["a.txt", "e.txt", "b.txt", "c.txt", "d.txt", "f.txt"]]
    );
    // The last open is the one shown.
    assert_eq!(shown(&workbench, cx), "f.txt");
}

#[gpui::test]
fn text_held_by_another_pane(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = four(cx, dir.path());
    let cx = &mut vcx;
    line("sp e.txt", &workbench, cx);
    let first = workbench.read_with(cx, |workbench, cx| workbench.panes(cx)[0].clone());
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.activate(first, window, cx))
    });
    draw(cx);
    line("tabfirst", &workbench, cx);
    line("tabnew e.txt", &workbench, cx);
    assert_eq!(
        tabs(&workbench, cx),
        [
            vec!["a.txt", "e.txt", "b.txt", "c.txt", "d.txt"],
            vec!["e.txt"]
        ]
    );
    assert_eq!(shown(&workbench, cx), "e.txt");
}

#[gpui::test]
fn open_waiting_behind_a_save_as(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = four(cx, dir.path());
    let cx = &mut vcx;
    // Another pane holds e.txt's text, so the open resumes at once.
    line("sp e.txt", &workbench, cx);
    let first = workbench.read_with(cx, |workbench, cx| workbench.panes(cx)[0].clone());
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.activate(first, window, cx))
    });
    draw(cx);
    line("tabfirst", &workbench, cx);

    let held = dir.path().join("e.txt");
    let reservation = crate::open::reserve(
        fe_buffer::Buffer::new("").id(),
        held.clone(),
        crate::open::file_identity(&held),
    );
    line("tabnew e.txt", &workbench, cx);
    assert_eq!(
        tabs(&workbench, cx)[0],
        ["a.txt", "b.txt", "c.txt", "d.txt"]
    );
    drop(reservation);
    draw(cx);
    assert_eq!(
        tabs(&workbench, cx)[0],
        ["a.txt", "e.txt", "b.txt", "c.txt", "d.txt"]
    );
    assert_eq!(shown(&workbench, cx), "e.txt");
}
