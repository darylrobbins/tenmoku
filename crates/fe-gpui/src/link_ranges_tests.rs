//! Links in a headless window: with a bound modifier click held, the whole
//! link under the pointer is underlined, not the word; the links move with
//! edits.

use std::{ops::Range, path::Path};

use fe_buffer::{Buffer, Origin};
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{
    Entity, Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px,
};

use crate::*;

const TEXT: &str = "// see https://a.example/docs for more\nfn main() {}\n";
const URL: &str = "https://a.example/docs";

/// The test layer: `cmd-click` runs `test.jump` in the editor.
const CMD_CLICK: &str = "\
[[bindings]]
keys = \"cmd-click\"
command = \"test.jump\"
context = \"editor\"
";

/// A focused editor on [`TEXT`] with `cmd-click` bound, [`URL`] set as its
/// one link, in an active window.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), CMD_CLICK, None);
    assert_eq!(diagnostics, []);
    let keymap = Keymap::from_layers(vec![default.unwrap(), layer.unwrap()]);
    cx.update(|cx| init_with_keymap(cx, keymap));
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), Some("main.rs".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    view.update(cx, |view, _| view.set_link_ranges(vec![url()]));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

fn url() -> Range<usize> {
    let start = TEXT.find(URL).unwrap();
    start..start + URL.len()
}

/// Just right of the caret position at `offset`, mid-row, in the last frame.
fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    })
}

/// The hover range, checking the last frame underlined it and showed the
/// pointing hand.
fn hover(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Range<usize>> {
    view.update(cx, |view, _| {
        let range = view.hover_range();
        let (underlines, hand) = &view.painted_hover;
        assert_eq!(*hand, range.is_some());
        assert_eq!(!underlines.is_empty(), range.is_some());
        range
    })
}

fn move_to(position: Point<Pixels>, modifiers: Modifiers, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, modifiers);
    cx.run_until_parked();
}

#[gpui::test]
fn hover_underlines_the_whole_link_with_the_modifier(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let link = url();
    let cmd = Modifiers::command();
    // Over `example`, a word of its own.
    let over = at(&view, link.start + "https://a.ex".len(), cx);
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(link.clone()));
    // From the link's start to its end.
    let (underlines, start, end) = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let start = layout.caret_bounds(link.start).unwrap();
        let end = layout.caret_bounds(link.end).unwrap();
        (view.painted_hover.0.clone(), start, end)
    });
    assert_eq!(underlines.first().unwrap().left(), start.left());
    assert_eq!(underlines.last().unwrap().right(), end.left());

    // Its first and last glyphs are in it too.
    move_to(at(&view, link.start, cx), cmd, cx);
    assert_eq!(hover(&view, cx), Some(link.clone()));
    move_to(at(&view, link.end - 1, cx), cmd, cx);
    assert_eq!(hover(&view, cx), Some(link.clone()));

    // Off the link, the word under the pointer, as before.
    let see = TEXT.find("see").unwrap();
    move_to(at(&view, see + 1, cx), cmd, cx);
    assert_eq!(hover(&view, cx), Some(see..see + "see".len()));
}

#[gpui::test]
fn without_the_modifier_nothing_is_underlined(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let link = url();
    let over = at(&view, link.start + 3, cx);
    move_to(over, Modifiers::none(), cx);
    assert_eq!(hover(&view, cx), None);
    // Shift-click isn't bound.
    move_to(over, Modifiers::shift(), cx);
    assert_eq!(hover(&view, cx), None);
    move_to(over, Modifiers::command(), cx);
    assert_eq!(hover(&view, cx), Some(link));
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "released");
}

#[gpui::test]
fn links_follow_edits(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let link = url();
    let edit = |view: &Entity<EditorView>, at: usize, text: &str, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            let mut buffer = view.model_mut().buffer_mut();
            buffer.edit([(at..at, text)], Origin::User);
            drop(buffer);
            cx.notify();
        });
        cx.run_until_parked();
    };

    // Text before it moves it; text at either edge stays outside it.
    edit(&view, 0, "//", cx);
    let moved = link.start + 2..link.end + 2;
    view.read_with(cx, |view, _| {
        assert_eq!(view.link_ranges(), vec![moved.clone()]);
        assert_eq!(view.link_at(moved.start), Some(moved.clone()));
        assert_eq!(view.link_at(moved.end), None);
    });
    edit(&view, moved.end, "!", cx);
    edit(&view, moved.start, "<", cx);
    let moved = moved.start + 1..moved.end + 1;
    view.read_with(cx, |view, _| {
        assert_eq!(view.link_ranges(), vec![moved.clone()])
    });

    // Hovered where it now is.
    let over = at(&view, moved.start + 3, cx);
    move_to(over, Modifiers::command(), cx);
    assert_eq!(hover(&view, cx), Some(moved.clone()));

    // A new set replaces it; an empty one clears it.
    view.update(cx, |view, _| view.set_link_ranges(vec![0..2, 5..5]));
    let kept: Range<usize> = 0..2;
    view.read_with(cx, |view, _| assert_eq!(view.link_ranges(), vec![kept]));
    view.update(cx, |view, _| view.set_link_ranges(Vec::new()));
    view.read_with(cx, |view, _| {
        assert!(view.link_ranges().is_empty());
        assert_eq!(view.link_at(moved.start + 3), None);
    });
}

#[gpui::test]
fn links_set_under_a_still_pointer_over_a_space_underline(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    // A link with a space in it, the pointer still over the space.
    let see = TEXT.find("see ").unwrap();
    let link = see..url().end;
    view.update(cx, |view, _| view.set_link_ranges(Vec::new()));
    let space = at(&view, see + "see".len(), cx);
    move_to(space, Modifiers::command(), cx);
    assert_eq!(hover(&view, cx), None, "whitespace, no link yet");
    view.update(cx, |view, cx| {
        view.set_link_ranges(vec![link.clone()]);
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), Some(link));
}
