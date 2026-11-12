//! Vim's window keys back out of a dock, pressed through the default
//! keymap: to the editor pane last active, toward the panes; between the
//! sidebar and the terminal's dock below it; nowhere at the window's edge.
//! Escape in a list goes to the editor too, but not from an input or the
//! commit box.

use std::{fs, path::Path, process::Command};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{commands::RunCommand, set_vim_enabled};
use gpui::{Entity, EntityId, FocusHandle, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench, tests::open};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A workbench showing `a.go` from a temporary repository, where `a.go`
/// is changed, its editor focused, with vim on, whose terminal runs
/// `/bin/sh`.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    // The pty's threads wake the terminal's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = root.join("a.go");
    fs::write(&path, "package a\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(&path, "package a\n\nvar x = 1\n").unwrap();
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((path, Some(Buffer::new("package a\n\nvar x = 1\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    // Focus listeners hear nothing until the window is active.
    cx.update(|window, _| window.activate_window());
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    // Written for a sidebar on the right, `workbench.rail`'s old default.
    workbench.update(cx, |workbench, cx| {
        workbench.move_sidebar(kiln_shell::DockPosition::Right, cx)
    });
    focus(
        &workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx)),
        cx,
    );
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

fn focus(focus: &FocusHandle, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(focus, cx));
    draw(cx);
}

fn focused(focus: &FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

/// The active editor, which is each pane's own.
fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> EntityId {
    workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().entity_id()
    })
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().focus_handle(cx)
    });
    focused(&editor, cx)
}

fn project(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).project().focus_handle(cx)
    })
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    workbench.read_with(cx, |workbench, cx| workbench.terminal().focus_handle(cx))
}

/// Shows the project tree and focuses it.
fn focus_tree(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> FocusHandle {
    run("dock.show_project", cx);
    let tree = project(workbench, cx);
    focus(&tree, cx);
    assert!(focused(&tree, cx));
    tree
}

#[gpui::test]
fn to_editor(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);

    // Two panes, the right one active: the tree goes back to it, not to
    // the first.
    let left = active_editor(&workbench, cx);
    press("ctrl-w v", cx);
    let right = active_editor(&workbench, cx);
    assert_ne!(left, right);
    let tree = focus_tree(&workbench, cx);
    press("ctrl-w h", cx);
    assert!(
        editor_focused(&workbench, cx),
        "the panes left of the sidebar"
    );
    assert_eq!(active_editor(&workbench, cx), right);
    press("ctrl-w h", cx);
    assert_eq!(active_editor(&workbench, cx), left, "the panes' own key");
    focus(&tree, cx);
    press("ctrl-w ctrl-h", cx);
    assert!(editor_focused(&workbench, cx));
    assert_eq!(active_editor(&workbench, cx), left, "the last active");

    // Nothing right of the sidebar on the right.
    focus(&tree, cx);
    press("ctrl-w l", cx);
    assert!(focused(&tree, cx), "nothing on the right");
    press("ctrl-l", cx);
    assert!(focused(&tree, cx), "nothing on the right");

    // Moved left, the keys swap.
    run("dock.move_sidebar_left", cx);
    focus(&tree, cx);
    press("ctrl-w h", cx);
    assert!(focused(&tree, cx), "nothing on the left");
    press("ctrl-w l", cx);
    assert!(
        editor_focused(&workbench, cx),
        "the panes right of the sidebar"
    );
    assert_eq!(active_editor(&workbench, cx), left);
    focus(&tree, cx);
    press("ctrl-l", cx);
    assert!(editor_focused(&workbench, cx));

    // <C-w>p and <C-w>w from either dock.
    for keys in ["ctrl-w p", "ctrl-w w"] {
        focus(&tree, cx);
        press(keys, cx);
        assert!(editor_focused(&workbench, cx), "{keys} from the tree");
        run("terminal.toggle", cx);
        assert!(focused(&terminal(&workbench, cx), cx));
        press(keys, cx);
        assert!(editor_focused(&workbench, cx), "{keys} from the terminal");
    }
}

