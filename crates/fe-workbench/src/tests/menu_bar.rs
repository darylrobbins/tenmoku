//! The menu bar drawn in the window: the installed menus, opened by click
//! or F10, run in the context focus had before, and closed by Escape, a
//! click elsewhere or a chosen item.

use std::{cell::RefCell, fs, path::Path, rc::Rc};

use fe_buffer::Buffer;
use fe_command::{Invocation, Source};
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, Keymap, KeymapLayer};
use fe_ui::MENU_BAR_HEIGHT;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, point, px};

use super::open;
use crate::{Launch, Workbench};

/// A workbench showing `a.rs`, with the Linux keymap and menus and the
/// in-window menu bar, and the invocations run from then on.
fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<Workbench>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.rs");
    fs::write(&file, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((file, Some(Buffer::new("fn a() {}\n")))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        let (layer, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_LINUX, None);
        fe_gpui::set_user_keymap(cx, Keymap::from_layers([layer.unwrap()]));
        let (bar, diagnostics) = fe_keymap::default_menus_linux(Some(&*crate::command_catalog()));
        assert_eq!(diagnostics, []);
        fe_gpui::menu::install(cx, bar.unwrap());
        let sink = seen.clone();
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone());
        });
    });
    workbench.update(cx, |workbench, cx| workbench.set_menu_bar(true, cx));
    cx.update(|window, cx| {
        let editor = workbench.read(cx).active_editor(cx).expect("a.rs is open");
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (workbench, cx, seen, dir)
}

fn open_menu(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(usize, Option<usize>)> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.menu_bar().unwrap().read(cx).open_menu()
    })
}

/// The commands run from a menu: the editor's and the app's, which
/// fe-gpui runs, then the workbench's.
fn menu_runs(
    seen: &RefCell<Vec<Invocation>>,
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    let workbench_runs = workbench.read_with(cx, |workbench, _| workbench.commands_run.clone());
    seen.borrow()
        .iter()
        .chain(&workbench_runs)
        .filter(|invocation| invocation.source == Source::Menu)
        .map(|invocation| invocation.command.as_str().to_owned())
        .collect()
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let editor = workbench.read(cx).active_editor(cx).expect("a.rs is open");
        editor.focus_handle(cx).is_focused(window)
    })
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn f10_and_the_arrows_choose_an_item(cx: &mut TestAppContext) {
    let (workbench, cx, seen, _dir) = setup(cx);
    assert!(editor_focused(&workbench, cx));

    cx.simulate_keystrokes("f10");
    assert_eq!(
        open_menu(&workbench, cx),
        Some((0, Some(0))),
        "File, Open Folder…"
    );
    assert!(!editor_focused(&workbench, cx), "the menu has focus");
    cx.simulate_keystrokes("left");
    assert_eq!(
        open_menu(&workbench, cx),
        Some((4, Some(0))),
        "wraps to Window"
    );
    cx.simulate_keystrokes("right right");
    // Edit: Undo, Redo, separator, Cut, Copy, Paste, Select All, ...
    assert_eq!(open_menu(&workbench, cx), Some((1, Some(0))));
    cx.simulate_keystrokes("down down down");
    assert_eq!(
        open_menu(&workbench, cx),
        Some((1, Some(4))),
        "the separator skipped"
    );
    cx.simulate_keystrokes("up up up");
    assert_eq!(open_menu(&workbench, cx), Some((1, Some(0))));
    cx.simulate_keystrokes("up");
    assert_eq!(
        open_menu(&workbench, cx),
        Some((1, Some(8))),
        "wraps to Find in Files"
    );

    // From Find in Files: Undo, Redo, Cut, Copy, Paste, Select All.
    cx.simulate_keystrokes("down down down down down down enter");
    cx.run_until_parked();
    assert_eq!(open_menu(&workbench, cx), None);
    assert!(editor_focused(&workbench, cx), "focus is back");
    assert_eq!(menu_runs(&seen, &workbench, cx), ["editor.select_all"]);
    let selected = workbench.update(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).unwrap();
        editor.read(cx).model().selections()[0].range.clone()
    });
    assert_eq!(selected, 0..10, "Select All ran in the editor");
}

#[gpui::test]
fn escape_closes_and_gives_focus_back(cx: &mut TestAppContext) {
    let (workbench, cx, seen, _dir) = setup(cx);
    cx.simulate_keystrokes("f10");
    assert!(open_menu(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape");
    assert_eq!(open_menu(&workbench, cx), None);
    assert!(editor_focused(&workbench, cx));
    assert_eq!(menu_runs(&seen, &workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn clicks_open_switch_choose_and_close(cx: &mut TestAppContext) {
    let (workbench, cx, seen, _dir) = setup(cx);
    click("menu:View", cx);
    assert_eq!(open_menu(&workbench, cx), Some((2, None)));
    click("menu:View", cx);
    assert_eq!(open_menu(&workbench, cx), None, "a second click closes it");
    assert!(editor_focused(&workbench, cx));

    click("menu:File", cx);
    let edit = cx.debug_bounds("menu:Edit").unwrap();
    cx.simulate_mouse_move(edit.center(), None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        open_menu(&workbench, cx),
        Some((1, None)),
        "hovering switches"
    );

    // A click elsewhere in the window, below the bar, closes it.
    cx.simulate_click(point(px(600.), MENU_BAR_HEIGHT * 10.), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(open_menu(&workbench, cx), None);

    // A menu-only command, from a click on its item.
    assert!(workbench.read_with(cx, |workbench, _| workbench.status_bar_visible()));
    click("menu:View", cx);
    click("menu-item:Status Bar", cx);
    assert_eq!(open_menu(&workbench, cx), None);
    assert_eq!(menu_runs(&seen, &workbench, cx), ["view.toggle_status_bar"]);
    assert!(!workbench.read_with(cx, |workbench, _| workbench.status_bar_visible()));
}

#[gpui::test]
fn hidden_by_default(cx: &mut TestAppContext) {
    let (workbench, cx, _) = super::open(
        cx,
        Launch {
            root: None,
            file: None,
        },
    );
    assert!(workbench.read_with(cx, |workbench, _| workbench.menu_bar().is_none()));
    assert_eq!(cx.debug_bounds("menu:File"), None);
}
