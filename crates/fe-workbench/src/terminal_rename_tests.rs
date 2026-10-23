//! `terminal.rename`: a name the user chose for a session, in place of its
//! shell's title.

use gpui::{Modifiers, MouseButton, SharedString, TestAppContext};

use super::terminal_tabs_tests::{
    draw, drawn_tabs, ids, opened, run, tab_bounds, type_line, until, with_sessions,
};
use crate::workbench::Palette;

fn labels(
    workbench: &gpui::Entity<crate::Workbench>,
    cx: &mut gpui::VisualTestContext,
) -> Vec<String> {
    drawn_tabs(workbench, cx)
        .iter()
        .map(SharedString::to_string)
        .collect()
}

fn asking(workbench: &gpui::Entity<crate::Workbench>, cx: &mut gpui::VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| {
        workbench.palette == Some(Palette::Rename)
    })
}

/// Empties the name input.
fn clear_input(cx: &mut gpui::VisualTestContext) {
    for _ in 0..24 {
        cx.simulate_keystrokes("backspace");
    }
}

#[gpui::test]
fn rename_applies_cancels_and_clears(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    with_sessions(&workbench, 1, cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Terminal 2"]);

    // Asks with the active session's label.
    run("terminal.rename", cx);
    assert!(asking(&workbench, cx));
    let picker = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        layer
            .modal_view()
            .unwrap()
            .clone()
            .downcast::<crate::picker::Picker<crate::terminal_panel::RenameTerminal>>()
            .unwrap()
    });
    assert_eq!(
        picker.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned()),
        "Terminal 2"
    );

    // Escape leaves the name alone.
    cx.simulate_input("x");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!asking(&workbench, cx));
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Terminal 2"]);

    // Enter applies it.
    run("terminal.rename", cx);
    clear_input(cx);
    cx.simulate_input("build");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "build"]);

    // Asks with the name, and an empty answer clears it.
    run("terminal.rename", cx);
    let text = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        layer
            .modal_view()
            .unwrap()
            .clone()
            .downcast::<crate::picker::Picker<crate::terminal_panel::RenameTerminal>>()
            .unwrap()
    });
    assert_eq!(
        text.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned()),
        "build"
    );
    clear_input(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Terminal 2"]);
}

#[gpui::test]
fn custom_name_wins_over_shell_title(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 2, cx);

    // The third, active, is named after its shell set a title.
    type_line(&all[2], r"printf '\033]0;three\007'", cx);
    until(cx, "the third's title", |cx| {
        labels(&workbench, cx)[2] == "three"
    });
    run("terminal.rename", cx);
    clear_input(cx);
    cx.simulate_input("mine");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "Terminal 2", "mine"]);

    // Other sessions keep their own labels, and a new title doesn't
    // replace the name.
    type_line(&all[1], r"printf '\033]0;two\007'", cx);
    until(cx, "the second's title", |cx| {
        labels(&workbench, cx)[1] == "two"
    });
    type_line(&all[2], r"printf '\033]0;newer\007'; echo done", cx);
    until(cx, "the third's output", |cx| {
        all[2].read_with(cx, |view, _| view.title() == Some("newer"))
    });
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "two", "mine"]);

    // It goes with its session: the next one started is a new one.
    run("terminal.close", cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "two"]);
    run("terminal.new", cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "two", "Terminal 4"]);
    assert_eq!(ids(&workbench, cx).len(), 3);
}

#[gpui::test]
fn double_click_renames_its_row(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    with_sessions(&workbench, 1, cx);
    let first = tab_bounds(&workbench, 0, cx).center();
    cx.simulate_click(first, Modifiers::none());
    draw(cx);
    assert!(!asking(&workbench, cx));
    cx.simulate_event(gpui::MouseDownEvent {
        position: first,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: first,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
    });
    draw(cx);
    assert!(asking(&workbench, cx));
    clear_input(cx);
    cx.simulate_input("front");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["front", "Terminal 2"]);
}

#[gpui::test]
fn enter_keeps_the_shown_name_and_refocuses(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 1, cx);
    run("terminal.rename", cx);
    clear_input(cx);
    cx.simulate_input("keep");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "keep"]);

    // Confirming the prefilled name unedited leaves it, and the session
    // has focus again.
    run("terminal.rename", cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(labels(&workbench, cx), ["Terminal 1", "keep"]);
    assert!(cx.update(|window, cx| {
        use gpui::Focusable as _;
        all[1].focus_handle(cx).is_focused(window)
    }));
}