#[gpui::test]
fn between_docks(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("terminal.toggle", cx);
    let terminal = terminal(&workbench, cx);
    assert!(focused(&terminal, cx));

    // Down from the sidebar to the terminal, and back up.
    let tree = focus_tree(&workbench, cx);
    press("ctrl-w j", cx);
    assert!(focused(&terminal, cx), "the terminal below the sidebar");
    press("ctrl-w k", cx);
    assert!(focused(&tree, cx), "back to the sidebar's shown view");
    press("ctrl-w k", cx);
    assert!(focused(&tree, cx), "nothing above the sidebar");
    press("ctrl-w ctrl-j", cx);
    assert!(focused(&terminal, cx));
    press("ctrl-w ctrl-k", cx);
    assert!(focused(&tree, cx));

    // Up after focus has been elsewhere goes to the editor.
    focus(&tree, cx);
    press("ctrl-w j", cx);
    assert!(focused(&terminal, cx));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    focus(&editor, cx);
    focus(&terminal, cx);
    press("ctrl-w k", cx);
    assert!(
        editor_focused(&workbench, cx),
        "the panes above the terminal"
    );

    // The sidebar is beside the terminal too.
    focus(&terminal, cx);
    press("ctrl-w l", cx);
    assert!(focused(&tree, cx), "the sidebar on the right");
    focus(&terminal, cx);
    press("ctrl-w h", cx);
    assert!(focused(&terminal, cx), "nothing on the left");
    press("ctrl-w j", cx);
    assert!(focused(&terminal, cx), "nothing below");

    // A hidden terminal isn't below the sidebar.
    run("terminal.toggle", cx);
    focus(&tree, cx);
    press("ctrl-w j", cx);
    assert!(focused(&tree, cx), "the terminal is hidden");
}

/// Up from the bottom panel goes back to the sidebar only from the tool
/// window a window key brought focus to from there: focus leaving one tool
/// window forgets it for the others too.
#[gpui::test]
fn up_from_another_tool_window_goes_to_the_editor(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let problems = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().focus_handle(cx)
    });
    let terminal = terminal(&workbench, cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));

    // Sidebar, then Problems, then the editor, then the terminal.
    run("problems.show", cx);
    let tree = focus_tree(&workbench, cx);
    press("ctrl-w j", cx);
    assert!(focused(&problems, cx), "Problems below the sidebar");
    focus(&editor, cx);
    run("terminal.toggle", cx);
    assert!(focused(&terminal, cx));
    press("ctrl-w k", cx);
    assert!(
        editor_focused(&workbench, cx),
        "the terminal wasn't entered from the sidebar"
    );

    // Sidebar, then the terminal, then the editor, then Problems.
    focus(&tree, cx);
    press("ctrl-w j", cx);
    assert!(focused(&terminal, cx), "the terminal below the sidebar");
    focus(&editor, cx);
    run("problems.show", cx);
    focus(&problems, cx);
    assert!(focused(&problems, cx));
    press("ctrl-w k", cx);
    assert!(
        editor_focused(&workbench, cx),
        "Problems wasn't entered from the sidebar"
    );
}

