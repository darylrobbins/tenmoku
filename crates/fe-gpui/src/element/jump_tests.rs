//! A pending vim jump in the editor: its labels painted over the targets on
//! a dimmed backdrop, readable, its query in the status bar, and the letters
//! shown after `{c1}` being the labels that, typed after `{c2}`, jump.

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

/// After `g s {c1}`, the targets of `{c1}{c2}` shown: the direct ones
/// and those with a letter, with it.
fn preview(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    c2: char,
) -> (Vec<usize>, Vec<(usize, String)>) {
    let text = view.read_with(cx, |view, _| view.model().text().to_string());
    let group = |offset: usize| text[offset + 1..].starts_with(c2);
    let shown: Vec<(usize, String, JumpLabelState)> = (laid_out(view, cx).into_iter())
        .filter(|&(offset, ..)| group(offset))
        .collect();
    let direct = (shown.iter())
        .filter(|(.., state)| *state == JumpLabelState::Direct)
        .map(|&(offset, ..)| offset)
        .collect();
    let letters: Vec<(usize, String)> = (shown.iter())
        .filter(|(.., state)| *state == JumpLabelState::Preview)
        .map(|(offset, text, _)| (*offset, text.clone()))
        .collect();
    assert!(letters.len() > 1, "{shown:?}");
    (direct, letters)
}

/// From the caret at the text's start, `g s {c1}` then `{c2}`: the letter
/// each target of `{c1}{c2}` shows after `{c1}` is its label after `{c2}`,
/// and the target with none is where it autojumped.
fn preview_labels_match_live(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    c1: char,
    c2: char,
) {
    assert_eq!(head(view, cx), 0);
    cx.simulate_keystrokes(&format!("g s {c1}"));
    draw(view, cx);
    let (direct, letters) = preview(view, cx, c2);
    cx.simulate_keystrokes(&c2.to_string());
    draw(view, cx);
    let labels: Vec<(usize, String)> = (laid_out(view, cx).into_iter())
        .filter(|(.., state)| *state == JumpLabelState::Live)
        .map(|(offset, text, _)| (offset, text))
        .collect();
    assert_eq!(letters, labels, "{c1}{c2}: after {{c1}}, then after {{c2}}");
    match direct[..] {
        [] => assert_eq!(head(view, cx), 0, "{c1}{c2} waits"),
        [first] => assert_eq!(head(view, cx), first, "{c1}{c2} autojumps"),
        _ => panic!("{c1}{c2}: {direct:?} direct"),
    }
    cx.simulate_keystrokes("escape g g");
    draw(view, cx);
}

/// From the caret at the text's start, `g s {c1} {c2}` then the letter a
/// target of `{c1}{c2}` showed after `{c1}`, for the first and last such
/// target: the caret lands on it.
fn type_ahead_lands(view: &Entity<EditorView>, cx: &mut VisualTestContext, c1: char, c2: char) {
    for last in [false, true] {
        assert_eq!(head(view, cx), 0);
        cx.simulate_keystrokes(&format!("g s {c1}"));
        draw(view, cx);
        let (_, letters) = preview(view, cx, c2);
        let (offset, letter) = match last {
            true => letters.last(),
            false => letters.first(),
        }
        .cloned()
        .unwrap();
        cx.simulate_keystrokes(&format!("{c2} {letter}"));
        assert_eq!(head(view, cx), offset, "{c1}{c2} then {letter}");
        cx.simulate_keystrokes("g g");
        draw(view, cx);
    }
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
fn preview_labels_match_live_with_partial_row(cx: &mut TestAppContext) {
    let (view, cx) = partial_row(cx);
    preview_labels_match_live(&view, cx, 'a', 'b');
    preview_labels_match_live(&view, cx, 'a', 'c');
}

#[gpui::test]
fn preview_labels_match_live_with_a_fold(cx: &mut TestAppContext) {
    let (view, cx) = folded(cx);
    preview_labels_match_live(&view, cx, 'a', 'b');
    preview_labels_match_live(&view, cx, 'a', 'c');
}

#[gpui::test]
fn type_ahead_from_preview_lands_on_target(cx: &mut TestAppContext) {
    // `ab` autojumps, `ac` waits.
    let (view, cx) = partial_row(cx);
    type_ahead_lands(&view, cx, 'a', 'b');
    type_ahead_lands(&view, cx, 'a', 'c');
}

#[gpui::test]
fn type_ahead_from_preview_lands_on_target_with_a_fold(cx: &mut TestAppContext) {
    let (view, cx) = folded(cx);
    type_ahead_lands(&view, cx, 'a', 'b');
    type_ahead_lands(&view, cx, 'a', 'c');
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

#[gpui::test]
fn direct_target_shows_its_char(cx: &mut TestAppContext) {
    let (view, cx) = editor_of(cx, "ab ab ab ac\nab".to_string(), 10.);
    for (name, palette) in palettes() {
        view.update(cx, |view, cx| view.set_colors(&palette, cx));
        cx.simulate_keystrokes("g s a");
        draw(&view, cx);
        // `ab` autojumps to (0, 3), `ac` to (0, 9).
        let direct: Vec<usize> = (laid_out(&view, cx).into_iter())
            .filter(|(.., state)| *state == JumpLabelState::Direct)
            .map(|(offset, ..)| offset)
            .collect();
        assert_eq!(direct, [3, 9], "{name}");
        let cells: Vec<Bounds<Pixels>> = view.update(cx, |view, _| {
            let layout = view.layout.as_mut().unwrap();
            (direct.iter())
                .map(|&offset| {
                    let at = layout.caret_bounds(offset).unwrap();
                    let next = layout.caret_bounds(offset + 1).unwrap();
                    Bounds {
                        origin: at.origin,
                        size: size(next.origin.x - at.origin.x, at.size.height),
                    }
                })
                .collect()
        });
        let paints = paints();
        let rings: Vec<(Bounds<Pixels>, Pixels)> = (paints.iter())
            .filter_map(|paint| match *paint {
                JumpPaint::Direct { bounds, width, .. } => Some((bounds, width)),
                _ => None,
            })
            .collect();
        assert_eq!(rings.len(), cells.len(), "{name}: {paints:?}");
        for (cell, (ring, width)) in cells.iter().zip(rings) {
            // The border is outside the char's block.
            let inside = Bounds {
                origin: point(ring.origin.x + width, ring.origin.y + width),
                size: size(ring.size.width - width * 2., ring.size.height - width * 2.),
            };
            let within = |a: Pixels, b: Pixels| a >= b - px(0.01);
            assert!(
                within(cell.origin.x, inside.origin.x)
                    && within(cell.origin.y, inside.origin.y)
                    && within(inside.right(), cell.right())
                    && within(inside.bottom(), cell.bottom()),
                "{name}: the ring {ring:?} covers {cell:?}"
            );
            for paint in &paints {
                if let JumpPaint::Label { bounds, .. } = paint {
                    let cell = Bounds {
                        origin: point(cell.origin.x + px(0.01), cell.origin.y + px(0.01)),
                        size: size(cell.size.width - px(0.02), cell.size.height - px(0.02)),
                    };
                    assert!(
                        !bounds.intersects(&cell),
                        "{name}: a label {bounds:?} covers {cell:?}"
                    );
                }
            }
        }
        cx.simulate_keystrokes("escape");
    }
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
        for (keys, state) in [
            ("g s a", JumpLabelState::Preview),
            ("b", JumpLabelState::Live),
        ] {
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
