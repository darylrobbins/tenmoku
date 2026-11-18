//! The New menu's keys (T681): `project_panel.new_menu` opens it under the
//! `+` from the focused tree, and File and Folder show keys without vim.
//!
//! The test harness installs the macOS keymap on every host, so the Linux
//! keystroke is looked up in the Linux keymap, for the tree's context, and
//! the command it names run from the focused tree; the rest is typed.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::{NEW_BUTTON, button, draw, field, items, project, workbench};
use crate::ProjectPanel;

const COMMAND: &str = "project_panel.new_menu";

fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers([layer.expect("the default keymap loads")])
}

/// What `keys` runs with the tree focused, `vim` on or off.
fn in_tree(keymap: &Keymap, keys: &str, vim: bool) -> Option<String> {
    let mut tree = ContextFrame::new("project_panel");
    if vim {
        tree = tree.flag("vim");
    }
    let stack: ContextStack = [ContextFrame::new("workbench"), tree].into_iter().collect();
    let keystroke = keys.parse().unwrap();
    keymap
        .lookup(&keystroke, &stack)
        .map(|invocation| invocation.command.as_str().to_owned())
}

fn focus_tree(panel: &gpui::Entity<ProjectPanel>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus = panel.focus_handle(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
}

/// Runs `command` as a key does, from where focus is.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let stack = kiln::platform::keys::context_stack(&window.context_stack(), cx);
        let invocation = Invocation::of(command).expect("a known command");
        kiln::platform::command_actions::dispatch_invocation(invocation.context(stack), window, cx)
            .expect("it runs");
    });
    draw(cx);
}

/// Installs the Linux keymap, as the harness has the macOS one.
fn use_linux_keys(cx: &mut VisualTestContext) {
    let linux = keymap(DEFAULT_LINUX);
    cx.update(|_, cx| fe_gpui::set_user_keymap(cx, linux));
}

#[gpui::test]
fn new_menu_key_opens_the_menu_from_the_tree(cx: &mut TestAppContext) {
    let macos = keymap(DEFAULT_MACOS);
    assert_eq!(in_tree(&macos, "cmd-n", false).as_deref(), Some(COMMAND));
    let dir = project(&[("a.txt", "")]);
    let (_workbench, panel, cx) = workbench(cx, &dir);
    use_linux_keys(cx);
    focus_tree(&panel, cx);
    assert_eq!(items(&panel, cx), None);
    // The Linux keystroke, through the application's keymap.
    cx.simulate_keystrokes("alt-n");
    draw(cx);
    assert!(items(&panel, cx).is_some(), "alt-n opens the menu");
    let plus = button(NEW_BUTTON, cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_anchor()),
        Some(plus)
    );
    // File and Folder show their keys, with vim off.
    let drawn = panel
        .read_with(cx, |panel, cx| panel.new_menu_drawn(cx))
        .unwrap();
    assert!(drawn[..2].iter().all(|(_, key)| key.is_some()), "{drawn:?}");
    // Down twice reaches Folder; Enter names it.
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    assert_eq!(items(&panel, cx), None);
    assert!(field(&panel, cx).is_some(), "the name field opens");
    cx.simulate_input("src");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(dir.path().join("src").is_dir());
}

#[gpui::test]
fn the_command_shows_a_hidden_dock_from_the_editor(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "")]);
    let (workbench, panel, cx) = workbench(cx, &dir);
    // The palette's route: the editor has focus and the dock is hidden.
    cx.update(|window, cx| {
        let focus = workbench.focus_handle(cx);
        window.focus(&focus, cx);
    });
    run("dock.toggle", cx);
    let visible = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).is_visible())
    };
    assert!(!visible(cx), "the dock is hidden");
    run(COMMAND, cx);
    // The menu opens a frame after the dock is drawn.
    draw(cx);
    draw(cx);
    assert!(visible(cx), "it shows the Project dock");
    assert!(items(&panel, cx).is_some(), "and opens the menu");
    let plus = button(NEW_BUTTON, cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_anchor()),
        Some(plus)
    );
}

