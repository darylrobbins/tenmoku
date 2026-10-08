//! Vim's visual modes in a headless window: their keys, the selection and
//! the block cursor as painted, mouse drags and clicks, and the default
//! keymap's visual layer.

use std::ops::Range;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_MACOS, KeymapStore};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, Pixels, Point,
    TestAppContext, VisualTestContext, px,
};

use super::*;
use crate::{EditorTheme, EditorView, commands::command_catalog, init};

/// A focused editor showing `text` with a caret at byte `caret`, vim on.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (view, cx) = editor_vim_off(cx, text, caret);
    set_vim(true, cx);
    (view, cx)
}

fn editor_vim_off<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    open(cx, text, caret)
}

/// Another focused editor, in a window of its own.
fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

fn set_vim(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_vim_enabled(cx, on));
    cx.run_until_parked();
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    view.read_with(cx, |view, _| view.model().primary_selection().range)
}

fn vim_cursor(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<usize> {
    view.read_with(cx, |view, _| view.vim.as_ref()?.visual_cursor(view.model()))
}

fn unnamed(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        let registers = &cx.default_global::<Vim>().registers;
        registers.get('"').map(|register| register.text.clone())
    })
}

/// Draws a frame: the selections and the block carets it painted.
fn painted(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        (view.painted_selections.clone(), view.painted_carets.clone())
    })
}

/// The window bounds of a bar caret at `offset`, in the last frame.
fn bar_at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

/// A point just inside the grapheme at `offset`, in the last frame.
fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bar = bar_at(view, offset, cx);
    bar.origin + gpui::point(px(1.), bar.size.height / 2.)
}

fn space_width(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    cx.update(|_, cx| {
        let theme = view.read(cx).theme.clone();
        let text_system = cx.text_system();
        let font = text_system.resolve_font(&gpui::font(theme.font_family));
        text_system.layout_width(font, theme.font_size, ' ')
    })
}

fn close(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < px(0.01)
}

#[gpui::test]
fn visual_keys_in_the_editor(cx: &mut TestAppContext) {
    let original = "one two\nthree\nfour";
    let (view, cx) = editor(cx, original, 0);

    // v e y: yanks the word, back to normal mode.
    cx.simulate_keystrokes("v e y");
    assert_eq!(unnamed(cx).as_deref(), Some("one"));
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(text(&view, cx), original);

    // V j d: both rows, one undo step.
    cx.simulate_keystrokes("V j d");
    assert_eq!(text(&view, cx), "four");
    assert_eq!(mode(&view, cx), Some("normal"));
    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), original);

    // ctrl-v j l d: a 2x2 block.
    cx.simulate_keystrokes("g g 0 ctrl-v j l d");
    assert_eq!(text(&view, cx), "e two\nree\nfour");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), original);

    // The arrows extend the selection a grapheme each; escape collapses it.
    cx.simulate_keystrokes("g g 0 v");
    assert_eq!(selection(&view, cx), 0..1);
    cx.simulate_keystrokes("right right");
    assert_eq!(selection(&view, cx), 0..3);
    assert_eq!(mode(&view, cx), Some("visual"));
    cx.simulate_keystrokes("escape");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert!(selection(&view, cx).is_empty());

    // Typing types nothing in any visual mode: no input handler. Not
    // "abc": in a visual mode `ab` is a text object and `c` changes the
    // selection, as below. `Q` and `K` are keys vim leaves alone there.
    for (key, name) in [
        ("v", "visual"),
        ("V", "visual_line"),
        ("ctrl-v", "visual_block"),
    ] {
        cx.simulate_keystrokes(key);
        assert_eq!(mode(&view, cx), Some(name));
        cx.simulate_input("QK");
        assert_eq!(text(&view, cx), original, "{name}");
        cx.simulate_keystrokes("escape");
        assert_eq!(mode(&view, cx), Some("normal"), "{name}");
    }

    // v c: insert mode, which types.
    cx.simulate_keystrokes("g g 0 v c");
    assert_eq!(mode(&view, cx), Some("insert"));
    cx.simulate_input("xyz");
    assert_eq!(text(&view, cx), "xyzne two\nthree\nfour");
    cx.simulate_keystrokes("escape u");
    assert_eq!(text(&view, cx), original);

    // cmd-c copies the selection and stays in visual mode.
    cx.simulate_keystrokes("g g 0 v e cmd-c");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("one"));
    assert_eq!(mode(&view, cx), Some("visual"));
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn ctrl_v_and_ctrl_q_reach_the_reducer(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two\nthree", 0);
    for key in ["ctrl-v", "ctrl-q"] {
        cx.simulate_keystrokes(key);
        assert_eq!(mode(&view, cx), Some("visual_block"), "{key}");
        cx.simulate_keystrokes("v");
        assert_eq!(mode(&view, cx), Some("visual"), "{key}");
        cx.simulate_keystrokes(key);
        assert_eq!(mode(&view, cx), Some("visual_block"), "{key}");
        cx.simulate_keystrokes(key);
        assert_eq!(mode(&view, cx), Some("normal"), "{key}");
    }
}

