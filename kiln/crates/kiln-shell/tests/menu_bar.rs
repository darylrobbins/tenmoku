// SPDX-License-Identifier: LicenseRef-Proprietary

//! The menu bar drawn in the window: the installed menus, opened by a
//! click or `menu_bar.open` (F10), chosen with the arrows and Enter, run
//! in the context focus had before, and closed by Escape, a click
//! elsewhere or a chosen item. Moved from the editor it was written for,
//! with its tests.

use crate::support;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use kiln_shell::Root;
use kiln_ui::MENU_BAR_HEIGHT;

use support::{click, linux, mac, open, pad_focused};

fn open_menu(root: &Entity<Root>, cx: &mut VisualTestContext) -> Option<(usize, Option<usize>)> {
    root.read_with(cx, |root, cx| root.menu_bar().unwrap().read(cx).open_menu())
}

fn ran(pad: &Entity<support::Pad>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    pad.read_with(cx, |pad, _| pad.ran.clone())
}

#[gpui::test]
fn f10_and_the_arrows_choose_an_item(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    assert!(pad_focused(&pad, cx));

    cx.simulate_keystrokes("f10");
    assert_eq!(open_menu(&root, cx), Some((0, Some(0))), "Pad, Clear");
    assert!(!pad_focused(&pad, cx), "the menu has focus");
    // Pad: Clear, a separator, Select All.
    cx.simulate_keystrokes("down");
    assert_eq!(
        open_menu(&root, cx),
        Some((0, Some(2))),
        "the separator skipped"
    );
    cx.simulate_keystrokes("down");
    assert_eq!(open_menu(&root, cx), Some((0, Some(0))), "wraps");
    cx.simulate_keystrokes("up");
    assert_eq!(open_menu(&root, cx), Some((0, Some(2))), "wraps back");
    cx.simulate_keystrokes("left");
    assert_eq!(open_menu(&root, cx), Some((2, Some(0))), "wraps to Window");
    cx.simulate_keystrokes("right right");
    assert_eq!(open_menu(&root, cx), Some((1, Some(0))), "Edit");

    // Edit's Select All, which the pad runs, with focus back on it.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(open_menu(&root, cx), None);
    assert!(pad_focused(&pad, cx), "focus is back");
    assert_eq!(ran(&pad, cx), ["pad.select_all"]);
}

#[gpui::test]
fn escape_closes_and_gives_focus_back(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    cx.simulate_keystrokes("f10");
    assert!(open_menu(&root, cx).is_some());
    cx.simulate_keystrokes("escape");
    assert_eq!(open_menu(&root, cx), None);
    assert!(pad_focused(&pad, cx));
    assert_eq!(ran(&pad, cx), Vec::<&str>::new());
}

#[gpui::test]
fn clicks_open_switch_choose_and_close(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    click("menu:Edit", cx);
    assert_eq!(open_menu(&root, cx), Some((1, None)));
    click("menu:Edit", cx);
    assert_eq!(open_menu(&root, cx), None, "a second click closes it");
    assert!(pad_focused(&pad, cx));

    click("menu:Pad", cx);
    let edit = cx.debug_bounds("menu:Edit").unwrap();
    cx.simulate_mouse_move(edit.center(), None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(open_menu(&root, cx), Some((1, None)), "hovering switches");

    // A click elsewhere in the window, below the bar, closes it.
    cx.simulate_click(point(px(300.), MENU_BAR_HEIGHT * 10.), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(open_menu(&root, cx), None);

    click("menu:Pad", cx);
    click("menu-item:Clear", cx);
    assert_eq!(open_menu(&root, cx), None);
    assert!(pad_focused(&pad, cx));
    assert_eq!(ran(&pad, cx), ["pad.clear"]);
}

/// The menus can change while one is open, as when a command is
/// contributed: one whose menu is gone closes, giving focus back, and the
/// arrows don't step through an empty bar.
#[gpui::test]
fn a_menu_removed_while_open_closes(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    let bar = cx.update(|_, cx| kiln_platform::menus::installed(cx).cloned().unwrap());
    cx.simulate_keystrokes("f10 down");
    assert_eq!(open_menu(&root, cx), Some((0, Some(2))));
    cx.update(|_, cx| kiln_platform::menus::install(cx, Default::default()));
    cx.run_until_parked();
    assert_eq!(open_menu(&root, cx), None);
    assert!(pad_focused(&pad, cx), "focus is back");

    cx.update(|_, cx| kiln_platform::menus::install(cx, bar));
    cx.simulate_keystrokes("f10");
    assert_eq!(open_menu(&root, cx), Some((0, Some(0))));
    cx.update(|_, cx| kiln_platform::menus::install(cx, Default::default()));
    cx.simulate_keystrokes("left right");
    assert_eq!(open_menu(&root, cx), None);
    assert!(pad_focused(&pad, cx));
}

#[gpui::test]
fn the_bar_sits_above_the_content(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, linux());
    let name = cx.debug_bounds("menu:Pad").unwrap();
    assert_eq!(name.origin.y, px(0.));
    let pad = cx.debug_bounds("pad").unwrap();
    assert_eq!(pad.origin.y, MENU_BAR_HEIGHT);
    let size = cx.update(|window, _| window.viewport_size());
    assert_eq!(pad.size.width, size.width);
    assert_eq!(pad.bottom(), size.height);
}

#[gpui::test]
fn native_menus_draw_no_bar(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, mac());
    assert!(root.read_with(cx, |root, _| root.menu_bar().is_none()));
    assert_eq!(cx.debug_bounds("menu:Pad"), None);
    cx.simulate_keystrokes("f10");
    assert!(pad_focused(&pad, cx), "nothing opened");
}
