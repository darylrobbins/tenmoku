//! Hover highlights on the project tree's rows. A highlight shows only in a
//! frame drawn while the pointer is over the row, so a pointer crossing from
//! one row to another, or out of the list, must itself ask for a redraw:
//! these tests count the panel's notifications, which mark its window dirty
//! (a test app draws a dirty window when effects flush, and that frame takes
//! the highlight from where the pointer is), and never draw on their own.

use std::{cell::Cell, fs, path::Path, rc::Rc};

use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{
    Bounds, Entity, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px,
};

use crate::{Launch, ProjectPanel, Workbench};

/// A workbench rooted at `root` in a headless window, run until parked, and
/// its project panel.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<ProjectPanel>, &'a mut VisualTestContext) {
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
    cx.run_until_parked();
    let panel = workbench.read_with(cx, |workbench: &Workbench, _| workbench.panel().clone());
    (panel, cx)
}

/// Counts the panel's notifications from now on.
fn count_notifications(
    panel: &Entity<ProjectPanel>,
    cx: &mut VisualTestContext,
) -> Rc<Cell<usize>> {
    let count = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let count = count.clone();
        cx.observe(panel, move |_, _| count.set(count.get() + 1))
            .detach();
    });
    count
}

fn row_bounds(
    panel: &Entity<ProjectPanel>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"))
}

/// Moves the pointer to `position` and says whether that notified the panel.
fn move_to(position: Point<Pixels>, count: &Cell<usize>, cx: &mut VisualTestContext) -> bool {
    let before = count.get();
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.run_until_parked();
    count.get() > before
}

#[gpui::test]
fn hovering_across_rows_and_out_of_the_list_redraws(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), "").unwrap();
    }
    let (panel, cx) = open(cx, dir.path());
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.selected_row()),
        None,
        "no row is selected, so every row shows a hover highlight"
    );
    let first = row_bounds(&panel, 0, cx);
    let second = row_bounds(&panel, 1, cx);
    let list = panel.read_with(cx, |panel, _| panel.list_bounds());
    let count = count_notifications(&panel, cx);

    assert!(
        move_to(first.center(), &count, cx),
        "entering the first row highlights it"
    );
    assert!(
        !move_to(first.center() + point(px(10.), px(0.)), &count, cx),
        "moving within the row changes nothing"
    );
    assert!(
        move_to(second.center(), &count, cx),
        "crossing to the second row moves the highlight"
    );
    assert!(
        !move_to(second.center() - point(px(10.), px(0.)), &count, cx),
        "moving within the second row changes nothing"
    );
    let outside = point(list.right() + px(40.), second.center().y);
    assert!(!list.contains(&outside));
    assert!(
        move_to(outside, &count, cx),
        "leaving the list clears the highlight"
    );
    assert!(
        !move_to(outside + point(px(10.), px(0.)), &count, cx),
        "moving outside the list changes nothing"
    );
}
