//! Which carets a References panel open leaves out of Back history: only
//! where an earlier open from the panel landed, in the same pane, not moved
//! since. A reveal that never landed (cancelled by a tab switch, a failed
//! read, or another open) isn't a landing, and neither is a landing the
//! caret left and came back to.

use std::{
    fs,
    path::{Path, PathBuf},
};

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::references_panel::{
    at, caret_on, click, click_now, focus_editor, opened, project, references, settle, shown,
};
use crate::{Pane, ReferencesPanel, Workbench, references_panel::ShownRow::File};

const MAIN: &str = "mod a;\n\nfn main() {\n    a::helper();\n}\n";
const A: &str = "pub fn helper() {}\n";

/// The results of shift-F12 on `helper` in main.rs.
struct Found {
    panel: Entity<ReferencesPanel>,
    /// Where shift-F12 was pressed.
    caret: usize,
    /// a.rs's reference's row, and the end of its range.
    a_row: usize,
    a_end: usize,
    main_row: usize,
}

/// Puts the caret on `helper` in main.rs and finds its references.
fn find(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Found {
    let caret = caret_on(workbench, "helper", cx);
    cx.simulate_keystrokes("shift-f12");
    settle(cx);
    let panel = references(workbench, cx);
    let rows = shown(&panel, cx);
    let row = |rel: &str| {
        let header = rows.iter().position(|row| *row == File(rel.into(), 1));
        header.expect("a header") + 1
    };
    let locations = panel.read_with(cx, |panel, _| panel.locations());
    let a = locations.iter().find(|l| l.rel == Path::new("src/a.rs"));
    Found {
        caret,
        a_row: row("src/a.rs"),
        a_end: a.expect("a.rs's reference").range.end,
        main_row: row("src/main.rs"),
        panel,
    }
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// The index of `path`'s tab in `pane`.
fn tab(pane: &Pane, path: &Path) -> usize {
    (pane.tabs().iter())
        .position(|tab| tab.path.as_deref() == Some(path))
        .expect("a tab")
}

fn set_cursor(workbench: &Entity<Workbench>, offset: usize, cx: &mut VisualTestContext) {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.expect("a file is shown").update(cx, |editor, cx| {
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
}

/// Goes back, returning the file and caret offset it went to.
fn back(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize) {
    focus_editor(workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(workbench, cx);
    (path, selection.end)
}

#[gpui::test]
fn revisiting_the_latest_landing_after_moving_away_is_recorded(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);

    click(&found.panel, found.a_row, cx);
    let (path, landed, focused) = at(&workbench, cx);
    assert_eq!(path, root.join("src/a.rs"));
    assert!(focused, "the editor has focus");
    // Away from the landing, then back to exactly what it selected.
    cx.simulate_keystrokes("right");
    settle(cx);
    assert_ne!(at(&workbench, cx).1, landed, "the caret moved");
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.unwrap().update(cx, |editor, cx| {
        editor.model_mut().select_range(landed.clone(), false);
        cx.notify();
    });
    assert_eq!(at(&workbench, cx).1, landed);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"));

    assert_eq!(back(&workbench, cx), (root.join("src/a.rs"), landed.end));
    assert_eq!(
        back(&workbench, cx),
        (root.join("src/main.rs"), found.caret)
    );
}

#[gpui::test]
fn a_reveal_cancelled_by_a_tab_switch_is_not_a_landing(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // a.rs's reference opens, and main.rs's tab replaces the read at once.
    let bounds = found
        .panel
        .read_with(cx, |panel, _| panel.row_bounds(found.a_row));
    cx.update(|window, cx| {
        click_now(bounds.expect("drawn").center(), window, cx);
        assert!(pane.read(cx).is_opening(&a));
        pane.update(cx, |pane, cx| {
            let ix = tab(pane, &main);
            pane.activate(ix, window, cx);
        });
    });
    settle(cx);
    // Read in the background, a.rs's tab doesn't show.
    assert_eq!(at(&workbench, cx).0, main);
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let ix = tab(pane, &a);
            pane.activate(ix, window, cx);
        });
    });
    settle(cx);
    set_cursor(&workbench, found.a_end, cx);
    assert_eq!(at(&workbench, cx).0, a);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (a, found.a_end));
    assert_eq!(back(&workbench, cx), (main, found.caret));
}

#[gpui::test]
fn a_reveal_whose_read_fails_is_not_a_landing(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // Reading a directory fails: the error shows in the editor's place.
    fs::remove_file(&a).unwrap();
    fs::create_dir(&a).unwrap();
    click(&found.panel, found.a_row, cx);
    let shown = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    assert!(shown.is_none(), "the read error shows");
    assert!(!pane.read_with(cx, |pane, _| pane.is_opening(&a)));

    // Restored, and opened plainly.
    fs::remove_dir(&a).unwrap();
    fs::write(&a, A).unwrap();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(a.clone(), window, cx)));
    settle(cx);
    assert_eq!(at(&workbench, cx).0, a);
    set_cursor(&workbench, found.a_end, cx);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (a, found.a_end));
    assert_eq!(back(&workbench, cx), (main, found.caret));
}

