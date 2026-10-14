//! How a pending vim jump paints: no backdrop until `{c1}`, then a light
//! one, labels in the theme's jump colours, and the block caret above the
//! backdrop.

use fe_buffer::Buffer;
use fe_config::{Color, EditorPalette};
use fe_editor::EditorModel;
use gpui::{Focusable as _, FontWeight, Hsla, Rgba, TestAppContext, VisualTestContext, rgb};

use super::*;
use crate::{EditorSettings, init, vim::set_vim_enabled};

/// `rows` rows of `row`, drawn in an editor `width` wide and `visible` rows
/// tall, with vim on.
pub(super) fn editor<'a>(
    cx: &'a mut TestAppContext,
    row: &str,
    rows: usize,
    width: f32,
    visible: f32,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = vec![row; rows].join("\n");
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
    cx.simulate_resize(size(px(width), px(line_height * visible)));
    draw(&view, cx);
    (view, cx)
}

/// jump_tests.rs's editor: 40 rows of six `ab`s, 10 rows tall.
fn abs(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor(cx, "ab ab ab ab ab ab", 40, 600., 10.)
}

/// Draws a frame; its line height.
pub(super) fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    })
}

/// What the last frame painted for the jump, in order.
pub(super) fn paints() -> Vec<JumpPaint> {
    JUMP_PAINTS.with(|paints| paints.borrow().clone())
}

fn backdrops(paints: &[JumpPaint]) -> Vec<(Bounds<Pixels>, Hsla)> {
    (paints.iter())
        .filter_map(|paint| match *paint {
            JumpPaint::Backdrop { bounds, color } => Some((bounds, color)),
            _ => None,
        })
        .collect()
}

/// The labels the last frame laid out: bounds and state.
pub(super) fn laid_out(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(Bounds<Pixels>, JumpLabelState)> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let labels: Vec<_> = (layout.jump.as_ref().unwrap().labels.iter())
            .map(|label| (label.rect, label.state))
            .collect();
        (labels.into_iter())
            .map(|(rect, state)| (layout.bounds(rect), state))
            .collect()
    })
}

/// A label paint: bounds, fill and glyph.
type LabelPaint = (Bounds<Pixels>, Hsla, Option<(Hsla, FontWeight)>);

