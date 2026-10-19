//! `ctrl-t` and `ctrl-d` in vim's insert and replace mode shift the row's
//! indent, are recorded in a macro and replayed, on both platform keymaps.

use std::path::Path;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{EditorTheme, EditorView, init_with_keymap, set_vim_enabled};

fn editor<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    let map = Keymap::from_layers(vec![layer.unwrap()]);
    cx.update(|cx| init_with_keymap(cx, map));
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

const PLATFORMS: [(&str, &str); 2] = [("macOS", DEFAULT_MACOS), ("Linux", DEFAULT_LINUX)];

#[gpui::test]
fn ctrl_t_indents_in_insert_mode(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "foo");
        cx.simulate_keystrokes("i a ctrl-t b escape");
        assert_eq!(text(&view, cx), "    abfoo", "{os}");
        cx.simulate_keystrokes("u");
        assert_eq!(text(&view, cx), "foo", "{os}: one undo");
    }
}

#[gpui::test]
fn ctrl_d_outdents_and_zero_ctrl_d_clears(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "        foo");
        cx.simulate_keystrokes("$ i a ctrl-d b escape");
        assert_eq!(text(&view, cx), "    foabo", "{os}");
        let (view, cx) = editor(cx, source, "        foo");
        cx.simulate_keystrokes("A 0 ctrl-d x escape");
        assert_eq!(text(&view, cx), "foox", "{os}");
    }
}

#[gpui::test]
fn ctrl_d_in_replace_mode(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "    foo");
        cx.simulate_keystrokes("l l l l R a ctrl-d b escape");
        assert_eq!(text(&view, cx), "abo", "{os}");
    }
}

#[gpui::test]
fn a_macro_replays_the_indent_keys(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "foo");
        cx.simulate_keystrokes("q a i a ctrl-t b escape q");
        assert_eq!(text(&view, cx), "    abfoo", "{os}");
        cx.simulate_keystrokes("@ a");
        assert_eq!(text(&view, cx), "        aabbfoo", "{os}");
    }
}