#[gpui::test]
fn a_reveal_superseded_by_a_plain_open_is_not_a_landing(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // While a.rs's reference is read, the same file is opened plainly.
    let bounds = found
        .panel
        .read_with(cx, |panel, _| panel.row_bounds(found.a_row));
    cx.update(|window, cx| click_now(bounds.expect("drawn").center(), window, cx));
    cx.update(|window, cx| {
        assert!(pane.read(cx).is_opening(&a));
        pane.update(cx, |pane, cx| pane.open(a.clone(), window, cx));
    });
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (a.clone(), 0..0));
    set_cursor(&workbench, found.a_end, cx);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (a, found.a_end));
    assert_eq!(back(&workbench, cx), (main, found.caret));
}

/// Shows `path`'s tab in `pane`.
fn show_tab(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let ix = tab(pane, path);
            pane.activate(ix, window, cx);
            cx.notify();
        })
    });
    settle(cx);
}

#[gpui::test]
fn returning_to_a_landing_by_its_tab_is_recorded(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // Landed, then main.rs's tab and a.rs's again, the selection untouched.
    click(&found.panel, found.a_row, cx);
    let (_, landed, _) = at(&workbench, cx);
    show_tab(&pane, &main, cx);
    show_tab(&pane, &a, cx);
    assert_eq!(at(&workbench, cx).1, landed);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (a, landed.end));
    assert_eq!(back(&workbench, cx), (main, found.caret));
}

#[gpui::test]
fn returning_to_a_landing_after_back_is_recorded(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // Landed, back to where shift-F12 was pressed, then a.rs's tab again.
    click(&found.panel, found.a_row, cx);
    let (_, landed, _) = at(&workbench, cx);
    assert_eq!(back(&workbench, cx), (main.clone(), found.caret));
    show_tab(&pane, &a, cx);
    assert_eq!(at(&workbench, cx).1, landed);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    // The first back already took where shift-F12 was pressed.
    assert_eq!(back(&workbench, cx), (a, landed.end));
}

/// Moves `pane`'s tab `ix` to before tab `index`, or else to the end, as
/// dragging it in the tab bar does.
fn reorder(pane: &Entity<Pane>, ix: usize, index: Option<usize>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.reorder(ix, index, window, cx)));
    settle(cx);
}

#[gpui::test]
fn returning_to_a_landing_by_dragging_tabs_is_recorded(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // [main.rs, a.rs], a.rs landed. main.rs dragged to the end shows it;
    // a.rs dragged to the end then shows a.rs again, the selection untouched.
    click(&found.panel, found.a_row, cx);
    let (_, landed, _) = at(&workbench, cx);
    assert_eq!(pane.read_with(cx, |pane, _| tab(pane, &a)), 1);
    reorder(&pane, 0, None, cx);
    assert_eq!(at(&workbench, cx).0, main);
    reorder(&pane, 0, None, cx);
    assert_eq!(at(&workbench, cx), (a.clone(), landed.clone(), true));
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (a, landed.end));
    assert_eq!(back(&workbench, cx), (main, found.caret));
}

#[gpui::test]
fn reordering_a_landed_tab_keeps_the_landing(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let pane = pane(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // a.rs, landed and shown, moves before main.rs and stays shown.
    click(&found.panel, found.a_row, cx);
    let (_, landed, _) = at(&workbench, cx);
    reorder(&pane, 1, Some(0), cx);
    assert_eq!(pane.read_with(cx, |pane, _| tab(pane, &a)), 0);
    assert_eq!(at(&workbench, cx), (a, landed, true));
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (main, found.caret));
}

/// Selects `range` in the active editor, reversed or not, notifying only
/// if `notify`.
fn select(
    workbench: &Entity<Workbench>,
    range: std::ops::Range<usize>,
    reversed: bool,
    notify: bool,
    cx: &mut VisualTestContext,
) {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.expect("a file is shown").update(cx, |editor, cx| {
        editor.model_mut().select_range(range, reversed);
        if notify {
            cx.notify();
        }
    });
}

#[gpui::test]
fn reversing_a_landed_selection_is_recorded(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    // The same range, its caret at the start now: unseen until the open.
    click(&found.panel, found.a_row, cx);
    let (_, landed, _) = at(&workbench, cx);
    assert!(!landed.is_empty());
    select(&workbench, landed.clone(), true, false, cx);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let (path, selection, _) = at(&workbench, cx);
    assert_eq!((path, selection), (a, landed.start..landed.start));
}

#[gpui::test]
fn reversing_a_landed_selection_and_back_is_recorded(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/a.rs", A), ("src/main.rs", MAIN)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let found = find(&workbench, cx);
    let (a, main) = (root.join("src/a.rs"), root.join("src/main.rs"));

    click(&found.panel, found.a_row, cx);
    let (_, landed, _) = at(&workbench, cx);
    select(&workbench, landed.clone(), true, true, cx);
    settle(cx);
    select(&workbench, landed.clone(), false, true, cx);
    settle(cx);
    click(&found.panel, found.main_row, cx);
    assert_eq!(at(&workbench, cx).0, main);

    assert_eq!(back(&workbench, cx), (a, landed.end));
    assert_eq!(back(&workbench, cx), (main, found.caret));
}
