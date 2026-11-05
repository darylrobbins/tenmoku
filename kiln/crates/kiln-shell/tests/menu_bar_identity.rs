// SPDX-License-Identifier: Apache-2.0

//! The open menu and its highlight follow their menu and command when the
//! installed menus change, rather than staying at their positions.

use crate::support;

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_core::MenuBar;
use kiln_shell::Root;

use support::{linux, open, pad_focused};

fn open_menu(root: &Entity<Root>, cx: &mut VisualTestContext) -> Option<(usize, Option<usize>)> {
    root.read_with(cx, |root, cx| root.menu_bar().unwrap().read(cx).open_menu())
}

fn ran(pad: &Entity<support::Pad>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    pad.read_with(cx, |pad, _| pad.ran.clone())
}

fn installed(cx: &mut VisualTestContext) -> MenuBar {
    cx.update(|_, cx| kiln_platform::menus::installed(cx).cloned().unwrap())
}

fn install(bar: MenuBar, cx: &mut VisualTestContext) {
    cx.update(|_, cx| kiln_platform::menus::install(cx, bar));
    cx.run_until_parked();
}

#[gpui::test]
fn an_open_menu_removed_before_a_sibling_closes(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    let mut bar = installed(cx);
    cx.simulate_keystrokes("f10");
    assert_eq!(open_menu(&root, cx), Some((0, Some(0))), "Pad, Clear");

    bar.menus.retain(|menu| menu.name != "Pad");
    install(bar, cx);
    assert_eq!(open_menu(&root, cx), None, "Pad is gone");
    assert!(pad_focused(&pad, cx), "focus is back");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(ran(&pad, cx), Vec::<&str>::new());
}

#[gpui::test]
fn an_open_menu_that_moves_stays_open_on_it(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    let mut bar = installed(cx);
    cx.simulate_keystrokes("f10 right");
    assert_eq!(open_menu(&root, cx), Some((1, Some(0))), "Edit");

    bar.menus.retain(|menu| menu.name != "Pad");
    install(bar, cx);
    assert_eq!(open_menu(&root, cx), Some((0, Some(0))), "still Edit");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(ran(&pad, cx), ["pad.select_all"]);
}

#[gpui::test]
fn an_item_moved_before_the_highlight_keeps_it_on_its_command(cx: &mut TestAppContext) {
    let (root, pad, cx) = open(cx, linux());
    let mut bar = installed(cx);
    cx.simulate_keystrokes("f10 down");
    assert_eq!(open_menu(&root, cx), Some((0, Some(2))), "Select All");

    // Pad: Select All, Clear, a separator.
    bar.menus[0].items.rotate_right(1);
    install(bar, cx);
    assert_eq!(open_menu(&root, cx), Some((0, Some(0))), "follows it");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(ran(&pad, cx), ["pad.select_all"]);
}
