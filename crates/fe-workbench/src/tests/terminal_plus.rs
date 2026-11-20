//! The terminal panel's `+`: a 16 px icon button the size of kill, in the
//! dock strip with one session and the panel's row with two.

use std::time::Duration;

use gpui::{Modifiers, TestAppContext};
use kiln_shell::DockView as _;

use crate::terminal_panel::terminal_tabs_tests::{
    draw, new_session, opened, sessions, started, toggle, until,
};

#[gpui::test]
fn plus_is_an_icon_button_the_size_of_kill(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();
    let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
    assert_eq!(plus.size, kill.size);
    assert_eq!(plus.size.width, gpui::px(22.));
    let icon = cx
        .debug_bounds("icon-button-icon:terminal-new-button:kiln-ui/icons/plus.svg")
        .expect("the plus icon is drawn");
    assert_eq!(icon.size, gpui::size(gpui::px(16.), gpui::px(16.)));
    assert!(plus.right() <= kill.left(), "+ comes before kill");

    // As the panel's own row has it from two sessions, in the wrapper the
    // bounds tests read.
    started(&sessions(&workbench, cx)[0], cx);
    new_session(&workbench, cx);
    let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();
    let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
    assert_eq!(plus.size, kill.size);
    assert_eq!(plus.size.width, gpui::px(22.));
    assert!(plus.right() <= kill.left(), "+ comes before kill");
    let wrapper = cx.debug_bounds("terminal-new").unwrap();
    assert!(wrapper.contains(&plus.center()));
}

#[gpui::test]
fn plus_keeps_its_name_tooltip_and_click(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();

    // Its accessible name is the string the button had as text.
    let named = cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        fe_ui::nodes(window, cx).into_iter().any(|node| {
            node.role == fe_ui::Role::Button
                && node.label == "New Terminal"
                && format!("{:?}", node.id).contains("terminal-new-button")
        })
    });
    assert!(named, "a Button named New Terminal");

    // Hovering shows its name and the key bound to terminal.new.
    cx.simulate_mouse_move(plus.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    let key = cx.update(|_, cx| crate::pane_toolbar::shortcut("terminal.new", cx));
    // One session: the dock strip's button.
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    let shown = dock
        .read_with(cx, |dock, _| dock.dock().drawn().last_tooltip())
        .expect("a tooltip showed");
    match key {
        Some(key) => assert_eq!(shown, format!("New Terminal {key}")),
        None => assert_eq!(shown, "New Terminal"),
    }
    assert!(shown.starts_with("New Terminal"));

    cx.simulate_click(plus.center(), Modifiers::none());
    draw(cx);
    until(cx, "a second session", |cx| {
        sessions(&workbench, cx).len() == 2
    });
}

#[test]
fn plus_icon_uses_the_line_stroke() {
    let svg = include_str!("../../../../kiln/crates/kiln-ui/assets/icons/plus.svg");
    assert!(svg.contains(r#"viewBox="0 0 24 24""#));
    assert_eq!(svg.matches("stroke-width=\"1.75\"").count(), 1);
}

/// The `+` beside its caret: the `+` keeps its size and its click, a new
/// session of the default profile; the caret, as big, opens the menu of
/// profiles and starts nothing. In the dock strip with one session and the
/// panel's row with two.
#[gpui::test]
fn plus_keeps_its_size_and_click_beside_its_caret(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let terminal = crate::terminal_panel::terminal_tabs_tests::terminal(&workbench, cx);
    let found = vec![fe_terminal_view::profiles::Profile::shell(
        "sh",
        "sh",
        "/bin/sh",
        Vec::new(),
    )];
    terminal.update(cx, |terminal, cx| terminal.set_detected(found, cx));
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    for count in [1, 2] {
        draw(cx);
        let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();
        let caret = cx
            .debug_bounds("icon-button:terminal-profiles-button")
            .unwrap();
        let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
        assert_eq!(plus.size, kill.size, "with {count}");
        assert_eq!(plus.size.width, gpui::px(22.));
        assert_eq!(caret.size, plus.size);
        assert!(plus.right() <= caret.left(), "the caret follows +");
        assert!(caret.right() <= kill.left(), "kill follows the caret");
        let chevron = "icon-button-icon:terminal-profiles-button:kiln-ui/icons/chevron-down.svg";
        assert!(cx.debug_bounds(chevron).is_some(), "the caret is drawn");

        // The caret opens the menu, starting nothing.
        cx.simulate_click(caret.center(), Modifiers::none());
        draw(cx);
        let menu = terminal.read_with(cx, |terminal, _| terminal.profile_menu_names());
        assert_eq!(menu, Some(vec!["sh".to_owned()]));
        assert_eq!(sessions(&workbench, cx).len(), count);
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(
            terminal.read_with(cx, |terminal, _| terminal.profile_menu_names()),
            None
        );

        // The + still starts one.
        cx.simulate_click(plus.center(), Modifiers::none());
        draw(cx);
        until(cx, "another session", |cx| {
            sessions(&workbench, cx).len() == count + 1
        });
        started(sessions(&workbench, cx).last().unwrap(), cx);
    }
}
