//! One terminal draws one header row: the bottom dock's strip, whose
//! trailing controls carry the panel's `+` and kill; the panel's own row of
//! session tabs comes back from two terminals and goes again at one.

use std::time::Duration;

use fe_ui::TAB_HEIGHT;
use gpui::{Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext};
use kiln_shell::{DockView as _, dock::HIDE_BUTTON};

use crate::{
    Workbench,
    terminal_panel::terminal_tabs_tests::{
        active_and_focused, dock_shown, draw, drawn_tabs, new_session, opened, run, sessions,
        started, tab_bounds, terminal, toggle, until,
    },
};

/// The bottom dock's title row, as last drawn, and its controls' ids.
fn header(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Bounds<Pixels>, Vec<String>) {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| {
        let drawn = dock.dock().drawn();
        let controls = drawn.controls().into_iter().map(|(id, _)| id).collect();
        (drawn.title_bounds().expect("the dock shows"), controls)
    })
}

/// The tooltip the bottom dock's controls last built.
fn dock_tooltip(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.dock().drawn().last_tooltip())
}

/// Whether the last frame has a Button named `name` whose id holds `id`.
fn named_button(name: &str, id: &str, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        fe_ui::nodes(window, cx).into_iter().any(|node| {
            node.role == fe_ui::Role::Button
                && node.label == name
                && format!("{:?}", node.id).contains(id)
        })
    })
}

/// Hovers `bounds` until its tooltip shows.
fn hover(bounds: Bounds<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
}

/// The session's top: directly under the header row that shows.
fn session_top(cx: &mut VisualTestContext) -> Pixels {
    cx.debug_bounds("terminal-session")
        .expect("the session is laid out")
        .top()
}

#[gpui::test]
fn one_terminal_draws_no_session_row_only_the_dock_strip(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    draw(cx);

    // No row of session tabs.
    let terminal = terminal(&workbench, cx);
    assert!(!terminal.read_with(cx, |terminal, _| terminal.strip_drawn()));
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1"]);
    assert_eq!(
        terminal.read_with(cx, |terminal, _| terminal.tab_bounds(0)),
        None
    );
    assert!(cx.debug_bounds("terminal-new").is_none());
    assert!(cx.debug_bounds("terminal-debuggee-rerun").is_none());

    // The dock's strip is the one header row, the session right under it.
    let (title, _) = header(&workbench, cx);
    assert_eq!(title.size.height, TAB_HEIGHT);
    assert_eq!(session_top(cx), title.bottom());
    // One of each control: the dock's.
    let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();
    assert!(title.contains(&plus.center()));
}

#[gpui::test]
fn new_and_close_stay_reachable_with_one_terminal(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    draw(cx);

    // The dock strip's trailing controls: `+`, its caret, kill, then the
    // dock's own.
    let (title, controls) = header(&workbench, cx);
    assert_eq!(
        &controls[..3],
        [
            "terminal-new-button",
            "terminal-profiles-button",
            "terminal-kill"
        ]
    );
    assert_eq!(controls.last().map(String::as_str), Some(HIDE_BUTTON));
    let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();
    let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
    assert!(title.contains(&plus.center()) && title.contains(&kill.center()));
    assert!(plus.right() <= kill.left(), "+ comes before kill");
    assert_eq!(plus.size, kill.size);
    let icon = "icon-button-icon:terminal-new-button:kiln-ui/icons/plus.svg";
    assert!(cx.debug_bounds(icon).is_some(), "the plus icon is drawn");

    // Their accessible names, and tooltips with the commands' keys, are the
    // panel strip's.
    assert!(named_button("New Terminal", "terminal-new-button", cx));
    let kill_name = kiln_l10n::tr!("workbench-terminal-kill").to_string();
    assert!(named_button(&kill_name, "terminal-kill", cx));
    hover(plus, cx);
    let one_plus = dock_tooltip(&workbench, cx).expect("a tooltip showed");
    let key = cx.update(|_, cx| crate::pane_toolbar::shortcut("terminal.new", cx));
    match key {
        Some(key) => assert_eq!(one_plus, format!("New Terminal {key}")),
        None => assert_eq!(one_plus, "New Terminal"),
    }
    hover(kill, cx);
    let one_kill = dock_tooltip(&workbench, cx).expect("a tooltip showed");
    assert!(one_kill.starts_with(&kill_name));

    // `+` runs terminal.new.
    cx.simulate_click(plus.center(), Modifiers::none());
    draw(cx);
    until(cx, "a second session", |cx| {
        sessions(&workbench, cx).len() == 2
    });
    started(&sessions(&workbench, cx)[1], cx);
    draw(cx);

    // With two the strip's own show the same.
    let (_, controls) = header(&workbench, cx);
    assert!(!controls.iter().any(|id| id.starts_with("terminal-")));
    let plus = cx.debug_bounds("icon-button:terminal-new-button").unwrap();
    hover(plus, cx);
    let strip_plus = crate::terminal_panel::last_strip_tooltip().expect("a tooltip showed");
    assert_eq!(strip_plus, one_plus);
    let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
    hover(kill, cx);
    let strip_kill = crate::terminal_panel::last_strip_tooltip().expect("a tooltip showed");
    assert_eq!(strip_kill, one_kill);

    // Kill, from two to one and from one to none, as terminal.close: the
    // last one's goes from the dock strip and hides the panel.
    for left in [1, 0] {
        let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
        cx.simulate_click(kill.center(), Modifiers::none());
        draw(cx);
        until(cx, "a session to go", |cx| {
            sessions(&workbench, cx).len() == left
        });
        if left == 1 {
            let (title, _) = header(&workbench, cx);
            let kill = cx.debug_bounds("icon-button:terminal-kill").unwrap();
            assert!(
                title.contains(&kill.center()),
                "kill moved to the dock strip"
            );
        }
    }
    assert!(!dock_shown(&workbench, cx));
}

