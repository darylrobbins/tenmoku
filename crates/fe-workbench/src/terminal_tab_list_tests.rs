//! The terminal panel's own tab strip (T349): a tab per session along its
//! top, each with its icon and label and a close button that kills it.

use fe_ui::{ActiveTheme as _, TAB_HEIGHT};
use gpui::{Modifiers, TestAppContext};

use super::terminal_tabs_tests::{
    active, dock_shown, draw, drawn_tabs, ids, new_session, opened, sessions, tab_backgrounds,
    tab_bounds, terminal, toggle, until, with_sessions,
};

#[gpui::test]
fn the_strip_has_a_tab_per_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    let first = sessions(&workbench, cx)[0].clone();
    super::terminal_tabs_tests::started(&first, cx);
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1"]);

    new_session(&workbench, cx);
    let strip = terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.strip_bounds());
    let [one, two] = [0, 1].map(|ix| tab_bounds(&workbench, ix, cx));
    // A strip as tall as the editor's, along the panel's top, the tabs side
    // by side in it in start order.
    assert_eq!(strip.size.height, TAB_HEIGHT);
    assert_eq!(one.top(), strip.top());
    assert!(one.bottom() <= strip.bottom());
    assert_eq!(two.top(), one.top());
    assert_eq!(one.left(), strip.left());
    assert!(two.left() >= one.right() - gpui::px(0.5), "{one:?} {two:?}");
    assert!(strip.size.width > one.size.width + two.size.width);
}

#[gpui::test]
fn tabs_show_icon_label_and_active(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    with_sessions(&workbench, 1, cx);
    for label in ["Terminal 1", "Terminal 2"] {
        let icon = format!("tab-icon:{label}:file-terminal");
        assert!(cx.debug_bounds(icon.leak()).is_some(), "{label}");
    }
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1", "Terminal 2"]);
    let (tab_active, inactive) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (colors.tab.active, colors.tab.inactive)
    });
    assert_eq!(tab_backgrounds(&workbench, cx), [inactive, tab_active]);
}

#[gpui::test]
fn close_button_kills_its_own_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 2, cx);
    let [second, third] = [1, 2].map(|ix| all[ix].entity_id());
    assert_eq!(active(&workbench, cx), 2);

    let tab = tab_bounds(&workbench, 0, cx);
    cx.simulate_mouse_move(tab.center(), None, Modifiers::none());
    draw(cx);
    let close = cx
        .debug_bounds("tab-close:Terminal 1")
        .expect("the first's close button shows");
    cx.simulate_click(close.center(), Modifiers::none());
    draw(cx);
    assert_eq!(ids(&workbench, cx), [second, third]);
    assert_eq!(active(&workbench, cx), 1);

    // The active tab's button shows without hover and kills the active one.
    let close = cx
        .debug_bounds("tab-close:Terminal 3")
        .expect("the active close button shows");
    cx.simulate_click(close.center(), Modifiers::none());
    draw(cx);
    assert_eq!(ids(&workbench, cx), [second]);
}

#[gpui::test]
fn killing_the_last_hides_the_panel(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    with_sessions(&workbench, 1, cx);
    super::terminal_tabs_tests::run("terminal.close", cx);
    super::terminal_tabs_tests::run("terminal.close", cx);
    until(cx, "the sessions to go", |cx| {
        sessions(&workbench, cx).is_empty()
    });
    assert!(!dock_shown(&workbench, cx));
    // Shown again, it starts over from 1.
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1"]);
    new_session(&workbench, cx);
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1", "Terminal 2"]);
}
