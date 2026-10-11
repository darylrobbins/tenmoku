//! `@` replays a macro in a headless window: the keys it leaves are fed
//! through the vim host, in the keystroke that ran it.

use fe_buffer::{Buffer, Point};
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    EditorTheme, EditorView, init,
    vim::{self, set_vim_enabled},
};

/// A focused editor showing `text`, vim on, the caret at `at`.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    at: (u32, u32),
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let _ = vim::registry;
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        let offset = model.text().point_to_offset(Point::new(at.0, at.1));
        model.set_cursor(offset);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

/// The text, and the primary caret's head as a row and a byte column.
fn state(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (String, (u32, u32)) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (model.text().to_string(), (point.row, point.column))
    })
}

#[gpui::test]
fn recorded_macro_replays(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abc\nabc\nabc", (0, 0));
    cx.simulate_keystrokes("q a x j q");
    assert_eq!(state(&view, cx), ("bc\nabc\nabc".into(), (1, 0)));
    cx.simulate_keystrokes("@ a");
    assert_eq!(state(&view, cx), ("bc\nbc\nabc".into(), (2, 0)));
    cx.simulate_keystrokes("@ @");
    assert_eq!(state(&view, cx).0, "bc\nbc\nbc");
}

#[gpui::test]
fn replay_types_in_insert_mode(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "Axy\nfoo", (0, 0));
    // Register `a` holds `Axy`.
    cx.simulate_keystrokes("\" a y $ j");
    cx.simulate_keystrokes("@ a");
    assert_eq!(state(&view, cx).0, "Axy\nfooxy");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
}

#[gpui::test]
fn failing_key_stops_replay(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abc\njx", (1, 0));
    // Register `a` holds `jx`.
    cx.simulate_keystrokes("\" a y $");
    cx.simulate_keystrokes("@ a");
    assert_eq!(state(&view, cx).0, "abc\njx");
    cx.simulate_keystrokes("x");
    assert_eq!(state(&view, cx).0, "abc\nx");
}

#[gpui::test]
fn play_command_replays(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abc\nabc", (0, 0));
    cx.simulate_keystrokes("q a x q");
    let play = fe_command::Invocation::of("vim.play")
        .unwrap()
        .args(serde_json::json!({ "register": "a" }));
    cx.update(|window, cx| window.dispatch_action(Box::new(crate::commands::RunCommand(play)), cx));
    cx.run_until_parked();
    assert_eq!(state(&view, cx).0, "c\nabc");
}

#[gpui::test]
fn command_line_stops_replay(cx: &mut TestAppContext) {
    // Register `a` holds `x:`: the line opens as its last key.
    let (view, cx) = vim_editor(cx, "x:\nabc", (0, 0));
    cx.simulate_keystrokes("\" a y $ j @ a");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("cmdline")
    );
    assert_eq!(state(&view, cx).0, "x:\nbc");
}
