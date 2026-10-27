//! Focus Editor Group 1-4: `pane.focus_group_n` activates the nth pane in
//! layout order and focuses its active tab; past the last pane it does
//! nothing.

use fe_gpui::{set_vim_enabled, vim_registers};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Pane, Workbench,
    tests::find_bar::{self, editor, editor_focused, opened, pane, run},
};

fn all_panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| workbench.panes(cx))
}

/// Three panes: split right, then the new pane split down.
fn three(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    run("pane.split_right", cx);
    run("pane.split_down", cx);
    let panes = all_panes(workbench, cx);
    assert_eq!(panes.len(), 3);
    panes
}

#[gpui::test]
fn focuses_the_nth_pane_in_layout_order(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "one\n")]);
    let panes = three(&workbench, cx);
    for (n, expected) in panes.iter().enumerate() {
        run(&format!("pane.focus_group{}", n + 1), cx);
        assert_eq!(&pane(&workbench, cx), expected, "group {}", n + 1);
        assert!(editor_focused(&editor(expected, cx), cx), "group {}", n + 1);
    }
    // Only three panes: the fourth group is nothing.
    run("pane.focus_group1", cx);
    run("pane.focus_group4", cx);
    assert_eq!(pane(&workbench, cx), panes[0]);
    assert!(editor_focused(&editor(&panes[0], cx), cx));
    let after = all_panes(&workbench, cx);
    assert_eq!(after, panes);
}

#[gpui::test]
fn single_pane_and_repeat(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "one\n")]);
    let only = pane(&workbench, cx);
    let terminal = workbench.read_with(cx, |workbench, cx| workbench.terminal().focus_handle(cx));
    cx.update(|window, cx| window.focus(&terminal, cx));
    find_bar::draw(cx);
    assert!(!editor_focused(&editor(&only, cx), cx));

    run("pane.focus_group1", cx);
    assert_eq!(pane(&workbench, cx), only);
    assert!(editor_focused(&editor(&only, cx), cx));
    run("pane.focus_group1", cx);
    assert_eq!(pane(&workbench, cx), only);
    assert!(editor_focused(&editor(&only, cx), cx));
}

#[gpui::test]
fn vim_and_find(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "one\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    run("pane.split_right", cx);
    let panes = all_panes(&workbench, cx);
    let (left, right) = (panes[0].clone(), panes[1].clone());
    let register = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| Some(vim_registers(cx)?.get('"')?.text.clone()))
    };

    // Vim on, in normal mode in the left pane, with a line yanked.
    run("pane.focus_group1", cx);
    let editor = editor(&left, cx);
    cx.simulate_keystrokes("escape y y");
    find_bar::draw(cx);
    let mode = editor.read_with(cx, |editor, _| editor.vim_mode());
    assert_eq!(mode, Some("normal"));
    assert_eq!(register(cx).as_deref(), Some("one\n"));

    // The find bar opens in the right pane; focusing the left group
    // leaves it open there, and the mode and register as they were.
    run("pane.focus_group2", cx);
    run("find.show", cx);
    assert!(find_bar::open(&right, cx));
    run("pane.focus_group1", cx);
    assert_eq!(pane(&workbench, cx), left);
    assert!(find_bar::open(&right, cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.vim_mode()), mode);
    assert_eq!(register(cx).as_deref(), Some("one\n"));
}
