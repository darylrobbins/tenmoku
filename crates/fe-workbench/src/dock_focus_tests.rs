//! Vim's window keys at the panes' edge focus the dock shown on that side:
//! the sidebar's shown view or the terminal, wherever they have been
//! moved; a hidden dock stays hidden, a neighbouring pane comes first, and
//! with vim off the keys stop at the edge.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{commands::RunCommand, set_vim_enabled};
use gpui::{Entity, EntityId, FocusHandle, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench, tests::open};

/// A workbench showing `a.go` from a temporary project, its editor
/// focused, with vim on or off, whose terminal runs `/bin/sh`.
fn opened(
    vim: bool,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    // The pty's threads wake the terminal's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.go");
    fs::write(&path, "package a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("package a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|_, cx| set_vim_enabled(cx, vim));
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    focus_editor(&workbench, cx);
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

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn focus_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert!(editor_focused(workbench, cx));
}

fn focused(focus: &FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().focus_handle(cx)
    });
    focused(&editor, cx)
}

/// The project tree's, the outline's, search's and the Changes panel's
/// focus.
fn sidebar_views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> [FocusHandle; 4] {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        [
            dock.project().focus_handle(cx),
            dock.outline().focus_handle(cx),
            dock.search().focus_handle(cx),
            dock.changes().focus_handle(cx),
        ]
    })
}

/// The active editor, which is each pane's own.
fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> EntityId {
    workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().entity_id()
    })
}

fn terminal_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| workbench.terminal().focus_handle(cx))
}

fn sidebar_visible(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).is_visible())
}

fn terminal_visible(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        workbench.terminal_dock().read(cx).is_visible()
    })
}

/// Hides the sidebar if it shows.
fn hide_sidebar(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    if sidebar_visible(workbench, cx) {
        run("dock.toggle", cx);
    }
    assert!(!sidebar_visible(workbench, cx));
}

/// Shows the sidebar with each of the project tree, the outline (the
/// tree collapsed over it), search and the Changes panel in turn and,
/// from the editor, presses `keys`, which must focus it.
fn each_view_answers(keys: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let [project, outline, search, changes] = sidebar_views(workbench, cx);
    let explorer = workbench.read_with(cx, |workbench, cx| {
        workbench
            .dock()
            .read(cx)
            .explorer()
            .read(cx)
            .stack()
            .clone()
    });
    let collapse_folders = |collapsed: bool, cx: &mut VisualTestContext| {
        explorer.update(cx, |stack, cx| {
            stack.set_collapsed(crate::project_panel::FOLDERS, collapsed, cx);
        });
        draw(cx);
    };
    for (show, view) in [
        ("dock.show_project", &project),
        ("outline.show", &outline),
        ("search.show", &search),
        ("git.show_changes", &changes),
    ] {
        run(show, cx);
        if show == "outline.show" {
            collapse_folders(true, cx);
        }
        focus_editor(workbench, cx);
        press(keys, cx);
        assert!(focused(view, cx), "{keys} after {show}");
        if show == "outline.show" {
            collapse_folders(false, cx);
        }
    }
}

#[gpui::test]
fn sidebar_side(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(true, cx);

    // On the right, as it starts.
    each_view_answers("ctrl-w l", &workbench, cx);
    focus_editor(&workbench, cx);
    press("ctrl-w h", cx);
    assert!(editor_focused(&workbench, cx), "nothing on the left");

    run("dock.move_sidebar_left", cx);
    each_view_answers("ctrl-w h", &workbench, cx);
    focus_editor(&workbench, cx);
    press("ctrl-w l", cx);
    assert!(editor_focused(&workbench, cx), "nothing on the right");
}

#[gpui::test]
fn bottom(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(true, cx);
    let terminal = terminal_focus(&workbench, cx);
    run("terminal.toggle", cx);
    assert!(terminal_visible(&workbench, cx));

    focus_editor(&workbench, cx);
    press("ctrl-w j", cx);
    assert!(focused(&terminal, cx), "the terminal below");
    focus_editor(&workbench, cx);
    press("ctrl-w k", cx);
    assert!(editor_focused(&workbench, cx), "nothing above");

    run("terminal.move_to_left", cx);
    focus_editor(&workbench, cx);
    press("ctrl-w h", cx);
    assert!(focused(&terminal, cx), "the terminal on the left");
    focus_editor(&workbench, cx);
    press("ctrl-w j", cx);
    assert!(editor_focused(&workbench, cx), "nothing below");

    // The sidebar gives way to the left, hidden, so it doesn't answer.
    hide_sidebar(&workbench, cx);
    run("terminal.move_to_right", cx);
    focus_editor(&workbench, cx);
    press("ctrl-w l", cx);
    assert!(focused(&terminal, cx), "the terminal on the right");
    focus_editor(&workbench, cx);
    press("ctrl-w h", cx);
    assert!(
        editor_focused(&workbench, cx),
        "the sidebar on the left is hidden"
    );
    press("ctrl-w j", cx);
    assert!(editor_focused(&workbench, cx), "nothing below");
}

