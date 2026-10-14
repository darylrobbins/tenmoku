//! A pending vim jump in the editor: its labels painted over the targets on
//! a dimmed backdrop, readable, its query in the status bar, and none shown
//! after `{c1}`.

use fe_buffer::Buffer;
use fe_config::EditorPalette;
use fe_editor::EditorModel;
use gpui::{Focusable as _, Rgba, TestAppContext, VisualTestContext};

use super::*;
use crate::{init, pane_item::status, vim::set_vim_enabled};

/// 40 rows of six `ab`s in an editor 10 rows tall, drawn, with vim on.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let text = vec!["ab ab ab ab ab ab"; 40].join("\n");
    editor_of(cx, text, 10.)
}

/// `text` in an editor `visible` rows tall, drawn, with vim on.
fn editor_of(
    cx: &mut TestAppContext,
    text: String,
    visible: f32,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
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
    let line_height = draw(&view, cx);
    cx.simulate_resize(size(px(600.), px(line_height * visible)));
    draw(&view, cx);
    (view, cx)
}

/// Draws a frame; its line height.
fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    })
}

/// The vim mode item's text.
fn vim_status(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        let items = crate::pane_item::status_items(view);
        let item = items.iter().find(|item| item.id == status::VIM_MODE);
        item.unwrap().text.to_string()
    })
}

#[gpui::test]
fn labels_at_targets(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    // 59 matches in view after the caret, more than the safe labels take.
    cx.simulate_keystrokes("g s a b");
    draw(&view, cx);
    view.update(cx, |view, _| {
        assert_eq!(view.model().primary_selection().head(), 0);
        let layout = view.layout.as_mut().unwrap();
        let jump = layout.jump.as_ref().expect("a jump is pending");
        let window = layout.bounds(layout.frame.text_bounds());
        assert_eq!(
            jump.backdrop.map(|backdrop| layout.bounds(backdrop)),
            Some(window)
        );
        let offsets: Vec<usize> = jump.labels.iter().map(|label| label.offset).collect();
        let row = "ab ab ab ab ab ab\n".len();
        let targets: Vec<usize> = (0..10)
            .flat_map(|r| (0..6).map(move |m| r * row + m * 3))
            .skip(1)
            .collect();
        assert_eq!(offsets, targets);
        let states = |state| jump.labels.iter().filter(|l| l.state == state).count();
        let (live, concealed) = (
            states(JumpLabelState::Live),
            states(JumpLabelState::Concealed),
        );
        assert!(
            live > 0 && concealed > 0,
            "{live} live, {concealed} concealed"
        );
        let labels: Vec<(usize, Bounds<Pixels>)> = (jump.labels.iter())
            .map(|label| (label.offset, layout.bounds(label.rect)))
            .collect();
        for (offset, label) in labels {
            let caret = layout.caret_bounds(offset).unwrap();
            assert_eq!(
                (label.origin, label.size.height),
                (caret.origin, caret.size.height)
            );
            assert!(label.size.width > px(0.));
            let inside = window.contains(&label.origin)
                && label.bottom_right().y <= window.bottom_right().y
                && label.bottom_right().x <= window.bottom_right().x;
            assert!(inside, "{label:?} outside {window:?}");
        }
    });

    cx.simulate_keystrokes("escape");
    draw(&view, cx);
    view.read_with(cx, |view, _| {
        assert!(view.layout.as_ref().unwrap().jump.is_none());
    });
}

#[gpui::test]
fn status_shows_query(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    assert_eq!(vim_status(&view, cx), "NORMAL");
    cx.simulate_keystrokes("g s");
    assert_eq!(vim_status(&view, cx), "JUMP");
    cx.simulate_keystrokes("a");
    assert_eq!(vim_status(&view, cx), "JUMP a");
    cx.simulate_keystrokes("b");
    assert_eq!(vim_status(&view, cx), "JUMP ab");
    // Picking a label ends it.
    draw(&view, cx);
    let label = view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref().unwrap();
        let live = jump.labels.iter().find(|l| l.state == JumpLabelState::Live);
        live.unwrap().text.to_string()
    });
    let target = view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref().unwrap();
        let live = jump
            .labels
            .iter()
            .filter(|l| l.state == JumpLabelState::Live);
        live.clone().find(|l| l.text == label).unwrap().offset
    });
    cx.simulate_keystrokes(&label);
    assert_eq!(vim_status(&view, cx), "NORMAL");
    assert_eq!(head(&view, cx), target);
}

/// The caret.
fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

/// The labels the last frame laid out: offset, text and state.
fn laid_out(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(usize, String, JumpLabelState)> {
    view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref();
        (jump.expect("a jump is pending").labels.iter())
            .map(|label| (label.offset, label.text.to_string(), label.state))
            .collect()
    })
}

/// From the caret at the text's start, `g s {c1}`: no label shown. Then
/// `{c2}`: the target it autojumps to, or the caret stays at 0 and Live
/// labels wait, the last of which, typed, lands the caret on its target.
fn labels_only_after_c2(view: &Entity<EditorView>, cx: &mut VisualTestContext, c1: char, c2: char) {
    assert_eq!(head(view, cx), 0);
    cx.simulate_keystrokes(&format!("g s {c1}"));
    draw(view, cx);
    assert_eq!(laid_out(view, cx), []);
    cx.simulate_keystrokes(&c2.to_string());
    draw(view, cx);
    let live: Vec<(usize, String)> = (laid_out(view, cx).into_iter())
        .filter(|(.., state)| *state == JumpLabelState::Live)
        .map(|(offset, text, _)| (offset, text))
        .collect();
    assert!(live.len() > 1, "{c1}{c2}: {live:?}");
    let (offset, letter) = live.last().unwrap().clone();
    cx.simulate_keystrokes(&letter);
    assert_eq!(head(view, cx), offset, "{c1}{c2} then {letter}");
    cx.simulate_keystrokes("g g");
    draw(view, cx);
}

