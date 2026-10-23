//! The terminal panel's actions row: `+` and kill, with one session or
//! several.

use gpui::{Modifiers, TestAppContext};

use super::terminal_tabs_tests::{
    dock_shown, draw, new_session, opened, sessions, terminal, toggle, until,
};

#[gpui::test]
fn actions_show_with_one_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    assert_eq!(sessions(&workbench, cx).len(), 1);
    let new = cx
        .debug_bounds("terminal-new")
        .expect("+ shows with one session");
    let kill = cx
        .debug_bounds("icon-button:terminal-kill")
        .expect("kill shows with one session");
    assert!(new.right() <= kill.left());
    assert!(kill.right() <= cx.update(|window, _| window.viewport_size().width));
    assert!(new.size.height <= fe_ui::TAB_HEIGHT);

    // With two the row stays.
    new_session(&workbench, cx);
    assert!(cx.debug_bounds("terminal-new").is_some());
    assert!(cx.debug_bounds("icon-button:terminal-kill").is_some());
}

#[gpui::test]
fn plus_adds_and_kill_closes(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    super::terminal_tabs_tests::started(&sessions(&workbench, cx)[0], cx);

    let new = cx.debug_bounds("terminal-new").unwrap();
    cx.simulate_click(new.center(), Modifiers::none());
    draw(cx);
    until(cx, "a second session", |cx| {
        sessions(&workbench, cx).len() == 2
    });
    draw(cx);
    assert_eq!(
        super::terminal_tabs_tests::drawn_tabs(&workbench, cx),
        ["Terminal 1", "Terminal 2"]
    );
    let active = terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.active());
    assert_eq!(active, 1);

    for left in [1, 0] {
        let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
        cx.simulate_click(kill.center(), Modifiers::none());
        draw(cx);
        until(cx, "a session to go", |cx| {
            sessions(&workbench, cx).len() == left
        });
    }
    assert!(!dock_shown(&workbench, cx));
}
