//! Decoration providers' decorations laid out and painted in a headless
//! window: row backgrounds and gutter marks on every display row of the rows
//! they cover, highlights and underlines over their range's text, text after
//! a line, each in its tint.

use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc};

use fe_buffer::Buffer;
use fe_config::{Color, EditorPalette};
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, GutterMark, Tint, overlaps},
};
use fe_vim::search::{FindQuery, Pattern};
use gpui::{Bounds, Entity, Hsla, Pixels, TestAppContext, VisualTestContext, px, rgb};

use super::*;
use crate::{EditorSettings, EditorTheme, EditorView, init};

/// A provider handing out fixed decorations, noting each range it's asked
/// for.
#[derive(Default)]
struct Fixed {
    items: Vec<Decoration>,
    queries: RefCell<Vec<Range<usize>>>,
}

impl DecorationProvider for Fixed {
    fn decorations(
        &self,
        _: &fe_buffer::BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        self.queries.borrow_mut().push(range.clone());
        let items = self
            .items
            .iter()
            .filter(|item| overlaps(&item.range, &range));
        out.extend(items.cloned());
    }
}

/// An editor on `text`, drawn, with a [`Fixed`] provider registered.
fn editor<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    Rc<RefCell<Fixed>>,
    &'a mut VisualTestContext,
) {
    cx.update(init);
    let text = text.to_string();
    let provider = Rc::new(RefCell::new(Fixed::default()));
    let registered = provider.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.add_decoration_provider(registered);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    (view, provider, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Hands out `items` and draws a frame, noting what it painted.
fn show(
    provider: &Rc<RefCell<Fixed>>,
    items: Vec<Decoration>,
    cx: &mut VisualTestContext,
) -> Vec<&'static str> {
    provider.borrow_mut().items = items;
    PAINTED.with(|painted| painted.borrow_mut().clear());
    draw(cx);
    PAINTED.with(|painted| painted.take())
}

/// The last frame's decorations, by kind: their window bounds and colours.
fn placed(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(DecorationKind, Bounds<Pixels>, Hsla)> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        (layout.decorations.placed.iter())
            .map(|placed| (placed.kind.clone(), placed.bounds, placed.color))
            .collect()
    })
}

fn of_kind(
    all: &[(DecorationKind, Bounds<Pixels>, Hsla)],
    kind: &DecorationKind,
) -> Vec<(Bounds<Pixels>, Hsla)> {
    (all.iter())
        .filter(|(placed, ..)| placed == kind)
        .map(|&(_, bounds, color)| (bounds, color))
        .collect()
}

fn theme(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> EditorTheme {
    view.read_with(cx, |view, _| view.theme.clone())
}

/// A frame fact, in window coordinates.
fn with_layout<R>(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut FrameLayout) -> R,
) -> R {
    view.update(cx, |view, _| f(view.layout.as_mut().expect("drawn")))
}

/// The display rows of the last frame: (display row, buffer row, top).
fn rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, u32, Pixels)> {
    with_layout(view, cx, |layout| {
        let frame = &layout.frame;
        (frame.rows.iter())
            .map(|row| {
                let top = layout.origin.y + px(frame.row_top(row.row));
                (row.row, row.buffer_row, top)
            })
            .collect()
    })
}

fn decoration(range: Range<usize>, kind: DecorationKind, tint: Tint) -> Decoration {
    Decoration::new(range, kind, tint)
}

const BAR: DecorationKind = DecorationKind::Gutter(GutterMark::Bar);
const DELETION: DecorationKind = DecorationKind::Gutter(GutterMark::Deletion);

/// Rows of words, the second long enough to wrap in the window.
fn wrapping_text() -> String {
    let long = "word ".repeat(80);
    format!("first\n{long}\nthird\nfourth\nfifth\n")
}