#[gpui::test]
fn cursor_paints_on_the_last_grapheme(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "a漢b", 0);

    // The selection takes a and 漢; the block is on 漢, not b.
    cx.simulate_keystrokes("v l");
    assert_eq!(selection(&view, cx), 0..4);
    let (selections, blocks) = painted(&view, cx);
    let (at_0, at_1, at_4) = (
        bar_at(&view, 0, cx),
        bar_at(&view, 1, cx),
        bar_at(&view, 4, cx),
    );
    assert_eq!(selections.len(), 1);
    assert!(
        close(selections[0].origin.x, at_0.origin.x),
        "{selections:?}"
    );
    assert!(
        close(selections[0].right(), at_4.origin.x),
        "{selections:?}"
    );
    assert_eq!(blocks.len(), 1);
    assert!(close(blocks[0].origin.x, at_1.origin.x), "{blocks:?}");
    assert!(close(blocks[0].right(), at_4.origin.x), "{blocks:?}");

    // V: on the vim cursor's column, though the selection is the row.
    cx.simulate_keystrokes("V");
    assert_eq!(mode(&view, cx), Some("visual_line"));
    assert_eq!(selection(&view, cx), 0..5);
    let (_, blocks) = painted(&view, cx);
    assert_eq!(blocks.len(), 1);
    assert!(close(blocks[0].origin.x, at_1.origin.x), "{blocks:?}");
    assert!(close(blocks[0].right(), at_4.origin.x), "{blocks:?}");
    cx.simulate_keystrokes("escape");

    // A block: on the cursor's corner.
    let (view, cx) = open(cx, "abc\nabc\nabc", 0);
    cx.simulate_keystrokes("l ctrl-v j j l");
    assert_eq!(vim_cursor(&view, cx), Some(10));
    let (_, blocks) = painted(&view, cx);
    let (at_10, at_11) = (bar_at(&view, 10, cx), bar_at(&view, 11, cx));
    assert_eq!(blocks.len(), 1);
    assert!(close(blocks[0].origin.x, at_10.origin.x), "{blocks:?}");
    assert!(close(blocks[0].origin.y, at_10.origin.y), "{blocks:?}");
    assert!(close(blocks[0].right(), at_11.origin.x), "{blocks:?}");
    cx.simulate_keystrokes("escape");

    // An empty row inside V: one space wide.
    let (view, cx) = open(cx, "ab\n\ncd", 0);
    cx.simulate_keystrokes("V j j");
    let (selections, _) = painted(&view, cx);
    let row_1 = bar_at(&view, 3, cx);
    let on_row_1: Vec<_> = selections
        .iter()
        .filter(|rect| close(rect.origin.y, row_1.origin.y))
        .collect();
    assert_eq!(on_row_1.len(), 1, "{selections:?}");
    assert!(close(on_row_1[0].origin.x, row_1.origin.x), "{on_row_1:?}");
    let space = space_width(&view, cx);
    assert!(
        close(on_row_1[0].size.width, space),
        "{on_row_1:?} {space:?}"
    );
}

fn click(cx: &mut VisualTestContext, position: Point<Pixels>, count: usize) {
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: count,
        first_mouse: false,
    });
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
}

