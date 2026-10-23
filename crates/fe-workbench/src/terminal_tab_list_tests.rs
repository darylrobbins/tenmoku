//! The terminal panel's session list on its right: shown from two
//! sessions, a row each with an icon, the label and a kill button.

use fe_ui::ActiveTheme as _;
use gpui::{Modifiers, TestAppContext};

use super::terminal_tabs_tests::{
    active, dock_shown, draw, ids, new_session, opened, sessions, tab_backgrounds, tab_bounds,
    toggle, until, with_sessions,
};

#[gpui::test]
fn list_shows_from_two_sessions(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    let first = sessions(&workbench, cx)[0].clone();
    super::terminal_tabs_tests::started(&first, cx);
    assert!(super::terminal_tabs_tests::drawn_tabs(&workbench, cx).is_empty());

    new_session(&workbench, cx);
    let terminal = super::terminal_tabs_tests::terminal(&workbench, cx);
    let list = terminal.read_with(cx, |terminal, _| terminal.strip_bounds());
    let row = tab_bounds(&workbench, 0, cx);
    // On the right, ~160px wide, the full height of the terminal row.
    assert!((list.size.width.as_f32() - 160.0).abs() < 2.0, "{list:?}");
    // The terminal is to its left.
    assert!(row.left() >= list.left() && row.right() <= list.right());
    assert!(list.size.height > row.size.height);
}

#[gpui::test]
fn rows_show_icon_label_and_active(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 1, cx);
    for view in &all {
        let id = view.entity_id();
        assert!(
            cx.debug_bounds(format!("terminal-tab-icon-{id}").leak())
                .is_some()
        );
    }
    assert_eq!(
        super::terminal_tabs_tests::drawn_tabs(&workbench, cx),
        ["Terminal 1", "Terminal 2"]
    );
    let (selected, inactive) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (colors.list.row_selected, colors.tab.inactive)
    });
    assert_eq!(tab_backgrounds(&workbench, cx), [inactive, selected]);
}

#[gpui::test]
fn kill_button_drops_its_own_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 2, cx);
    let [first, second, third] = [0, 1, 2].map(|ix| all[ix].entity_id());
    assert_eq!(active(&workbench, cx), 2);

    let row = tab_bounds(&workbench, 0, cx);
    cx.simulate_mouse_move(row.center(), None, Modifiers::none());
    draw(cx);
    let close = cx
        .debug_bounds(format!("icon-button:terminal-tab-close-{first}").leak())
        .expect("the first's kill button shows");
    cx.simulate_click(close.center(), Modifiers::none());
    draw(cx);
    assert_eq!(ids(&workbench, cx), [second, third]);
    assert_eq!(active(&workbench, cx), 1);

    // The active row's button shows without hover and drops the active one.
    let close = cx
        .debug_bounds(format!("icon-button:terminal-tab-close-{third}").leak())
        .expect("the active kill button shows");
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
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    assert_eq!(
        super::terminal_tabs_tests::drawn_tabs(&workbench, cx).len(),
        0
    );
    new_session(&workbench, cx);
    assert_eq!(
        super::terminal_tabs_tests::drawn_tabs(&workbench, cx),
        ["Terminal 1", "Terminal 2"]
    );
}