#[gpui::test]
fn the_command_waits_for_the_project_header_after_another_panel(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "")]);
    let (workbench, panel, cx) = workbench(cx, &dir);
    cx.update(|window, cx| {
        let focus = workbench.focus_handle(cx);
        window.focus(&focus, cx);
    });
    // Search shows in the sidebar, the Project header gone from the frame.
    run("search.show", cx);
    draw(cx);
    assert!(cx.debug_bounds("icon-button:project-new").is_none());
    run(COMMAND, cx);
    draw(cx);
    draw(cx);
    assert!(items(&panel, cx).is_some(), "the menu opens");
    let plus = button(NEW_BUTTON, cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_anchor()),
        Some(plus),
        "under the + as drawn now"
    );
}

#[gpui::test]
fn the_command_anchors_to_a_header_never_drawn_before(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "")]);
    let (workbench, cx) = crate::tests::open_root(cx, dir.path());
    let panel = super::panel(&workbench, cx);
    cx.update(|window, cx| {
        window.activate_window();
        let focus = workbench.focus_handle(cx);
        window.focus(&focus, cx);
    });
    run("search.show", cx);
    assert!(cx.debug_bounds("icon-button:project-new").is_none());
    run(COMMAND, cx);
    draw(cx);
    draw(cx);
    let plus = button(NEW_BUTTON, cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_anchor()),
        Some(plus)
    );
}

#[gpui::test]
fn the_command_anchors_to_the_header_as_drawn_not_as_recorded(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "")]);
    let (workbench, panel, cx) = workbench(cx, &dir);
    cx.update(|window, cx| {
        let focus = workbench.focus_handle(cx);
        window.focus(&focus, cx);
    });
    let before = button(NEW_BUTTON, cx);
    // The sidebar moves while the Project header is hidden.
    run("search.show", cx);
    run("dock.move_sidebar_right", cx);
    draw(cx);
    run(COMMAND, cx);
    draw(cx);
    draw(cx);
    let after = button(NEW_BUTTON, cx);
    assert_ne!(before, after, "the + moved while hidden");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_anchor()),
        Some(after),
        "under the + as drawn now, not as last recorded"
    );
}

#[gpui::test]
fn the_command_with_no_folder_offers_open_folder(cx: &mut TestAppContext) {
    let (workbench, cx, _) = crate::tests::open(
        cx,
        crate::Launch {
            root: None,
            file: None,
        },
    );
    cx.update(|window, _| window.activate_window());
    draw(cx);
    let panel = super::panel(&workbench, cx);
    cx.update(|window, cx| {
        let focus = workbench.focus_handle(cx);
        window.focus(&focus, cx);
    });
    run(COMMAND, cx);
    // The menu opens a frame after the dock is drawn.
    draw(cx);
    draw(cx);
    let drawn = panel.read_with(cx, |panel, cx| panel.new_menu_drawn(cx));
    let titles: Option<Vec<String>> =
        drawn.map(|drawn| drawn.into_iter().map(|(title, _)| title).collect());
    assert_eq!(titles, Some(vec!["Open Folder…".to_owned()]));
}

#[gpui::test]
fn file_and_folder_have_keys_outside_vim(_cx: &mut TestAppContext) {
    for (source, keys) in [
        (DEFAULT_LINUX, ["ctrl-alt-n", "ctrl-alt-shift-n"]),
        (DEFAULT_MACOS, ["cmd-alt-n", "cmd-alt-shift-n"]),
    ] {
        let keymap = keymap(source);
        let [file, folder] = keys;
        assert_eq!(
            in_tree(&keymap, file, false).as_deref(),
            Some("project_panel.new_file")
        );
        assert_eq!(
            in_tree(&keymap, folder, false).as_deref(),
            Some("project_panel.new_folder")
        );
    }
}