/// Down at `from`, a small move, then a move to `to` and up there.
fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn mouse_drags_and_clicks(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two\nthree", 6);

    // A drag in normal mode: charwise visual, the cursor on the grapheme
    // under the pointer.
    let (from, to) = (at(&view, 0, cx), at(&view, 3, cx));
    drag(cx, from, to);
    assert_eq!(mode(&view, cx), Some("visual"));
    assert_eq!(vim_cursor(&view, cx), Some(3));
    assert_eq!(selection(&view, cx), 0..4);

    // A click leaves visual mode, the caret at the click.
    let p = at(&view, 9, cx);
    click(cx, p, 1);
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(selection(&view, cx), 9..9);
    // gv brings back the dragged extent.
    cx.simulate_keystrokes("g v");
    assert_eq!(mode(&view, cx), Some("visual"));
    assert_eq!(selection(&view, cx), 0..4);
    cx.simulate_keystrokes("escape");

    // A double click selects a word, charwise.
    let p = at(&view, 5, cx);
    click(cx, p, 2);
    assert_eq!(mode(&view, cx), Some("visual"));
    assert_eq!(selection(&view, cx), 4..7);
    assert_eq!(vim_cursor(&view, cx), Some(6));

    // A triple click: linewise.
    let p = at(&view, 9, cx);
    click(cx, p, 1);
    click(cx, p, 2);
    click(cx, p, 3);
    assert_eq!(mode(&view, cx), Some("visual_line"));
    assert_eq!(selection(&view, cx), 8..13);

    // A click in it leaves it too.
    let p = at(&view, 1, cx);
    click(cx, p, 1);
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(selection(&view, cx), 1..1);

    // Vim off: a plain selection.
    set_vim(false, cx);
    let (from, to) = (at(&view, 0, cx), at(&view, 3, cx));
    drag(cx, from, to);
    assert_eq!(mode(&view, cx), None);
    assert_eq!(selection(&view, cx), 0..3);
}

#[gpui::test]
fn mouse_drags_with_vim_off_from_the_start(cx: &mut TestAppContext) {
    let (view, cx) = editor_vim_off(cx, "one two", 0);
    let (from, to) = (at(&view, 0, cx), at(&view, 3, cx));
    drag(cx, from, to);
    assert_eq!(mode(&view, cx), None);
    assert_eq!(selection(&view, cx), 0..3);
}

#[gpui::test]
fn visual_layer_of_the_default_keymap(_: &mut TestAppContext) {
    let store = KeymapStore::load(DEFAULT_MACOS, None, None);
    assert_eq!(store.diagnostics(), []);
    let bindings = store.keymap().bindings();
    let visual: Vec<usize> = (0..bindings.len())
        .filter(|&ix| {
            bindings[ix]
                .context
                .as_ref()
                .map(ToString::to_string)
                .as_deref()
                == Some("editor && vim.visual")
        })
        .collect();
    let layer: Vec<_> = visual
        .iter()
        .map(|&ix| {
            let binding = &bindings[ix];
            (
                binding.keys.to_string(),
                binding.command.as_str(),
                binding.args.clone(),
            )
        })
        .collect();
    // T199's arrows, then the editing keys T161 binds in normal mode.
    let keys = |keys: &str| fe_keymap::KeySequence::parse(keys).unwrap().to_string();
    let mut expected: Vec<_> = ["left", "right", "up", "down"]
        .map(|motion| {
            (
                keys(motion),
                "vim.move",
                serde_json::json!({ "motion": motion }),
            )
        })
        .into();
    for key in [
        "enter",
        "tab",
        "backspace",
        "shift-backspace",
        "delete",
        "alt-backspace",
    ] {
        expected.push((keys(key), KEY, fe_command::Value::Null));
    }
    assert_eq!(layer, expected);

    // Each resolves against fe-gpui's catalog.
    let checked = KeymapStore::load(DEFAULT_MACOS, None, Some(command_catalog()));
    for &ix in &visual {
        let prefix = format!("bindings[{ix}]");
        let found = checked.diagnostics().iter().find(|diagnostic| {
            let key = diagnostic.key.as_deref().unwrap_or_default();
            key == prefix || key.starts_with(&format!("{prefix}."))
        });
        assert!(found.is_none(), "{found:?}");
    }
}