/// Each label paint.
pub(super) fn label_paints(paints: &[JumpPaint]) -> Vec<LabelPaint> {
    (paints.iter())
        .filter_map(|paint| match *paint {
            JumpPaint::Label {
                bounds,
                fill,
                glyph,
            } => Some((bounds, fill, glyph)),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn no_backdrop_before_first_char(cx: &mut TestAppContext) {
    assert_eq!(JUMP_BACKDROP_ALPHA, 0.3);
    let (view, cx) = abs(cx);
    cx.simulate_keystrokes("g s");
    draw(&view, cx);
    view.read_with(cx, |view, _| {
        let jump = view.layout.as_ref().unwrap().jump.as_ref();
        let jump = jump.expect("a jump is pending");
        assert_eq!(jump.backdrop, None);
    });
    assert_eq!(backdrops(&paints()), []);

    cx.simulate_keystrokes("a");
    draw(&view, cx);
    let (window, background) = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let text = layout.frame.text_bounds();
        assert_eq!(layout.jump.as_ref().unwrap().backdrop, Some(text));
        (layout.bounds(text), view.theme.background)
    });
    assert_eq!(
        backdrops(&paints()),
        [(window, background.opacity(JUMP_BACKDROP_ALPHA))]
    );
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
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The colour at `key` in the `[editor]` table of a theme file.
fn editor_color(source: &str, key: &str) -> Color {
    let table = source.split("[editor]").nth(1).unwrap();
    let line = (table.lines())
        .find(|line| line.split('=').next().map(str::trim) == Some(key))
        .unwrap_or_else(|| panic!("no {key}"));
    let value = line.split('=').nth(1).unwrap().trim().trim_matches('"');
    value.parse().unwrap()
}

#[gpui::test]
fn backdrop_keeps_text_readable(cx: &mut TestAppContext) {
    let source = include_str!("../../../fe-config/themes/fastedit-ember.toml");
    let mut ember = EditorPalette::fastedit_dark().clone();
    ember.editor.background = editor_color(source, "background");
    ember.editor.foreground = editor_color(source, "foreground");
    let palettes = [
        ("fastedit Dark", EditorPalette::fastedit_dark().clone(), 6.0),
        (
            "fastedit Light",
            EditorPalette::fastedit_light().clone(),
            4.59,
        ),
        ("fastedit Ember", ember, 6.6),
    ];
    for (name, palette, expected) in palettes {
        let theme =
            cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
        let (foreground, background): (Rgba, Rgba) =
            (theme.foreground.into(), theme.background.into());
        let alpha = JUMP_BACKDROP_ALPHA;
        let dim = |text: f32, backdrop: f32| text * (1. - alpha) + backdrop * alpha;
        let dimmed = Rgba {
            r: dim(foreground.r, background.r),
            g: dim(foreground.g, background.g),
            b: dim(foreground.b, background.b),
            a: 1.,
        };
        let ratio = contrast(dimmed, background);
        assert!(ratio >= 4.5, "{name}: {ratio:.2}");
        assert!((ratio - expected).abs() < 0.05, "{name}: {ratio:.2}");
    }
}

#[gpui::test]
fn labels_use_theme_slots(cx: &mut TestAppContext) {
    let (view, cx) = abs(cx);
    let mut palette = EditorPalette::fastedit_dark().clone();
    palette.editor.jump_label = Color::rgb(0x123456);
    palette.editor.jump_label_preview = Color::rgb(0x234567);
    palette.editor.jump_label_foreground = Color::rgb(0x345678);
    view.update(cx, |view, cx| view.set_colors(&palette, cx));
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let (live, preview, glyph) = (
        theme.jump_label,
        theme.jump_label_preview,
        theme.jump_label_foreground,
    );
    assert_eq!(live, Hsla::from(rgb(0x123456)));
    assert_eq!(preview, Hsla::from(rgb(0x234567)));
    assert_eq!(glyph, Hsla::from(rgb(0x345678)));
    let bold = Some((glyph, FontWeight::BOLD));

    // Nothing is painted after `{c1}`.
    cx.simulate_keystrokes("g s a");
    draw(&view, cx);
    assert_eq!(laid_out(&view, cx), []);
    assert!(label_paints(&paints()).is_empty());

    cx.simulate_keystrokes("b");
    draw(&view, cx);
    let labels = laid_out(&view, cx);
    let after_c2 = label_paints(&paints());
    assert_eq!(after_c2.len(), labels.len());
    let count = |state| labels.iter().filter(|(_, s)| *s == state).count();
    assert!(count(JumpLabelState::Live) > 0 && count(JumpLabelState::Concealed) > 0);
    for ((bounds, state), paint) in labels.iter().zip(&after_c2) {
        let expected = match *state == JumpLabelState::Live {
            true => (*bounds, live, bold),
            false => (*bounds, preview, None),
        };
        assert_eq!(*paint, expected);
    }

    for (_, fill, glyph) in &after_c2 {
        assert_eq!(fill.a, 1., "{fill:?} isn't opaque");
        let mut colors = vec![*fill];
        colors.extend(glyph.map(|(color, _)| color));
        for color in colors {
            assert_ne!(color, theme.cursor);
            assert_ne!(color, theme.background);
        }
    }
}

#[gpui::test]
fn caret_above_backdrop(cx: &mut TestAppContext) {
    let (view, cx) = abs(cx);
    let caret_after_backdrop = |paints: &[JumpPaint]| {
        let backdrop = paints
            .iter()
            .position(|paint| matches!(paint, JumpPaint::Backdrop { .. }));
        backdrop.is_some_and(|backdrop| {
            paints[backdrop..]
                .iter()
                .any(|paint| matches!(paint, JumpPaint::BlockCaret { .. }))
        })
    };

    cx.simulate_keystrokes("g s a");
    draw(&view, cx);
    let caret = view.update(cx, |view, _| {
        let head = view.model().primary_selection().head();
        view.layout.as_mut().unwrap().caret_bounds(head).unwrap()
    });
    let painted = paints();
    assert!(caret_after_backdrop(&painted), "{painted:?}");
    let carets: Vec<_> = (painted.iter())
        .filter_map(|paint| match *paint {
            JumpPaint::BlockCaret { bounds } => Some(bounds),
            _ => None,
        })
        .collect();
    assert_eq!(carets.len(), 1);
    assert_eq!(carets[0].origin, caret.origin);

    cx.simulate_keystrokes("escape");
    draw(&view, cx);
    let painted = paints();
    assert!(!caret_after_backdrop(&painted), "{painted:?}");
}

/// The offsets of the live labels vim has in the frame's rows, and those
/// laid out, and how many labels are laid out.
fn live_labels(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Vec<usize>, Vec<usize>, usize) {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let rows =
            frame.rows.first().unwrap().buffer_row..frame.rows.last().unwrap().buffer_row + 1;
        let overlay = view.vim.as_ref().unwrap().jump_overlay(&view.model, rows);
        let live = |state| state == JumpLabelState::Live;
        let expected = (overlay.unwrap().labels.into_iter())
            .filter(|label| live(label.state))
            .map(|label| label.offset)
            .collect();
        let labels = &layout.jump.as_ref().unwrap().labels;
        let laid_out = (labels.iter())
            .filter(|label| live(label.state))
            .map(|label| label.offset)
            .collect();
        (expected, laid_out, labels.len())
    })
}

#[gpui::test]
fn live_labels_survive_the_cap(cx: &mut TestAppContext) {
    // 30 targets a row, 50 rows in view: well over the cap.
    let (view, cx) = editor(cx, &"ab ".repeat(30), 60, 1600., 50.);

    cx.simulate_keystrokes("G g S a b");
    draw(&view, cx);
    let (expected, laid_out, count) = live_labels(&view, cx);
    assert_eq!(count, MAX_MATCHES);
    assert!(!expected.is_empty());
    assert_eq!(laid_out, expected);

    cx.simulate_keystrokes("escape g g g s a b space");
    draw(&view, cx);
    let (expected, laid_out, count) = live_labels(&view, cx);
    assert_eq!(count, MAX_MATCHES);
    assert!(!expected.is_empty());
    assert_eq!(laid_out, expected);
}

#[gpui::test]
fn slots_from_palette(cx: &mut TestAppContext) {
    let theme = cx.update(|cx| {
        EditorTheme::from_palette(
            EditorPalette::fastedit_light(),
            &EditorSettings::default(),
            cx,
        )
    });
    assert_eq!(theme.jump_label, Hsla::from(rgb(0xa8005a)));
    assert_eq!(theme.jump_label_preview, Hsla::from(rgb(0x00727f)));
    assert_eq!(theme.jump_label_foreground, Hsla::from(rgb(0xffffff)));
}
