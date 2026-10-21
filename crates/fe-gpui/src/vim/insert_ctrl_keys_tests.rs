//! Insert mode's ctrl keys in a headless window, on both platform keymaps:
//! `ctrl-o` runs one normal-mode command, with the block cursor and the
//! normal-mode key context while it runs, then insert mode again; `ctrl-w`
//! and `ctrl-u` delete before the caret, in replace mode too.

use std::{cell::RefCell, path::Path, rc::Rc};

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

/// `ctrl-w` and `ctrl-u` edit as `tests/corpus/insert_keys.toml`'s cases
/// in nvim, on both platform keymaps: off macOS the keymap hands them to
/// vim, on macOS they reach the editor unmatched.
#[gpui::test]
fn ctrl_w_and_ctrl_u_reach_vim_in_insert_mode(cx: &mut TestAppContext) {
    for (os, source) in PLATFORMS {
        // (buffer, keys, buffer after, cursor after): the corpus's cases.
        for (text, keys, after, cursor) in [
            // ctrl_w_word
            (
                "foo bar",
                "A space b a z space q u x ctrl-w x escape",
                "foo bar baz x",
                (0, 12),
            ),
            // ctrl_w_stops_once_at_insert_start
            (
                "foo bar",
                "A space b a z ctrl-w ctrl-w ctrl-w escape",
                "foo ",
                (0, 3),
            ),
            // ctrl_w_joins_rows
            ("  foo\nbar", "j i ctrl-w escape", "  foobar", (0, 4)),
            // ctrl_u_typed_text
            (
                "foo bar",
                "l l l a space x y ctrl-u escape",
                "foo bar",
                (0, 3),
            ),
            // ctrl_u_nothing_typed
            ("foo bar", "l l l a ctrl-u escape", "bar", (0, 0)),
            // ctrl_u_twice
            (
                "foo bar",
                "A space b a z ctrl-u ctrl-u x escape",
                "x",
                (0, 0),
            ),
            // ctrl_w_replace_mode
            (
                "foo bar baz",
                "w shift-r x y space z ctrl-w ctrl-w q escape",
                "foo qar baz",
                (0, 4),
            ),
            // ctrl_u_replace_mode
            (
                "foo bar",
                "w shift-r x y z ctrl-u q escape",
                "foo qar",
                (0, 4),
            ),
            // ctrl_w_replace_before_start
            ("foo bar", "w shift-r ctrl-w x escape", "xoo bar", (0, 0)),
            // ctrl_w_dot_repeat
            (
                "foo bar\nbaz qux",
                "A ctrl-w x escape j .",
                "foo x\nbaz x",
                (1, 4),
            ),
        ] {
            let (view, cx) = editor(cx, source, text);
            cx.simulate_keystrokes(keys);
            assert_eq!(state(&view, cx), (after.into(), cursor), "{os}: {keys}");
            assert_eq!(mode(&view, cx), Some("normal"), "{os}: {keys}");
        }

        // ctrl_w_in_macro: the recording holds `<C-w>`, and replays it.
        let (view, cx) = editor(cx, source, "foo bar\nbaz qux");
        cx.simulate_keystrokes("q a A space q u u x ctrl-w x escape q");
        assert_eq!(text_of(&view, cx), "foo bar x\nbaz qux", "{os}");
        let recorded = cx.update(|_, cx| {
            let registers = &cx.global::<Vim>().registers;
            registers.get('a').map(|register| register.text.clone())
        });
        assert_eq!(recorded.as_deref(), Some("A quux\u{17}x\u{1b}"), "{os}");
        cx.simulate_keystrokes("j @ a");
        assert_eq!(text_of(&view, cx), "foo bar x\nbaz qux x", "{os}");
        // ctrl_u_in_macro, and replace mode's keys in one.
        cx.simulate_keystrokes("q b 0 a ctrl-u y escape w shift-r z z ctrl-w w escape q");
        assert_eq!(text_of(&view, cx), "foo bar x\nyaz wux x", "{os}");
        cx.simulate_keystrokes("k @ b");
        assert_eq!(text_of(&view, cx), "yoo war x\nyaz wux x", "{os}");

        // In normal mode `ctrl-w h` is still the window key: the keymap
        // dispatches `pane.focus_left`, which no pane takes here.
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        cx.update(|_, cx| {
            cx.on_action(move |action: &crate::commands::RunCommand, _| {
                sink.borrow_mut().push(action.0.command.as_str().to_owned());
            });
        });
        cx.simulate_keystrokes("ctrl-w h");
        assert!(
            seen.borrow()
                .iter()
                .any(|command| command == "pane.focus_left"),
            "{os}: {:?}",
            seen.borrow()
        );
        assert_eq!(text_of(&view, cx), "yoo war x\nyaz wux x", "{os}");
    }
}

fn text_of(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    state(view, cx).0
}