#[gpui::test]
fn hidden_and_inner(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(true, cx);

    // Hidden docks stay hidden.
    hide_sidebar(&workbench, cx);
    assert!(!terminal_visible(&workbench, cx));
    press("ctrl-w l", cx);
    assert!(editor_focused(&workbench, cx));
    assert!(!sidebar_visible(&workbench, cx));
    press("ctrl-w j", cx);
    assert!(editor_focused(&workbench, cx));
    assert!(!terminal_visible(&workbench, cx));

    // The Explorer with both its views collapsed has nothing to focus.
    run("dock.show_project", cx);
    let explorer = workbench.read_with(cx, |workbench, cx| {
        workbench
            .dock()
            .read(cx)
            .explorer()
            .read(cx)
            .stack()
            .clone()
    });
    explorer.update(cx, |stack, cx| {
        stack.set_collapsed(crate::project_panel::FOLDERS, true, cx);
        stack.set_collapsed(crate::outline_panel::VIEW, true, cx);
    });
    focus_editor(&workbench, cx);
    press("ctrl-w l", cx);
    assert!(editor_focused(&workbench, cx), "all collapsed");
    explorer.update(cx, |stack, cx| {
        stack.set_collapsed(crate::project_panel::FOLDERS, false, cx);
    });

    // A neighbour comes first.
    let [project, ..] = sidebar_views(&workbench, cx);
    press("ctrl-w v", cx);
    press("ctrl-w h", cx);
    let left = active_editor(&workbench, cx);
    press("ctrl-w l", cx);
    assert!(editor_focused(&workbench, cx), "the right pane");
    let right = active_editor(&workbench, cx);
    assert_ne!(left, right);
    assert!(!focused(&project, cx));
    press("ctrl-w l", cx);
    assert!(focused(&project, cx), "the sidebar past the right pane");

    run("terminal.toggle", cx);
    let terminal = terminal_focus(&workbench, cx);
    focus_editor(&workbench, cx);
    press("ctrl-w s", cx);
    press("ctrl-w k", cx);
    press("ctrl-w j", cx);
    assert!(editor_focused(&workbench, cx), "the pane below");
    assert!(!focused(&terminal, cx));
    press("ctrl-w j", cx);
    assert!(focused(&terminal, cx), "the terminal past the bottom pane");
}

/// `secondary-k secondary-left`, ctrl-k ctrl-left off macOS, is cmd-k
/// cmd-left on the test platform.
#[gpui::test]
fn vim_off(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(false, cx);
    run("terminal.toggle", cx);
    run("dock.move_sidebar_left", cx);
    run("dock.show_project", cx);
    assert!(sidebar_visible(&workbench, cx) && terminal_visible(&workbench, cx));
    let [project, ..] = sidebar_views(&workbench, cx);

    // The keys walk the panes.
    focus_editor(&workbench, cx);
    let left = active_editor(&workbench, cx);
    press("cmd-\\", cx);
    assert_ne!(active_editor(&workbench, cx), left);
    press("cmd-k cmd-left", cx);
    assert_eq!(active_editor(&workbench, cx), left);
    assert!(editor_focused(&workbench, cx));

    // But stop at the edge.
    press("cmd-k cmd-left", cx);
    assert!(editor_focused(&workbench, cx), "stops at the left edge");
    assert!(!focused(&project, cx));

    run("dock.move_sidebar_right", cx);
    focus_editor(&workbench, cx);
    press("cmd-k cmd-right cmd-k cmd-right", cx);
    assert_ne!(active_editor(&workbench, cx), left);
    assert!(editor_focused(&workbench, cx), "stops at the right edge");
    assert!(!focused(&project, cx));
}