#[gpui::test]
fn second_terminal_brings_the_session_row_back_and_closing_to_one_removes_it(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    draw(cx);
    let (title, _) = header(&workbench, cx);
    assert_eq!(session_top(cx), title.bottom());

    // Two: the row of tabs under the dock's strip, each with its mark,
    // with the strip's `+` and kill; the dock strip's go.
    new_session(&workbench, cx);
    draw(cx);
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1", "Terminal 2"]);
    let terminal = terminal(&workbench, cx);
    assert!(terminal.read_with(cx, |terminal, _| terminal.strip_drawn()));
    let marks = terminal.read_with(cx, |terminal, _| terminal.drawn_tab_marks().len());
    assert_eq!(marks, 2);
    let tab = tab_bounds(&workbench, 0, cx);
    assert_eq!(tab.top(), title.bottom());
    assert_eq!(session_top(cx), title.bottom() + TAB_HEIGHT);
    let (_, controls) = header(&workbench, cx);
    assert!(!controls.iter().any(|id| id.starts_with("terminal-")));
    let wrapper = cx.debug_bounds("terminal-new").expect("the strip's +");
    assert!(!title.contains(&wrapper.center()));
    assert!(active_and_focused(&workbench, 1, cx));

    // A double click on a tab asks for its name.
    cx.simulate_click(tab.center(), Modifiers::none());
    draw(cx);
    cx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: tab.center(),
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        button: gpui::MouseButton::Left,
        position: tab.center(),
        modifiers: Modifiers::none(),
        click_count: 2,
    });
    draw(cx);
    let asking = workbench.read_with(cx, |workbench, _| {
        workbench.palette == Some(crate::workbench::Palette::Rename)
    });
    assert!(asking, "a double click renames");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(active_and_focused(&workbench, 0, cx));

    // Back to one: the row goes, focus moves to the session left.
    run("terminal.close", cx);
    draw(cx);
    assert_eq!(sessions(&workbench, cx).len(), 1);
    assert!(!terminal.read_with(cx, |terminal, _| terminal.strip_drawn()));
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 2"]);
    assert_eq!(
        terminal.read_with(cx, |terminal, _| terminal.tab_bounds(0)),
        None
    );
    assert_eq!(session_top(cx), title.bottom());
    assert!(active_and_focused(&workbench, 0, cx));
    let (_, controls) = header(&workbench, cx);
    assert_eq!(
        &controls[..3],
        [
            "terminal-new-button",
            "terminal-profiles-button",
            "terminal-kill"
        ]
    );

    // And two again brings it back.
    new_session(&workbench, cx);
    draw(cx);
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 2", "Terminal 3"]);
    assert!(active_and_focused(&workbench, 1, cx));
}

/// R1 F2: a chord the user bound to `+` or kill shows the same in either
/// header's tooltip, the dock strip's with one terminal and the panel's
/// with two.
#[gpui::test]
fn a_chord_for_new_or_close_shows_the_same_in_both_headers(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let user = dir.path().join("keymap.toml");
    let chords = concat!(
        "[[bindings]]\nkeys = \"ctrl-k ctrl-w\"\ncommand = \"terminal.close\"\n",
        "context = \"workbench\"\n",
        "[[bindings]]\nkeys = \"ctrl-k ctrl-n\"\ncommand = \"terminal.new\"\n",
        "context = \"workbench\"\n",
    );
    std::fs::write(&user, chords).unwrap();
    let store = fe_keymap::KeymapStore::load(fe_keymap::DEFAULT, Some(user), None);
    assert_eq!(store.diagnostics(), []);
    let keymap = store.keymap().clone();
    cx.update(|_, cx| kiln_platform::keys::set_keymap(cx, keymap));
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    draw(cx);

    let tooltips = |cx: &mut VisualTestContext,
                    last: &dyn Fn(&mut VisualTestContext) -> Option<String>| {
        [
            "icon-button:terminal-new-button",
            "icon-button:terminal-kill",
        ]
        .map(|id| {
            let button = cx.debug_bounds(id).expect("drawn");
            hover(button, cx);
            let shown = last(cx).expect("a tooltip showed");
            // Moved off, so the next hover builds its own.
            cx.simulate_mouse_move(
                gpui::point(gpui::px(1.), gpui::px(1.)),
                None,
                Modifiers::none(),
            );
            draw(cx);
            shown
        })
    };
    let dock = tooltips(cx, &|cx| dock_tooltip(&workbench, cx));
    let kill_name = kiln_l10n::tr!("workbench-terminal-kill").to_string();
    // Kill's one key is the chord, which the tooltip shows.
    assert_ne!(dock[1], kill_name, "{dock:?}");
    assert!(dock[1].starts_with(&kill_name), "{dock:?}");

    new_session(&workbench, cx);
    draw(cx);
    let strip = tooltips(cx, &|_| crate::terminal_panel::last_strip_tooltip());
    assert_eq!(strip, dock);
}