#[gpui::test]
fn row_backgrounds_and_bars_cover_every_display_row(cx: &mut TestAppContext) {
    let text = wrapping_text();
    let (view, provider, cx) = editor(&text, cx);
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    draw(cx);
    let theme = theme(&view, cx);
    let row_start: Vec<usize> = view.read_with(cx, |view, _| {
        let text = view.model().text();
        (0..6).map(|row| text.row_start(row)).collect()
    });
    // Rows 1 and 2: through the start of row 3, which isn't covered.
    let covered = row_start[1]..row_start[3];
    show(
        &provider,
        vec![
            decoration(covered.clone(), DecorationKind::RowBackground, Tint::Added),
            decoration(covered, BAR, Tint::Modified),
            decoration(row_start[4]..row_start[4], DELETION, Tint::Removed),
        ],
        cx,
    );
    let all = placed(&view, cx);
    let shown = rows(&view, cx);
    let covered_rows: Vec<_> = (shown.iter())
        .filter(|(_, buffer_row, _)| (1..=2).contains(buffer_row))
        .collect();
    assert!(
        covered_rows.len() > 2,
        "row 1 wraps: {} display rows",
        covered_rows.len()
    );
    let (text_area, lane) = with_layout(&view, cx, |layout| {
        (
            layout.bounds(layout.frame.text_bounds()),
            layout.bounds(layout.frame.change_lane),
        )
    });
    let gutter_x = lane.origin.x;
    let line_height = view.read_with(cx, |view, _| {
        px(view.layout.as_ref().unwrap().frame.line_height)
    });

    let added = theme.change_added;
    let backgrounds = of_kind(&all, &DecorationKind::RowBackground);
    assert_eq!(backgrounds.len(), covered_rows.len());
    for ((bounds, color), (_, _, top)) in backgrounds.iter().zip(&covered_rows) {
        assert_eq!(bounds.origin.x, text_area.origin.x);
        assert_eq!(bounds.size.width, text_area.size.width);
        assert_eq!((bounds.origin.y, bounds.size.height), (*top, line_height));
        assert_eq!(*color, added.opacity(0.08));
    }

    // One bar down the run of rows, inset 1 px at each end.
    let bars = of_kind(&all, &BAR);
    assert_eq!(bars.len(), 1);
    for (bounds, color) in &bars {
        let top = covered_rows[0].2;
        let height = line_height * covered_rows.len() as f32;
        assert_eq!(bounds.origin.x, gutter_x);
        assert_eq!(bounds.size.width, px(3.));
        assert_eq!(
            (bounds.origin.y, bounds.size.height),
            (top + px(1.), height - px(2.))
        );
        assert_eq!(*color, theme.change_modified.opacity(0.6));
    }

    // On row 4's top edge, centred on it.
    let deletions = of_kind(&all, &DELETION);
    let row_4_top = shown.iter().find(|(_, row, _)| *row == 4).unwrap().2;
    assert_eq!(
        deletions,
        [(
            Bounds::new(
                gpui::point(gutter_x, row_4_top - px(1.)),
                gpui::size(px(6.), px(2.))
            ),
            theme.change_deleted.opacity(1.0)
        )]
    );
    // Clear of the diagnostics' dot at the gutter's left.
    let side = (f32::from(line_height) * 0.35).round();
    let origin_x = with_layout(&view, cx, |layout| layout.origin.x);
    assert!(gutter_x >= origin_x + px(side * 1.5));

    // Folded away, rows give nothing; the fold's header still does.
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(0..=2);
        cx.notify();
    });
    draw(cx);
    let all = placed(&view, cx);
    assert!(of_kind(&all, &DecorationKind::RowBackground).is_empty());
    assert!(of_kind(&all, &BAR).is_empty());
    assert_eq!(of_kind(&all, &DELETION).len(), 1);
}

#[gpui::test]
fn scrolled_off_rows_give_nothing(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    let (view, provider, cx) = editor(&text, cx);
    let row_250 = text.find("row 250").unwrap();
    let row_251 = text.find("row 251").unwrap();
    let painted = show(
        &provider,
        vec![
            decoration(
                row_250..row_251,
                DecorationKind::RowBackground,
                Tint::Accent,
            ),
            decoration(row_250..row_251, BAR, Tint::Accent),
            decoration(row_251..row_251, DELETION, Tint::Accent),
            decoration(
                row_250..row_250 + 3,
                DecorationKind::Highlight,
                Tint::Accent,
            ),
            decoration(
                row_250..row_250 + 3,
                DecorationKind::AfterLine("note".into()),
                Tint::Accent,
            ),
        ],
        cx,
    );
    assert!(placed(&view, cx).is_empty());
    assert_eq!(painted, ["text"]);
    // Only the shown range is asked for.
    let queries = provider.borrow().queries.borrow().clone();
    assert!(queries.iter().all(|range| range.end < row_250));
}

