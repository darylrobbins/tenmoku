//! Git change marks in the frame's change lane, in a headless window: one
//! rounded bar per run of rows, inset so adjacent changes stay apart, and
//! removed lines' notches at the lane's left edge.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_display::layout::Rect;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, GutterMark, Tint, overlaps},
};
use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// A provider handing out fixed decorations.
#[derive(Default)]
struct Fixed(Vec<Decoration>);

impl DecorationProvider for Fixed {
    fn decorations(
        &self,
        _: &fe_buffer::BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        out.extend(
            self.0
                .iter()
                .filter(|item| overlaps(&item.range, &range))
                .cloned(),
        );
    }
}

/// An editor on `text`, soft wrapped or not, drawn with `items`.
fn editor<'a>(
    text: &str,
    soft_wrap: bool,
    items: Vec<Decoration>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_string();
    let provider = Rc::new(RefCell::new(Fixed(items)));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_soft_wrap(soft_wrap);
        model.add_decoration_provider(provider);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

/// What the last frame laid out of the decorations, in window coordinates.
fn placed(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Placed> {
    view.read_with(cx, |view, _| {
        view.layout
            .as_ref()
            .expect("drawn")
            .decorations
            .placed
            .clone()
    })
}

fn of_kind(placed: &[Placed], kind: &DecorationKind) -> Vec<Bounds<Pixels>> {
    (placed.iter())
        .filter(|placed| placed.kind == *kind)
        .map(|placed| placed.bounds)
        .collect()
}

/// The frame's change lane and line height, in window coordinates.
fn lane(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Bounds<Pixels>, Pixels) {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let Rect { x, width, .. } = layout.frame.change_lane;
        let lane = layout.bounds(layout.frame.change_lane);
        assert_eq!(lane.origin.x, layout.origin.x + px(x));
        assert_eq!(lane.size.width, px(width));
        (lane, px(layout.frame.line_height))
    })
}

/// The window tops of the last frame's display rows of `buffer_rows`.
fn tops(
    view: &Entity<EditorView>,
    buffer_rows: Range<u32>,
    cx: &mut VisualTestContext,
) -> Vec<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        (frame.rows.iter())
            .filter(|row| buffer_rows.contains(&row.buffer_row))
            .map(|row| layout.origin.y + px(frame.row_top(row.row)))
            .collect()
    })
}

/// The buffer offset of the start of each row of `text`, and its end.
fn row_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    starts.push(text.len());
    starts
}

const BAR: DecorationKind = DecorationKind::Gutter(GutterMark::Bar);
const DELETION: DecorationKind = DecorationKind::Gutter(GutterMark::Deletion);

const SHORT: &str = "zero\none\ntwo\nthree\nfour\nfive\nsix\n";

fn decoration(range: Range<usize>, kind: DecorationKind, tint: Tint) -> Decoration {
    Decoration::new(range, kind, tint)
}

/// A bar's and a notch's corners are rounded by half the smaller side;
/// nothing else is rounded.
fn assert_corners(placed: &[Placed]) {
    for placed in placed {
        let size = placed.bounds.size;
        let want = match placed.kind {
            DecorationKind::Gutter(_) => size.width.min(size.height) / 2.,
            _ => px(0.),
        };
        assert_eq!(placed.corner_radius, want, "{:?}", placed.kind);
    }
}

#[gpui::test]
fn a_bar_is_one_rounded_rect_down_its_rows(cx: &mut TestAppContext) {
    let starts = row_starts(SHORT);
    // Rows 1 through 3.
    let items = vec![
        decoration(
            starts[1]..starts[4],
            DecorationKind::RowBackground,
            Tint::Added,
        ),
        decoration(starts[1]..starts[4], BAR, Tint::Added),
    ];
    let (view, cx) = editor(SHORT, false, items, cx);
    let (lane, line_height) = lane(&view, cx);
    let tops = tops(&view, 1..4, cx);
    assert_eq!(tops.len(), 3);

    let all = placed(&view, cx);
    let bars = of_kind(&all, &BAR);
    assert_eq!(
        bars,
        [Bounds::new(
            gpui::point(lane.origin.x, tops[0] + px(1.)),
            gpui::size(px(3.), line_height * 3. - px(2.)),
        )]
    );
    assert_eq!(of_kind(&all, &DecorationKind::RowBackground).len(), 3);
    assert_corners(&all);
    let bar = all.iter().find(|placed| placed.kind == BAR).unwrap();
    assert_eq!(bar.corner_radius, px(1.5));
}

#[gpui::test]
fn a_bar_runs_down_every_wrapped_piece(cx: &mut TestAppContext) {
    let long = "word ".repeat(80);
    let text = format!("first\n{long}\n{long}\nthird\nfourth\n");
    let starts = row_starts(&text);
    // Rows 1 through 3, rows 1 and 2 wrapped.
    let items = vec![decoration(starts[1]..starts[4], BAR, Tint::Modified)];
    let (view, cx) = editor(&text, true, items, cx);
    let (lane, line_height) = lane(&view, cx);
    let tops = tops(&view, 1..4, cx);
    assert!(tops.len() > 3, "{} display rows", tops.len());

    let all = placed(&view, cx);
    assert_eq!(
        of_kind(&all, &BAR),
        [Bounds::new(
            gpui::point(lane.origin.x, tops[0] + px(1.)),
            gpui::size(px(3.), line_height * tops.len() as f32 - px(2.)),
        )]
    );
    assert_corners(&all);
}