#[gpui::test]
fn shift_clicks_and_word_drags(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two three", 0);

    // A shift-click extends charwise visual mode from the caret.
    let p = at(&view, 5, cx);
    cx.simulate_click(p, Modifiers::shift());
    assert_eq!(mode(&view, cx), Some("visual"));
    assert_eq!(selection(&view, cx), 0..6);
    let p = at(&view, 2, cx);
    cx.simulate_click(p, Modifiers::shift());
    assert_eq!(selection(&view, cx), 0..3);
    cx.simulate_keystrokes("escape");

    // A double click's drag grows whole words, either way.
    let (from, to) = (at(&view, 5, cx), at(&view, 9, cx));
    click(cx, from, 1);
    cx.simulate_event(MouseDownEvent {
        position: from,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    assert_eq!(selection(&view, cx), 4..13);
    assert_eq!(vim_cursor(&view, cx), Some(12));
    let to = at(&view, 1, cx);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    assert_eq!(selection(&view, cx), 0..7);
    assert_eq!(vim_cursor(&view, cx), Some(0));
    assert_eq!(mode(&view, cx), Some("visual"));
}

#[gpui::test]
fn jumps_and_bound_clicks_leave_visual_mode(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two three", 0);
    cx.simulate_keystrokes("v e");
    view.update(cx, |view, cx| view.select_and_reveal(8..13, cx));
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(selection(&view, cx), 8..13);
    // gv: the extent the jump left.
    cx.simulate_keystrokes("g v");
    assert_eq!(selection(&view, cx), 0..3);

    // A bound click, here cmd-click running a motion.
    let (default, _) = fe_keymap::KeymapLayer::parse(
        std::path::Path::new(fe_keymap::DEFAULT_KEYMAP_PATH),
        DEFAULT_MACOS,
        None,
    );
    let source = "[[bindings]]\nkeys = \"cmd-click\"\n\
        command = \"editor.move_right\"\ncontext = \"editor\"\n";
    let (layer, _) = fe_keymap::KeymapLayer::parse(std::path::Path::new("k.toml"), source, None);
    let keymap = fe_keymap::Keymap::from_layers(vec![default.unwrap(), layer.unwrap()]);
    cx.update(|_, cx| crate::keymap::set_user_keymap(cx, keymap));
    let p = at(&view, 9, cx);
    cx.simulate_click(p, Modifiers::command());
    cx.run_until_parked();
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(selection(&view, cx), 10..10);
}

#[gpui::test]
fn another_views_edits_keep_the_selection_the_visual_extent(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two", 0);
    cx.simulate_keystrokes("v e");
    assert_eq!(selection(&view, cx), 0..3);
    // Another view of the buffer deletes "one ": the model's selection
    // collapses, while vim's anchors follow to "t".
    view.update(cx, |view, _| {
        let mut other = view.model().split();
        other.select_range(0..4, false);
        other.insert("");
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(selection(&view, cx), 0..1);
    cx.simulate_keystrokes("cmd-c");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("t"));
    cx.simulate_keystrokes("d");
    assert_eq!(text(&view, cx), "wo");
}

#[gpui::test]
fn clicks_and_jumps_drop_a_pending_command(cx: &mut TestAppContext) {
    let original = "one two\nthree\nfour";
    let (view, cx) = editor(cx, original, 0);

    // An operator waiting for its motion: the click drops it.
    cx.simulate_keystrokes("d");
    let p = at(&view, 4, cx);
    click(cx, p, 1);
    assert_eq!(mode(&view, cx), Some("normal"));
    cx.simulate_keystrokes("w");
    assert_eq!(text(&view, cx), original);
    assert_eq!(selection(&view, cx), 8..8);

    // A count: `j` after the click moves one row.
    cx.simulate_keystrokes("g g 0 3");
    let p = at(&view, 1, cx);
    click(cx, p, 1);
    cx.simulate_keystrokes("j");
    assert_eq!(selection(&view, cx), 9..9);

    // `r` waiting in a visual mode: `x` after the click deletes.
    cx.simulate_keystrokes("v r");
    let p = at(&view, 0, cx);
    click(cx, p, 1);
    assert_eq!(mode(&view, cx), Some("normal"));
    cx.simulate_keystrokes("x");
    assert_eq!(text(&view, cx), "ne two\nthree\nfour");

    // And a jump.
    cx.simulate_keystrokes("d");
    view.update(cx, |view, cx| view.select_and_reveal(3..3, cx));
    assert_eq!(mode(&view, cx), Some("normal"));
    cx.simulate_keystrokes("w");
    assert_eq!(text(&view, cx), "ne two\nthree\nfour");
}

#[gpui::test]
fn a_word_drag_onto_an_empty_row_ends_on_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one\n\ntwo", 0);
    let (from, to) = (at(&view, 1, cx), at(&view, 4, cx));
    click(cx, from, 1);
    cx.simulate_event(MouseDownEvent {
        position: from,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    assert_eq!(mode(&view, cx), Some("visual"));
    assert_eq!(vim_cursor(&view, cx), Some(4));
    assert_eq!(selection(&view, cx), 0..5);
}