/// The window x of the caret at `offset` in the last frame.
fn x(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Pixels {
    with_layout(view, cx, |layout| {
        layout.caret_bounds(offset).unwrap().left()
    })
}

#[gpui::test]
fn highlights_and_underlines_span_their_range(cx: &mut TestAppContext) {
    let text = "fn main() {\n    let x = y + z;\n}\n";
    let (view, provider, cx) = editor(text, cx);
    let theme = theme(&view, cx);
    let at = |word: &str| {
        let start = text.find(word).unwrap();
        start..start + word.len()
    };
    let wavy = DecorationKind::Underline { wavy: true };
    let straight = DecorationKind::Underline { wavy: false };
    let empty = at("y").start;
    show(
        &provider,
        vec![
            decoration(at("main"), DecorationKind::Highlight, Tint::Accent),
            decoration(empty..empty, DecorationKind::Highlight, Tint::Muted),
            decoration(at("let x"), wavy.clone(), Tint::Warning),
            decoration(at("z;"), straight.clone(), Tint::Hint),
        ],
        cx,
    );
    let all = placed(&view, cx);
    let shown = rows(&view, cx);
    let line_height = shown[1].2 - shown[0].2;
    let space = with_layout(&view, cx, |layout| layout.glyph_widths.char_width(' '));

    let highlights = of_kind(&all, &DecorationKind::Highlight);
    assert_eq!(highlights.len(), 2);
    let main = at("main");
    let (bounds, color) = highlights[0];
    assert_eq!(bounds.left(), x(&view, main.start, cx));
    assert_eq!(bounds.right(), x(&view, main.end, cx));
    assert_eq!(
        (bounds.top(), bounds.size.height),
        (shown[0].2, line_height)
    );
    assert_eq!(color, theme.cursor.opacity(0.22));
    // An empty one is a space wide.
    let (bounds, color) = highlights[1];
    assert_eq!(bounds.left(), x(&view, empty, cx));
    assert_eq!(bounds.size.width, px(space));
    assert_eq!(
        (bounds.top(), bounds.size.height),
        (shown[1].2, line_height)
    );
    assert_eq!(color, theme.foreground.opacity(0.22));

    // Just below the baseline, 1 px, in the tint.
    let baseline_y = with_layout(&view, cx, |layout| {
        let frame = &layout.frame;
        let row = &frame.rows[1];
        let line = &row.line.layout;
        let (ascent, descent): (f32, f32) = (line.ascent.into(), line.descent.into());
        let baseline =
            frame.row_top(row.row) + (frame.line_height - ascent - descent) / 2. + ascent;
        layout.origin.y + px(baseline.round() + 1.)
    });
    for (kind, range, color) in [
        (&wavy, at("let x"), theme.warning),
        (&straight, at("z;"), theme.hint),
    ] {
        let underlines = of_kind(&all, kind);
        assert_eq!(underlines.len(), 1);
        let (bounds, painted) = underlines[0];
        assert_eq!(bounds.left(), x(&view, range.start, cx));
        assert_eq!(bounds.right(), x(&view, range.end, cx));
        assert_eq!((bounds.top(), bounds.size.height), (baseline_y, px(1.)));
        assert_eq!(painted, color);
    }
}

#[gpui::test]
fn highlights_cover_each_wrapped_piece(cx: &mut TestAppContext) {
    let text = wrapping_text();
    let (view, provider, cx) = editor(&text, cx);
    view.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    draw(cx);
    // The whole of row 1, without its line break.
    let start = text.find("word").unwrap();
    let end = start + "word ".repeat(80).len();
    show(
        &provider,
        vec![decoration(
            start..end,
            DecorationKind::Highlight,
            Tint::Information,
        )],
        cx,
    );
    let pieces: Vec<(usize, usize, Pixels)> = with_layout(&view, cx, |layout| {
        let frame = &layout.frame;
        (frame.rows.iter())
            .filter(|row| row.buffer_row == 1)
            .map(|row| {
                (
                    row.start,
                    row.end,
                    layout.origin.y + px(frame.row_top(row.row)),
                )
            })
            .collect()
    });
    assert!(pieces.len() > 1);
    let highlights = of_kind(&placed(&view, cx), &DecorationKind::Highlight);
    assert_eq!(highlights.len(), pieces.len());
    for ((bounds, _), &(from, to, top)) in highlights.iter().zip(&pieces) {
        assert_eq!(bounds.top(), top);
        assert_eq!(bounds.left(), x(&view, from, cx));
        // The caret at a wrap point is on the next row: measure its end on
        // this one.
        let right = with_layout(&view, cx, |layout| {
            let FrameLayout {
                frame,
                long_rows,
                glyph_widths,
                origin,
                ..
            } = layout;
            let row = frame.rows.iter().find(|row| row.start == from).unwrap();
            origin.x + px(frame.view_x(row.x_for_offset(to, long_rows, glyph_widths)))
        });
        assert_eq!(bounds.right(), right);
    }
}

fn pattern(text: &str) -> Pattern {
    Pattern::from_query(&FindQuery {
        text: text.into(),
        match_case: true,
        ..FindQuery::default()
    })
    .unwrap()
}

#[gpui::test]
fn painted_under_matches_and_the_selection_and_over_the_text(cx: &mut TestAppContext) {
    let text = "alpha beta\ngamma\n";
    let (view, provider, cx) = editor(text, cx);
    view.update(cx, |view, cx| {
        view.set_find(Some(pattern("beta")), cx);
        view.model_mut().select_range(0..5, false);
        cx.notify();
    });
    let painted = show(
        &provider,
        vec![
            decoration(0..11, DecorationKind::RowBackground, Tint::Added),
            decoration(6..10, DecorationKind::Highlight, Tint::Accent),
            decoration(0..5, DecorationKind::Underline { wavy: true }, Tint::Error),
            decoration(0..5, DecorationKind::AfterLine("note".into()), Tint::Hint),
            decoration(0..11, BAR, Tint::Added),
        ],
        cx,
    );
    let first = |stage: &str| {
        painted
            .iter()
            .position(|painted| *painted == stage)
            .unwrap_or_else(|| panic!("{stage} not in {painted:?}"))
    };
    assert!(first("backgrounds") < first("matches"));
    assert!(first("matches") < first("selection"));
    assert!(first("text") < first("underlines"));
    assert!(first("text") < first("after line"));
    assert!(first("text") < first("gutter"));
    // Row backgrounds go under highlights.
    let order: Vec<_> = (placed(&view, cx).into_iter())
        .filter(|(kind, ..)| {
            matches!(
                kind,
                DecorationKind::RowBackground | DecorationKind::Highlight
            )
        })
        .map(|(kind, ..)| kind)
        .collect();
    assert_eq!(
        order,
        [DecorationKind::RowBackground, DecorationKind::Highlight]
    );
}

/// The editor font's em, as the frame measures it.
fn em(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Pixels {
    let theme = theme(view, cx);
    cx.update(|window, _| {
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&gpui::font(theme.font_family.clone()));
        text_system
            .em_advance(font_id, theme.font_size)
            .unwrap_or(theme.font_size * 0.6)
    })
}

#[gpui::test]
fn after_line_text_follows_the_last_covered_row(cx: &mut TestAppContext) {
    let text = "one\ntwo\nthree\n";
    let (view, provider, cx) = editor(text, cx);
    let theme = theme(&view, cx);
    let note = |label: &str| DecorationKind::AfterLine(Arc::from(label));
    let painted = show(
        &provider,
        vec![
            decoration(0..2, note("first"), Tint::Accent),
            decoration(1..1, note("second"), Tint::Warning),
            // Rows 1 and 2: after row 2's end.
            decoration(4..10, note("third"), Tint::Information),
        ],
        cx,
    );
    assert!(painted.contains(&"after line"));
    let all = placed(&view, cx);
    let em = em(&view, cx);
    let shown = rows(&view, cx);

    let on_row = |label: &str| of_kind(&all, &note(label))[0];
    let (first, first_color) = on_row("first");
    let (second, second_color) = on_row("second");
    let (third, third_color) = on_row("third");
    assert_eq!(first.left(), x(&view, 3, cx) + em);
    assert_eq!(first.top(), shown[0].2);
    assert!(first.size.width > px(0.));
    // Several on a row go left to right, an em apart.
    assert!((second.left() - (first.right() + em)).abs() < px(0.01));
    assert_eq!(second.top(), shown[0].2);
    assert_eq!(third.left(), x(&view, 13, cx) + em);
    assert_eq!(third.top(), shown[2].2);
    assert_eq!(first_color, theme.cursor.opacity(0.6));
    assert_eq!(second_color, theme.warning.opacity(0.6));
    assert_eq!(third_color, theme.information.opacity(0.6));

    // Text and wrap are untouched: the rows end where they did.
    let ends = with_layout(&view, cx, |layout| {
        (layout.frame.rows.iter())
            .map(|row| row.end)
            .collect::<Vec<_>>()
    });
    assert_eq!(ends[..3], [3, 7, 13]);
}

#[gpui::test]
fn after_line_text_needs_its_row_in_the_frame(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    let (view, provider, cx) = editor(&text, cx);
    // From a shown row through one scrolled off: its text would go after
    // that one.
    let row_250 = text.find("row 250").unwrap();
    show(
        &provider,
        vec![decoration(
            0..row_250 + 1,
            DecorationKind::AfterLine("far".into()),
            Tint::Hint,
        )],
        cx,
    );
    assert!(placed(&view, cx).is_empty());
}

#[gpui::test]
fn tints_resolve_through_the_theme(cx: &mut TestAppContext) {
    let mut palette = EditorPalette::fastedit_dark().clone();
    palette.editor.change_added = Color::rgb(0x112233);
    palette.editor.change_modified = Color::rgb(0x223344);
    palette.editor.change_deleted = Color::rgb(0x334455);
    let theme = cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
    let expected = [
        (Tint::Error, theme.error),
        (Tint::Warning, theme.warning),
        (Tint::Information, theme.information),
        (Tint::Hint, theme.hint),
        (Tint::Accent, theme.cursor),
        (Tint::Muted, theme.foreground),
        (Tint::Added, Hsla::from(rgb(0x112233))),
        (Tint::Modified, Hsla::from(rgb(0x223344))),
        (Tint::Removed, Hsla::from(rgb(0x334455))),
    ];
    for (tint, color) in expected {
        assert_eq!(tint_color(&theme, tint), color, "{tint:?}");
    }
}

#[gpui::test]
fn queried_once_a_frame_for_the_shown_range(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    let (view, provider, cx) = editor(&text, cx);
    provider.borrow().queries.borrow_mut().clear();
    show(
        &provider,
        vec![decoration(0..3, DecorationKind::Highlight, Tint::Accent)],
        cx,
    );
    let shown = with_layout(&view, cx, |layout| {
        let rows = &layout.frame.rows;
        rows.first().unwrap().start..rows.last().unwrap().end
    });
    assert_eq!(*provider.borrow().queries.borrow(), [shown]);
}

#[gpui::test]
fn no_providers_paint_nothing_new(cx: &mut TestAppContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("one\ntwo\n"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    PAINTED.with(|painted| painted.borrow_mut().clear());
    draw(cx);
    assert!(placed(&view, cx).is_empty());
    assert_eq!(PAINTED.with(|painted| painted.take()), ["text"]);
}

#[gpui::test]
fn a_deletion_past_the_end_sits_under_the_last_row(cx: &mut TestAppContext) {
    let text = "one\ntwo";
    let (view, provider, cx) = editor(text, cx);
    show(
        &provider,
        vec![decoration(text.len()..text.len(), DELETION, Tint::Removed)],
        cx,
    );
    let shown = rows(&view, cx);
    let line_height = shown[1].2 - shown[0].2;
    let deletions = of_kind(&placed(&view, cx), &DELETION);
    assert_eq!(deletions.len(), 1);
    assert_eq!(deletions[0].0.top(), shown[1].2 + line_height - px(1.));
}
