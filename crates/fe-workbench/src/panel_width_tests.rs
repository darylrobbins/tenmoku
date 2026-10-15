//! The sidebar's six panels take the width their dock allocates, not a
//! fixed 240 px: wider than that they fill the column, narrower they
//! shrink with it, never reaching past it.

use std::fs;

use fe_buffer::Buffer;
use gpui::{Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px};
use kiln_shell::DockView as _;

use crate::{DockPanel, Launch, Workbench, tests::open};

/// A workbench showing `a.rs` from a temporary project, its editor focused.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

/// Draws a frame, then a few of those it asks for.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            return;
        }
    }
}

#[gpui::test]
fn panels_follow_the_dock_width(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let window = cx.update(|window, _| window.viewport_size());
    for width in [px(360.), px(180.)] {
        workbench.update(cx, |workbench, cx| {
            (workbench.dock()).update(cx, |dock, cx| {
                dock.dock_mut().set_size(width);
                cx.notify();
            })
        });
        draw(cx);
        for panel in [
            DockPanel::Project,
            DockPanel::Search,
            DockPanel::Outline,
            DockPanel::Changes,
            DockPanel::References,
            DockPanel::Problems,
        ] {
            workbench.update_in(cx, |workbench, window, cx| {
                (workbench.dock()).update(cx, |dock, cx| dock.show(panel, window, cx))
            });
            draw(cx);
            let column = workbench.read_with(cx, |workbench, cx| {
                workbench.dock().read(cx).state().drawn().column_bounds()
            });
            let column = column.expect("the dock shows");
            let drawn = cx.debug_bounds("sidebar-panel").expect("drawn");
            let what = format!("{panel:?} at {width:?}");
            assert_eq!(column.size.width, width, "{what}");
            assert_eq!(drawn.size.width, column.size.width, "{what}");
            assert!(
                drawn.left() >= column.left() && drawn.right() <= column.right(),
                "{what}: {drawn:?} outside {column:?}"
            );
            assert!(
                drawn.left() >= Pixels::ZERO && drawn.right() <= window.width,
                "{what}: {drawn:?} outside the window"
            );
        }
    }
}
