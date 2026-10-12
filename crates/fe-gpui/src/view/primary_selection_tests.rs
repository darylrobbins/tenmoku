//! The Linux primary selection, in a headless window: a selection is
//! written to it, and a middle click pastes from it.

use std::path::Path;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{
    ClipboardItem, Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use crate::*;

/// A focused editor on `text`, drawn, with the primary selection `on`.
fn editor<'a>(
    text: &str,
    on: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    cx.update(|cx| init_with_keymap(cx, Keymap::from_layers(vec![default.unwrap()])));
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("notes.txt".into()));
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_primary_selection(on);
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

/// Just right of the caret position at `offset`, mid-row, in the last frame.
fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    })
}

fn drag(view: &Entity<EditorView>, from: usize, to: usize, cx: &mut VisualTestContext) {
    let (from, to) = (at(view, from, cx), at(view, to, cx));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn primary(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text()))
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

#[gpui::test]
fn selections_are_written(cx: &mut TestAppContext) {
    let (view, cx) = editor("hello world\n", true, cx);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("clip".into())));
    drag(&view, 0, 5, cx);
    assert_eq!(primary(cx).as_deref(), Some("hello"));
    assert_eq!(clipboard(cx).as_deref(), Some("clip"));

    cx.simulate_keystrokes("shift-right");
    cx.run_until_parked();
    assert_eq!(primary(cx).as_deref(), Some("hello "));
    assert_eq!(clipboard(cx).as_deref(), Some("clip"));
}

#[gpui::test]
fn middle_click_pastes(cx: &mut TestAppContext) {
    let text = "abc\ndef\nghijkl\n";
    let (view, cx) = editor(text, true, cx);
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("xyz".into())));
    let position = at(&view, 8 + 3, cx);
    cx.simulate_mouse_down(position, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Middle, Modifiers::none());
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert_eq!(view.model().text().to_string(), "abc\ndef\nghixyzjkl\n");
        assert_eq!(view.model().primary_selection().range, 14..14);
        view.model_mut().undo().unwrap();
        assert_eq!(view.model().text().to_string(), text);
    });
}

#[gpui::test]
fn off_and_large(cx: &mut TestAppContext) {
    let (view, cx) = editor("hello world\n", false, cx);
    drag(&view, 0, 5, cx);
    assert_eq!(primary(cx), None);
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("xyz".into())));
    let position = at(&view, 3, cx);
    cx.simulate_mouse_down(position, MouseButton::Middle, Modifiers::none());
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert_eq!(view.model().text().to_string(), "hello world\n");
    });

    let big = "a".repeat((1 << 20) + 1);
    let (view, cx) = editor(&big, true, cx);
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("old".into())));
    view.update(cx, |view, cx| {
        view.model_mut().select_all();
        view.write_primary(cx);
    });
    assert_eq!(primary(cx).as_deref(), Some("old"));
}

fn middle_click(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) {
    let position = at(view, offset, cx);
    cx.simulate_mouse_down(position, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Middle, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn middle_click_leaves_visual_mode(cx: &mut TestAppContext) {
    for keys in ["v l l", "shift-v", "ctrl-v l"] {
        let (view, cx) = editor("abcdef\nghijkl\n", true, cx);
        cx.update(|_, cx| set_vim_enabled(cx, true));
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        // The visual selection took the primary selection; paste something else.
        cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("XY".into())));
        middle_click(&view, 7 + 3, cx);
        // Another frame, which syncs a visual extent if vim still held one.
        cx.run_until_parked();
        view.update(cx, |view, _| {
            assert_eq!(view.vim_mode(), Some("normal"), "{keys}");
            assert_eq!(view.model().text().to_string(), "abcdef\nghiXYjkl\n");
            assert_eq!(view.model().primary_selection().range.len(), 0, "{keys}");
        });
        // The next command doesn't act on the old extent.
        cx.simulate_keystrokes("x");
        cx.run_until_parked();
        view.update(cx, |view, _| {
            assert_eq!(
                view.model().text().to_string(),
                "abcdef\nghiXYkl\n",
                "{keys}"
            );
        });
    }
}

#[gpui::test]
fn middle_click_after_unused_autoindent(cx: &mut TestAppContext) {
    let (view, cx) = editor("    abc\ndef\nghi\n", true, cx);
    cx.update(|_, cx| {
        set_vim_enabled(cx, true);
        cx.write_to_primary(ClipboardItem::new_string("XY".into()));
    });
    // `o` opens an indented row, left unused when the session ends.
    cx.simulate_keystrokes("o");
    cx.run_until_parked();
    let before = view.read_with(cx, |view, _| view.model().text().to_string());
    assert_eq!(before, "    abc\n    \ndef\nghi\n");
    // The `g` of `ghi`, a later row than the autoindent.
    middle_click(&view, before.len() - 4, cx);
    view.update(cx, |view, _| {
        assert_eq!(view.model().text().to_string(), "    abc\n\ndef\nXYghi\n");
        assert_eq!(view.vim_mode(), Some("insert"));
        assert_eq!(view.model().primary_selection().range, 15..15);
    });
    // Leaving insert mode and undoing takes back the paste alone.
    cx.simulate_keystrokes("escape u");
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert_eq!(view.model().text().to_string(), "    abc\n\ndef\nghi\n");
    });
}
