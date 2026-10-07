//! Insert mode in a headless window: `escape` and `ctrl-[` back to normal
//! mode, typed text as one undo step, the editor's keys, and the session
//! split by other editor commands and mouse clicks.

use fe_buffer::{Buffer, Point};
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_MACOS, KeySequence, KeymapStore};
use gpui::{
    Entity, EntityInputHandler as _, Focusable as _, Modifiers, Pixels, TestAppContext,
    VisualTestContext,
};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// A focused editor showing `text` with a caret at byte `caret`, vim on.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
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

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

fn set_caret(view: &Entity<EditorView>, at: (u32, u32), cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        let model = view.model_mut();
        let offset = model.text().point_to_offset(Point::new(at.0, at.1));
        model.set_cursor(offset);
    });
}

/// The window position of a caret at `offset`, in the frame drawn now.
fn position_of(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> gpui::Point<Pixels> {
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        view.update(cx, |view, cx| {
            let utf16 = view.model().text().offset_to_utf16(offset);
            view.bounds_for_range(utf16..utf16, Default::default(), window, cx)
                .expect("visible")
                .center()
        })
    })
}

#[gpui::test]
fn insert_session_round_trip(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one two", 4);
    assert_eq!(mode(&view, cx), Some("normal"));

    cx.simulate_keystrokes("i");
    assert_eq!(mode(&view, cx), Some("insert"));
    // A bar, not a block.
    assert!(!view.read_with(cx, |view, _| view.block_cursor()));
    cx.simulate_input("hello");
    cx.simulate_keystrokes("escape");
    assert_eq!(mode(&view, cx), Some("normal"));
    // The caret steps back onto the `o`.
    assert_eq!(state(&view, cx), ("one hellotwo".into(), (0, 8)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 4)));

    // `ctrl-[` ends a session as `escape` does.
    cx.simulate_keystrokes("i");
    cx.simulate_input("x");
    cx.simulate_keystrokes("ctrl-[");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(state(&view, cx), ("one xtwo".into(), (0, 4)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 4)));

    // A count types the text again at `escape`, all one undo step.
    cx.simulate_keystrokes("3 i x escape");
    assert_eq!(state(&view, cx), ("one xxxtwo".into(), (0, 6)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 4)));

    // Text an input method marks and then commits joins the session.
    cx.simulate_keystrokes("i");
    cx.simulate_input("a");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "k", Some(1..1), window, cx);
            view.replace_and_mark_text_in_range(None, "ka", Some(2..2), window, cx);
            view.replace_text_in_range(None, "か", window, cx);
        })
    });
    cx.simulate_input("b");
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("one aかbtwo".into(), (0, 8)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 4)));
}

#[gpui::test]
fn insert_mode_keeps_the_macos_keys(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one two", 0);

    cx.simulate_keystrokes("shift-a");
    assert_eq!(mode(&view, cx), Some("insert"));
    cx.simulate_input(" three");
    cx.simulate_keystrokes("alt-backspace");
    assert_eq!(state(&view, cx), ("one two ".into(), (0, 8)));
    // `j` and `k` are text here, through the keymap or as input.
    cx.simulate_keystrokes("j k");
    cx.simulate_input("jk");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("x");
    assert_eq!(state(&view, cx), ("one two jkjk\nx".into(), (1, 1)));
    assert_eq!(mode(&view, cx), Some("insert"));
    // `cmd-z` ends the session and undoes it, still in insert mode.
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(state(&view, cx).0, "one two");
    assert_eq!(mode(&view, cx), Some("insert"));

    // The input handler is gone in normal mode: input types nothing.
    cx.simulate_keystrokes("escape");
    assert_eq!(mode(&view, cx), Some("normal"));
    let before = state(&view, cx);
    cx.simulate_input("QK");
    assert_eq!(state(&view, cx), before);

    // `enter` is a typed line break: the indentation it copies goes at
    // `escape` if nothing was typed after it, as with 'autoindent'.
    view.update(cx, |view, _| {
        let model = view.model_mut();
        model.select_all();
        model.insert("    a");
    });
    cx.simulate_keystrokes("shift-a enter escape");
    assert_eq!(state(&view, cx), ("    a\n".into(), (1, 0)));
}

