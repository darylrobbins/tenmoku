//! A which-key popup that shows keeps its prefix waiting: a chord the
#![allow(clippy::bool_assert_comparison)]
//! keymap matcher holds, and a vim map's keys, don't time out under it.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{
    EditorTheme, EditorView, init, keymap::CHORD_TIMEOUT, set_vim_enabled, set_vim_remaps,
    set_vim_which_key,
};
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

const DELAY: Duration = Duration::from_millis(200);

fn editor<'a>(
    text: &str,
    remaps: &[Remap],
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        set_vim_enabled(cx, true);
        set_vim_which_key(cx, true, DELAY);
    });
    let issues = cx.update(|_, cx| set_vim_remaps(cx, remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

fn noremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

#[gpui::test]
fn a_shown_chord_popup_outlasts_the_chord_timeout(cx: &mut TestAppContext) {
    let (view, cx) = editor("one two\nthree four", &[], cx);
    cx.simulate_keystrokes("j w g");
    wait(DELAY, cx);
    draw(cx);
    assert_eq!(cx.debug_bounds("which-key-entry-b").is_some(), true);
    wait(CHORD_TIMEOUT * 3, cx);
    draw(cx);
    assert_eq!(cx.debug_bounds("which-key-entry-b").is_some(), true);
    assert!(cx.update(|_, cx| kiln_platform::keys::chord_pending(cx)));
    cx.simulate_keystrokes("e");
    draw(cx);
    assert_eq!(cx.debug_bounds("which-key-popup").is_some(), false);
    assert_eq!(caret(&view, cx), (1, 4));
}

#[gpui::test]
fn a_shown_map_popup_outlasts_the_map_timeout(cx: &mut TestAppContext) {
    let remaps = [
        noremap("<leader>ff", "gg"),
        noremap("<leader>w", "<Cmd>fe:editor.undo<CR>"),
    ];
    let (view, cx) = editor("one\ntwo\nthree\nfour\nfive\nsix", &remaps, cx);
    cx.simulate_keystrokes("j j ,");
    wait(DELAY, cx);
    draw(cx);
    assert_eq!(cx.debug_bounds("which-key-entry-f").is_some(), true);
    wait(CHORD_TIMEOUT * 3, cx);
    draw(cx);
    assert_eq!(cx.debug_bounds("which-key-entry-f").is_some(), true);
    cx.simulate_keystrokes("f f");
    draw(cx);
    assert_eq!(cx.debug_bounds("which-key-popup").is_some(), false);
    assert_eq!(caret(&view, cx).0, 0);
}