/// Rows of two `ab`s and three `ac`s after a row with neither: with nine
/// such rows in view, `ab` autojumps (18 targets fit the safe labels) and
/// `ac` waits.
fn groups() -> String {
    let rows = std::iter::once("x").chain(std::iter::repeat_n("ab ab ac ac ac", 40));
    rows.collect::<Vec<_>>().join("\n")
}

/// [`groups`] in an editor ten and a half rows tall.
fn partial_row(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor_of(cx, groups(), 10.5)
}

/// [`groups`] in an editor ten rows tall, rows 4 to 12 folded behind 3.
fn folded(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = editor_of(cx, groups(), 10.);
    view.update(cx, |view, _| view.model.fold_rows(3..=12));
    draw(&view, cx);
    (view, cx)
}

#[gpui::test]
fn labels_only_after_c2_with_partial_row(cx: &mut TestAppContext) {
    // `ab` autojumps, `ac` waits.
    let (view, cx) = partial_row(cx);
    labels_only_after_c2(&view, cx, 'a', 'b');
    labels_only_after_c2(&view, cx, 'a', 'c');
}

#[gpui::test]
fn labels_only_after_c2_with_a_fold(cx: &mut TestAppContext) {
    let (view, cx) = folded(cx);
    labels_only_after_c2(&view, cx, 'a', 'b');
    labels_only_after_c2(&view, cx, 'a', 'c');
}

#[gpui::test]
fn no_labels_after_first_char(cx: &mut TestAppContext) {
    // Many `p`s, each followed by a different char, `u` among them often.
    let text = ('a'..='z').map(|c| format!("p{c} pu ")).collect::<String>();
    let (view, cx) = editor_of(cx, text, 10.);
    cx.simulate_keystrokes("g s p");
    draw(&view, cx);
    assert_eq!(vim_status(&view, cx), "JUMP p");
    assert_eq!(laid_out(&view, cx), []);
    let marks = paints()
        .into_iter()
        .filter(|paint| matches!(paint, JumpPaint::Label { .. }));
    assert_eq!(marks.count(), 0);
    // `u` autojumps to the nearest `pu` or waits; a shown Live label then
    // moves the caret to its target.
    cx.simulate_keystrokes("u");
    draw(&view, cx);
    let live: Vec<(usize, String)> = (laid_out(&view, cx).into_iter())
        .filter(|(.., state)| *state == JumpLabelState::Live)
        .map(|(offset, text, _)| (offset, text))
        .collect();
    let (offset, letter) = live.last().expect("Live labels after pu").clone();
    cx.simulate_keystrokes(&letter);
    assert_eq!(head(&view, cx), offset);
}

#[gpui::test]
fn label_picked_while_labels_wait(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.simulate_keystrokes("g s a b");
    draw(&view, cx);
    let labels = laid_out(&view, cx);
    let mut live = (labels.iter()).filter(|(.., state)| *state == JumpLabelState::Live);
    let (offset, letter, _) = live.next_back().unwrap().clone();
    assert_ne!(offset, 0);
    cx.simulate_keystrokes(&letter);
    assert_eq!(head(&view, cx), offset);
}

/// The default dark and light palettes.
fn palettes() -> [(&'static str, EditorPalette); 2] {
    [
        ("fastedit Dark", EditorPalette::fastedit_dark().clone()),
        ("fastedit Light", EditorPalette::fastedit_light().clone()),
    ]
}

/// The last frame's jump paints.
fn paints() -> Vec<JumpPaint> {
    JUMP_PAINTS.with(|paints| paints.borrow().clone())
}

/// WCAG relative luminance.
fn luminance(color: Rgba) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// WCAG contrast ratio of `a` against `b`.
fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a.into()), luminance(b.into()));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[gpui::test]
fn label_glyphs_readable(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    for (name, palette) in palettes() {
        view.update(cx, |view, cx| view.set_colors(&palette, cx));
        for (keys, state) in [("g s a b", JumpLabelState::Live)] {
            cx.simulate_keystrokes(keys);
            draw(&view, cx);
            let shown = (laid_out(&view, cx).into_iter())
                .filter(|(.., s)| *s == state)
                .count();
            let glyphs: Vec<(Hsla, Hsla)> = (paints().into_iter())
                .filter_map(|paint| match paint {
                    JumpPaint::Label {
                        fill,
                        glyph: Some((glyph, _)),
                        ..
                    } => Some((fill, glyph)),
                    _ => None,
                })
                .collect();
            assert!(shown > 0, "{name}: no {state:?} labels");
            assert_eq!(glyphs.len(), shown, "{name} {state:?}");
            for (fill, glyph) in glyphs {
                let ratio = contrast(glyph, fill);
                assert!(ratio >= 4.5, "{name} {state:?}: {ratio:.2}");
            }
        }
        cx.simulate_keystrokes("escape");
    }
}