/// What nvim 0.12.4 (`nvim --clean`, recorded by hand) leaves after each
/// step from `one two` with the cursor at (0, 4), as text and cursor:
///
/// | keys                | text          | cursor |
/// |---------------------|---------------|--------|
/// | `iab<Left>cd<Esc>`  | `one acdbtwo` | (0, 6) |
/// | then `u`            | `one abtwo`   | (0, 5) |
/// | then `.`            | `one acdbtwo` | (0, 6) |
/// | `3iab<Left>cd<Esc>` | `one acdbtwo` | (0, 6) |
/// | `iab<Left><Esc>`    | `one abtwo`   | (0, 4) |
/// | then `u`            | `one two`     | (0, 4) |
/// | then `.`            | `one abtwo`   | (0, 5) |
/// | `iab<Left><BS><Esc>`| `one btwo`    | (0, 3) |
/// | then `0.`           | `one btwo`    | (0, 0) |
/// | `Aab<Right>cd<Esc>` | `one twoabcd` | (0, 10)|
/// | then `u`            | `one two`     | (0, 6) |
///
/// and from `    a` / `b` with the cursor at (0, 4):
///
/// | keys           | text              | cursor |
/// |----------------|-------------------|--------|
/// | `o<Up><Esc>`   | `    a` / `` / `b` | (0, 3) |
/// | then `u`       | `    a` / `b`      | (0, 4) |
/// | `o<Left><Esc>` | `    a` / `    ` / `b` | (1, 2) |
///
/// So a cursor key that moves ends the session as an undo step, drops its
/// count and the indentation it made on rows the cursor left, and starts a
/// new one that `.` repeats as `i`, unless it changes nothing. One that
/// can't move doesn't split it.
#[gpui::test]
fn other_commands_split_the_session(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "one two", 4);
    let split_by_left = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("left");
        assert_eq!(mode(&view, cx), Some("insert"));
    };

    cx.simulate_keystrokes("i");
    cx.simulate_input("ab");
    split_by_left(cx);
    cx.simulate_input("cd");
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("one acdbtwo".into(), (0, 6)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one abtwo".into(), (0, 5)));
    cx.simulate_keystrokes(".");
    assert_eq!(state(&view, cx), ("one acdbtwo".into(), (0, 6)));

    // A mouse click inside the session splits it the same way.
    let reset = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let model = view.model_mut();
            model.select_all();
            model.insert("one two");
        });
        set_caret(&view, (0, 4), cx);
    };
    reset(cx);
    cx.simulate_keystrokes("i");
    cx.simulate_input("ab");
    let between = position_of(&view, 5, cx);
    cx.simulate_click(between, Modifiers::none());
    assert_eq!(mode(&view, cx), Some("insert"));
    assert_eq!(state(&view, cx), ("one abtwo".into(), (0, 5)));
    cx.simulate_input("cd");
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("one acdbtwo".into(), (0, 6)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one abtwo".into(), (0, 5)));
    cx.simulate_keystrokes(".");
    assert_eq!(state(&view, cx), ("one acdbtwo".into(), (0, 6)));

    // A click that leaves the caret where it is doesn't split it.
    reset(cx);
    cx.simulate_keystrokes("i");
    cx.simulate_input("ab");
    let at_caret = position_of(&view, 6, cx);
    cx.simulate_click(at_caret, Modifiers::none());
    cx.simulate_input("cd");
    cx.simulate_keystrokes("escape u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 4)));

    // A click past the row `o` opened lands where it was aimed, though the
    // split drops the row's unused indentation before it (as nvim does once
    // the cursor leaves the row).
    view.update(cx, |view, _| {
        let model = view.model_mut();
        model.select_all();
        model.insert("    a\nbcd");
    });
    set_caret(&view, (0, 4), cx);
    cx.simulate_keystrokes("o");
    assert_eq!(state(&view, cx), ("    a\n    \nbcd".into(), (1, 4)));
    let on_c = position_of(&view, 12, cx);
    cx.simulate_click(on_c, Modifiers::none());
    assert_eq!(state(&view, cx), ("    a\n\nbcd".into(), (2, 1)));
    assert_eq!(mode(&view, cx), Some("insert"));
    cx.simulate_keystrokes("escape");

    // The split drops the count.
    reset(cx);
    cx.simulate_keystrokes("3 i");
    cx.simulate_input("ab");
    split_by_left(cx);
    cx.simulate_input("cd");
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("one acdbtwo".into(), (0, 6)));

    // A session that types nothing after the split leaves no undo step, and
    // `.` the change before it.
    reset(cx);
    cx.simulate_keystrokes("i");
    cx.simulate_input("ab");
    split_by_left(cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("one abtwo".into(), (0, 4)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 4)));
    cx.simulate_keystrokes(".");
    assert_eq!(state(&view, cx), ("one abtwo".into(), (0, 5)));

    // A session after the split that only deletes is what `.` repeats.
    reset(cx);
    cx.simulate_keystrokes("i");
    cx.simulate_input("ab");
    split_by_left(cx);
    cx.simulate_keystrokes("backspace escape");
    assert_eq!(state(&view, cx), ("one btwo".into(), (0, 3)));
    cx.simulate_keystrokes("0 .");
    assert_eq!(state(&view, cx), ("one btwo".into(), (0, 0)));

    // A cursor key that can't move doesn't split it.
    reset(cx);
    cx.simulate_keystrokes("shift-a");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("right");
    cx.simulate_input("cd");
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("one twoabcd".into(), (0, 10)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("one two".into(), (0, 6)));

    // Leaving the row `o` opened drops its unused indentation, in the same
    // undo step, and the caret keeps its column; moving along the row keeps
    // it.
    let indented = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let model = view.model_mut();
            model.select_all();
            model.insert("    a\nb");
        });
        set_caret(&view, (0, 4), cx);
    };
    indented(cx);
    cx.simulate_keystrokes("o up");
    assert_eq!(mode(&view, cx), Some("insert"));
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&view, cx), ("    a\n\nb".into(), (0, 3)));
    cx.simulate_keystrokes("u");
    assert_eq!(state(&view, cx), ("    a\nb".into(), (0, 4)));
    indented(cx);
    cx.simulate_keystrokes("o left escape");
    assert_eq!(state(&view, cx), ("    a\n    \nb".into(), (1, 2)));
}

#[gpui::test]
fn escape_bindings(cx: &mut TestAppContext) {
    let store = KeymapStore::load(DEFAULT_MACOS, None, None);
    let insert: Vec<_> = store
        .keymap()
        .bindings()
        .iter()
        .filter(|binding| {
            binding.context.as_ref().map(ToString::to_string).as_deref()
                == Some("editor && vim.mode == insert")
        })
        .map(|binding| {
            (
                binding.keys.clone(),
                binding.command.as_str(),
                binding.args.clone(),
            )
        })
        .collect();
    let normal_mode = |keys: &str| {
        (
            KeySequence::parse(keys).unwrap(),
            "vim.normal_mode",
            serde_json::Value::Null,
        )
    };
    assert_eq!(insert, [normal_mode("escape"), normal_mode("ctrl-[")]);

    // With vim off, `escape` in an editor changes nothing.
    let (view, cx) = vim_editor(cx, "one two", 4);
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    let selections = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            (view.model().text().to_string(), view.model().selections())
        })
    };
    let before = selections(cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(selections(cx), before);
    assert_eq!(mode(&view, cx), None);
}