#[gpui::test]
fn escape(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let (outline, problems, changes) = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        (
            dock.outline().focus_handle(cx),
            dock.problems().focus_handle(cx),
            dock.changes().focus_handle(cx),
        )
    });

    // From each list, with no field open, to the editor.
    let tree = focus_tree(&workbench, cx);
    press("escape", cx);
    assert!(editor_focused(&workbench, cx), "from the tree");
    for (show, view) in [
        ("outline.show", &outline),
        ("problems.show", &problems),
        ("git.show_changes", &changes),
    ] {
        run(show, cx);
        focus(view, cx);
        assert!(focused(view, cx), "{show}");
        press("escape", cx);
        assert!(editor_focused(&workbench, cx), "escape after {show}");
    }

    // The tree's name field: escape closes it, in the tree.
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    focus_tree(&workbench, cx);
    press("a", cx);
    assert!(
        panel
            .read_with(cx, |panel, cx| panel.name_field(cx))
            .is_some()
    );
    press("escape", cx);
    assert_eq!(panel.read_with(cx, |panel, cx| panel.name_field(cx)), None);
    assert!(focused(&tree, cx), "the field closes, in the tree");
    assert!(!editor_focused(&workbench, cx));

    // Search's input keeps escape and ctrl-w.
    run("search.show", cx);
    let input = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().read(cx).input().clone()
    });
    let input_focus = cx.update(|_, cx| input.focus_handle(cx));
    assert!(focused(&input_focus, cx));
    press("escape", cx);
    assert!(focused(&input_focus, cx), "escape in search's input");
    cx.simulate_input("ab");
    press("ctrl-w h", cx);
    assert!(focused(&input_focus, cx), "ctrl-w h in search's input");
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "abh"
    );

    // The commit box: escape leaves insert mode, then stays.
    run("git.show_changes", cx);
    let commit_box = workbench.read_with(cx, |workbench, cx| {
        workbench
            .dock()
            .read(cx)
            .changes()
            .read(cx)
            .commit_box()
            .cloned()
    });
    let commit_box = commit_box.expect("the panel's drawn with its box");
    run("git.commit", cx);
    let box_focus = cx.update(|_, cx| commit_box.focus_handle(cx));
    let box_focused = |cx: &mut VisualTestContext| focused(&box_focus, cx);
    assert!(box_focused(cx));
    assert_eq!(
        commit_box.read_with(cx, |editor, _| editor.vim_mode()),
        Some("insert")
    );
    press("escape", cx);
    assert!(box_focused(cx));
    assert_eq!(
        commit_box.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
    press("escape", cx);
    assert!(box_focused(cx), "escape in the box's normal mode");
}

/// Three quickfix items in `root`, the second selected.
fn quickfix_list(root: &Path) -> crate::quickfix::QuickfixList {
    use crate::quickfix::{QuickfixItem, QuickfixKind, QuickfixList};
    let item = |row: u32, text: &str| QuickfixItem {
        path: root.join("a.go"),
        row,
        col: 0,
        text: text.to_owned(),
        kind: QuickfixKind::Error,
    };
    let mut list = QuickfixList::new(
        ":grep x",
        vec![item(0, "one"), item(1, "two"), item(2, "three")],
    );
    list.current = 1;
    list
}

#[gpui::test]
fn escape_and_ctrl_w_leave_the_quickfix_panel(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    workbench.update(cx, |workbench, cx| {
        workbench.push_quickfix_list(false, quickfix_list(dir.path()), cx)
    });
    for keys in ["escape", "ctrl-w h", "ctrl-w p", "ctrl-h"] {
        focus(
            &workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx)),
            cx,
        );
        run("quickfix.show", cx);
        assert!(
            !editor_focused(&workbench, cx),
            "{keys}: the panel has focus"
        );
        press(keys, cx);
        assert!(editor_focused(&workbench, cx), "{keys} leaves the panel");
        let current = workbench.update(cx, |workbench, cx| {
            workbench.quickfix_list(false, cx).map(|list| list.current)
        });
        assert_eq!(current, Some(1), "{keys} keeps the current item");
    }
}

#[gpui::test]
fn escape_leaves_an_empty_quickfix_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // A pane with no location list shows nothing (E776), so `location.show`
    // is tried on an empty one.
    workbench.update(cx, |workbench, cx| {
        let empty = crate::quickfix::QuickfixList::new("", Vec::new());
        workbench.push_quickfix_list(true, empty, cx)
    });
    for show in ["quickfix.show", "location.show"] {
        focus(
            &workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx)),
            cx,
        );
        run(show, cx);
        assert!(
            !editor_focused(&workbench, cx),
            "{show}: the panel has focus"
        );
        press("escape", cx);
        assert!(editor_focused(&workbench, cx), "escape after {show}");
    }
}