#[gpui::test]
fn adjacent_bars_stay_apart(cx: &mut TestAppContext) {
    let starts = row_starts(SHORT);
    let items = vec![
        decoration(starts[1]..starts[3], BAR, Tint::Modified),
        decoration(starts[3]..starts[5], BAR, Tint::Added),
    ];
    let (view, cx) = editor(SHORT, false, items, cx);
    let (lane, line_height) = lane(&view, cx);
    let tops = tops(&view, 1..5, cx);

    let all = placed(&view, cx);
    let bars = of_kind(&all, &BAR);
    let bar = |top: Pixels| {
        Bounds::new(
            gpui::point(lane.origin.x, top + px(1.)),
            gpui::size(px(3.), line_height * 2. - px(2.)),
        )
    };
    assert_eq!(bars, [bar(tops[0]), bar(tops[2])]);
    assert_eq!(bars[1].top() - bars[0].bottom(), px(2.));
    assert_corners(&all);
}

/// Asserts `mark` is a notch at the lane's left edge, twice the 3 px bar
/// wide and 2 px tall at 13 px, centred on `edge`.
fn assert_deletion(mark: Bounds<Pixels>, lane: Bounds<Pixels>, _: Pixels, edge: Pixels) {
    assert_eq!(
        mark,
        Bounds::new(
            gpui::point(lane.origin.x, edge - px(1.)),
            gpui::size(px(6.), px(2.)),
        )
    );
    assert_eq!(mark.center().y, edge);
}

#[gpui::test]
fn a_deletion_is_a_notch_on_a_row_edge(cx: &mut TestAppContext) {
    let starts = row_starts(SHORT);
    let items = vec![
        decoration(starts[2]..starts[2], DELETION, Tint::Removed),
        decoration(0..starts[1], DecorationKind::Highlight, Tint::Accent),
    ];
    let (view, cx) = editor(SHORT, false, items, cx);
    let (lane, line_height) = lane(&view, cx);
    let tops = tops(&view, 2..3, cx);

    let all = placed(&view, cx);
    let deletions = of_kind(&all, &DELETION);
    assert_eq!(deletions.len(), 1);
    assert_deletion(deletions[0], lane, line_height, tops[0]);
    assert_corners(&all);
}

#[gpui::test]
fn a_deletion_past_the_end_sits_on_the_last_rows_bottom_edge(cx: &mut TestAppContext) {
    let text = "one\ntwo";
    let items = vec![decoration(text.len()..text.len(), DELETION, Tint::Removed)];
    let (view, cx) = editor(text, false, items, cx);
    let (lane, line_height) = lane(&view, cx);
    let tops = tops(&view, 1..2, cx);

    let all = placed(&view, cx);
    let deletions = of_kind(&all, &DELETION);
    assert_eq!(deletions.len(), 1);
    assert_deletion(deletions[0], lane, line_height, tops[0] + line_height);
    assert_corners(&all);
}

#[gpui::test]
fn the_bar_clears_the_text_by_0_6_em(cx: &mut TestAppContext) {
    let starts = row_starts(SHORT);
    let items = vec![decoration(starts[1]..starts[4], BAR, Tint::Added)];
    let (view, cx) = editor(SHORT, false, items, cx);
    let text_left = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(layout.frame.text_bounds()).left()
    });
    // The em as layout measures it, from the theme's font and size.
    let em = cx.update(|window, cx| {
        let theme = &view.read(cx).theme;
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&gpui::font(theme.font_family.clone()));
        text_system
            .em_advance(font_id, theme.font_size)
            .unwrap_or(theme.font_size * 0.6)
    });
    let all = placed(&view, cx);
    let bars = of_kind(&all, &BAR);
    assert!(!bars.is_empty());
    let (lane, _) = lane(&view, cx);
    for bar in bars {
        assert_eq!(bar.left(), lane.left());
        assert_eq!(bar.size.width, px(3.));
        assert!(
            bar.right() + em * 0.6 <= text_left,
            "{bar:?} {em:?} {text_left:?}"
        );
    }
    for placed in all.iter().filter(|placed| placed.kind == BAR) {
        assert!((placed.color.a - 0.6).abs() < 1e-6);
    }
}

#[gpui::test]
fn a_deletion_is_a_full_colour_notch_with_round_ends(cx: &mut TestAppContext) {
    let starts = row_starts(SHORT);
    let items = vec![decoration(starts[2]..starts[2], DELETION, Tint::Removed)];
    let (view, cx) = editor(SHORT, false, items, cx);
    let (lane_box, line_height) = lane(&view, cx);
    let row_tops = tops(&view, 2..3, cx);
    let all = placed(&view, cx);
    let mark = all.iter().find(|placed| placed.kind == DELETION).unwrap();
    assert_deletion(mark.bounds, lane_box, line_height, row_tops[0]);
    assert_eq!(mark.color.a, 1.0);
    assert_eq!(mark.corner_radius, px(1.));

    let text = "one\ntwo";
    let items = vec![decoration(text.len()..text.len(), DELETION, Tint::Removed)];
    let (view, cx) = editor(text, false, items, cx);
    let (lane_box, line_height) = lane(&view, cx);
    let row_tops = tops(&view, 1..2, cx);
    let all = placed(&view, cx);
    let mark = all.iter().find(|placed| placed.kind == DELETION).unwrap();
    assert_deletion(
        mark.bounds,
        lane_box,
        line_height,
        row_tops[0] + line_height,
    );
    assert_eq!(mark.color.a, 1.0);
}
