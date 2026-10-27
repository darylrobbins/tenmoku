//! `workbench.open_editor_at_index1` to `9` and
//! `workbench.last_editor_in_group` over the active pane's tabs.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::PaneView as _;

use crate::{Workbench, tests::open_root};

/// A valid 1 × 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38, 0x21, 0x27, 0xf7,
    0x1f, 0x00, 0x04, 0xba, 0x02, 0x04, 0x3e, 0x57, 0x1f, 0xc7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(command)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// A workbench with `a.txt`, `b.txt` and `c.txt` open, `c.txt` active.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), name).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for name in ["a.txt", "b.txt", "c.txt"] {
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).active().unwrap()
    })
}

#[gpui::test]
fn opens_the_nth_tab(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    for (command, expected) in [
        ("workbench.open_editor_at_index2", 1),
        ("workbench.open_editor_at_index1", 0),
        ("workbench.open_editor_at_index3", 2),
        ("workbench.open_editor_at_index2", 1),
        // Past the last tab: nothing.
        ("workbench.open_editor_at_index4", 1),
        ("workbench.open_editor_at_index9", 1),
    ] {
        run(command, cx);
        assert_eq!(active(&workbench, cx), expected, "{command}");
    }
    // The tab's editor has the focus.
    let focused = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.tabs()[1].editor().unwrap().focus_handle(cx)
    });
    cx.update(|window, _| assert!(focused.is_focused(window)));
}

#[gpui::test]
fn last_editor(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    run("workbench.open_editor_at_index1", cx);
    run("workbench.last_editor_in_group", cx);
    assert_eq!(active(&workbench, cx), 2);
    run("workbench.last_editor_in_group", cx);
    assert_eq!(active(&workbench, cx), 2);
}

/// A single tab is re-activated by Last Editor in Group, and its editor
/// regains the focus.
#[gpui::test]
fn last_editor_of_one_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let editor = pane.read_with(cx, |pane, cx| {
        assert_eq!(pane.tabs().len(), 1);
        pane.tabs()[0].editor().unwrap().focus_handle(cx)
    });
    // Focus moves off the editor, to a handle of its own.
    let elsewhere = cx.update(|_, cx| cx.focus_handle());
    cx.update(|window, cx| window.focus(&elsewhere, cx));
    draw(cx);
    cx.update(|window, _| assert!(!editor.is_focused(window)));
    run("workbench.last_editor_in_group", cx);
    assert_eq!(active(&workbench, cx), 0);
    cx.update(|window, _| assert!(editor.is_focused(window)));
}

/// The index counts the active pane's tabs: a preview tab is one and stays
/// a preview, an image viewer tab is one too, and the other pane is left
/// alone.
#[gpui::test]
fn follows_the_active_pane(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "c.txt", "d.txt"] {
        fs::write(dir.path().join(name), name).unwrap();
    }
    fs::write(dir.path().join("b.png"), PNG).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for name in ["a.txt", "b.png", "c.txt"] {
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    let path = dir.path().join("d.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open_preview(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);

    // The split's new pane, active, has one tab (a copy of the preview);
    // back to the first.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.split(crate::Direction::Right, window, cx)
        })
    });
    draw(cx);
    run("pane.focus_left", cx);
    assert_eq!(workbench.read_with(cx, |w, _| w.pane().clone()), pane);

    let state = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            workbench
                .panes(cx)
                .iter()
                .map(|pane| (pane.read(cx).tabs().len(), pane.read(cx).active()))
                .collect::<Vec<_>>()
        })
    };
    let before = state(cx);
    assert_eq!(before[0], (4, Some(3)));
    let other = before[1];

    // Index 2 is the image viewer, which has the focus.
    run("workbench.open_editor_at_index2", cx);
    assert_eq!(state(cx), [(4, Some(1)), other]);
    let viewer = pane.read_with(cx, |pane, cx| {
        pane.tabs()[1].image_viewer().unwrap().focus_handle(cx)
    });
    cx.update(|window, _| assert!(viewer.is_focused(window)));

    // Index 4 is the preview tab; it is still one.
    run("workbench.open_editor_at_index4", cx);
    assert_eq!(state(cx), [(4, Some(3)), other]);
    let previews = pane.read_with(cx, |pane, cx| {
        (pane.pane().tabs(cx).iter())
            .map(|tab| tab.is_preview())
            .collect::<Vec<_>>()
    });
    assert_eq!(previews, [false, false, false, true]);
}
