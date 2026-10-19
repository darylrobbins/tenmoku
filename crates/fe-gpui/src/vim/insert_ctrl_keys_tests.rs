//! Insert mode's `ctrl-o` in a headless window: one normal-mode command,
//! with the block cursor and the normal-mode key context while it runs,
//! then insert mode again, on both platform keymaps.

use std::path::Path;

use fe_command::ContextFrame;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, KeymapLayer};

use super::*;
use crate::{init_with_keymap, keymap::context_stack};

const PLATFORMS: [(&str, &str); 2] = [("macOS", DEFAULT_MACOS), ("Linux", DEFAULT_LINUX)];

/// A focused editor showing `text`, vim on, with `source` as the keymap.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    let map = fe_keymap::Keymap::from_layers(vec![layer.unwrap()]);
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
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

/// The editor's frame of the key context the window draws now.
fn drawn(cx: &mut VisualTestContext) -> ContextFrame {
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        context_stack(&window.context_stack())
            .frames()
            .iter()
            .rfind(|frame| frame.name == "editor")
            .expect("an editor frame")
            .clone()
    })
}

fn shape(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> CaretShape {
    view.read_with(cx, |view, _| view.caret_shape())
}

#[gpui::test]
fn ctrl_o_runs_one_command_with_the_block_cursor(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "abc def");
        cx.simulate_keystrokes("A x ctrl-o");
        assert_eq!(mode(&view, cx), Some("normal"), "{os}");
        assert_eq!(shape(&view, cx), CaretShape::Block, "{os}");
        let frame = drawn(cx);
        assert_eq!(
            frame.values.get("vim.mode").map(String::as_str),
            Some("normal"),
            "{os}"
        );
        assert!(
            frame.flags.contains("vim.insert_pending"),
            "{os}: {frame:?}"
        );
        // At the row's end, the block is on its last character.
        assert_eq!(state(&view, cx), ("abc defx".into(), (0, 7)), "{os}");

        cx.simulate_keystrokes("0");
        assert_eq!(mode(&view, cx), Some("insert"), "{os}");
        assert_eq!(shape(&view, cx), CaretShape::Bar, "{os}");
        let frame = drawn(cx);
        assert_eq!(
            frame.values.get("vim.mode").map(String::as_str),
            Some("insert"),
            "{os}"
        );
        assert!(!frame.flags.contains("vim.insert_pending"), "{os}");

        cx.simulate_keystrokes("y escape");
        assert_eq!(state(&view, cx), ("yabc defx".into(), (0, 0)), "{os}");
        // Two undo steps, as in nvim: the text typed on each side.
        cx.simulate_keystrokes("u");
        assert_eq!(state(&view, cx).0, "abc defx", "{os}");
        cx.simulate_keystrokes("u");
        assert_eq!(state(&view, cx).0, "abc def", "{os}");
    }
}

#[gpui::test]
fn escape_after_ctrl_o_returns_to_insert_mode(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "abc def");
        cx.simulate_keystrokes("A ctrl-o escape");
        assert_eq!(mode(&view, cx), Some("insert"), "{os}");
        cx.simulate_keystrokes("x escape");
        assert_eq!(text_of(&view, cx), "abc defx", "{os}");
        // A count and an operator belong to the command; `escape` drops them
        // and returns.
        cx.simulate_keystrokes("i ctrl-o 2 d escape");
        assert_eq!(mode(&view, cx), Some("insert"), "{os}");
        cx.simulate_keystrokes("ctrl-o d w escape");
        assert_eq!(text_of(&view, cx), "abc def", "{os}");
        // So does a key vim has no name for.
        cx.simulate_keystrokes("i ctrl-o d f1");
        assert_eq!(mode(&view, cx), Some("insert"), "{os}");
        cx.simulate_keystrokes("escape");
    }
}

#[gpui::test]
fn a_macro_replays_ctrl_o(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        let (view, cx) = editor(cx, source, "abc\nabc");
        cx.simulate_keystrokes("q a A ctrl-o 0 x escape j q");
        assert_eq!(text_of(&view, cx), "xabc\nabc", "{os}");
        cx.simulate_keystrokes("@ a");
        assert_eq!(text_of(&view, cx), "xabc\nxabc", "{os}");
    }
}

fn text_of(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    state(view, cx).0
}
