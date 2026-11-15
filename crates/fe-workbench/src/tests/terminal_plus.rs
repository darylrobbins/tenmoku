//! The terminal panel's `+`: a 16 px icon button the size of kill.

use std::time::Duration;

use gpui::{Modifiers, TestAppContext};

use crate::terminal_panel::terminal_tabs_tests::{draw, opened, sessions, started, toggle, until};

#[gpui::test]
fn plus_is_an_icon_button_the_size_of_kill(cx: &mut TestAppContext) {
    let (_workbench, cx, _dir) = opened(cx);
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
    // The wrapper the bounds tests read still wraps it.
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
    let shown = crate::terminal_panel::last_strip_tooltip().expect("a tooltip showed");
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
