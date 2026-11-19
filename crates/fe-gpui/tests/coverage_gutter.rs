//! Test-coverage marks in the gutter (T701, cov2): a host's
//! [`GutterMark::Coverage`] decorations draw a thin strip per display row in
//! the change lane's gap, right of the git bar and never over it, in the
//! theme's covered, uncovered and partial colours; a partly covered row's is
//! two stacked dashes, so it reads without its colour. The gutter keeps its
//! widths (`gutter_b3.rs`), and the bars, the agent lane and the removed
//! lines' notches draw as they did without the marks.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_editor::{
    EditorModel,
    decorations::{
        CoverageMark, Decoration, DecorationKind, DecorationProvider, GutterMark, Tint, overlaps,
    },
};
use fe_gpui::{DrawnGutter, EditorTheme, EditorView, GutterAnnotations};
use gpui::{Bounds, Entity, Hsla, Pixels, TestAppContext, VisualTestContext, px};

const TEXT: &str = "one\ntwo\nthree\nfour\nfive\nsix\n";

/// Fixed decorations, as a coverage or a git provider would hand out.
struct Fixed(Vec<Decoration>);

impl DecorationProvider for Fixed {
    fn decorations(&self, _: &BufferSnapshot, range: Range<usize>, out: &mut Vec<Decoration>) {
        out.extend(
            (self.0.iter())
                .filter(|item| overlaps(&item.range, &range))
                .cloned(),
        );
    }
}

/// An editor on [`TEXT`] in the dark theme at `font_size`, drawn with
/// `decorations`.
fn editor(
    font_size: f32,
    decorations: Vec<Decoration>,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.add_decoration_provider(Rc::new(RefCell::new(Fixed(decorations))));
        let mut theme = EditorTheme::default_dark(cx);
        theme.font_size = px(font_size);
        EditorView::new(model, theme, cx)
    });
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn drawn(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> DrawnGutter {
    view.read_with(cx, |view, _| view.drawn_gutter().expect("drawn").clone())
}

/// Buffer row `row`'s top and bottom, as the last frame laid it out.
fn row_span(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> (Pixels, Pixels) {
    let bounds = view.update(cx, |view, _| {
        let offset = view.model().text().row_start(row);
        view.caret_bounds_at(offset).expect("row in view")
    });
    (bounds.top(), bounds.bottom())
}

/// A decoration of `kind` over [`TEXT`]'s `rows`.
fn rows(rows: Range<u32>, kind: DecorationKind, tint: Tint) -> Decoration {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(TEXT.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    Decoration::new(
        starts[rows.start as usize]..starts[rows.end as usize],
        kind,
        tint,
    )
}

fn coverage(row: u32, mark: CoverageMark) -> Decoration {
    // The tint is unused: the theme's colour for the mark draws it.
    rows(
        row..row + 1,
        DecorationKind::Gutter(GutterMark::Coverage(mark)),
        Tint::Muted,
    )
}

fn bar(rows_: Range<u32>) -> Decoration {
    rows(
        rows_,
        DecorationKind::Gutter(GutterMark::Bar),
        Tint::Modified,
    )
}

/// The coverage marks drawn on the row from `top` to `bottom`, by their
/// centres: their bounds, kinds and colours, top to bottom.
fn marks_on(
    drawn: &DrawnGutter,
    (top, bottom): (Pixels, Pixels),
) -> Vec<(Bounds<Pixels>, CoverageMark, Hsla)> {
    (drawn.coverage.iter())
        .filter(|(bounds, _, _)| (top..bottom).contains(&bounds.center().y))
        .copied()
        .collect()
}

fn intersect(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
    a.left() < b.right() && b.left() < a.right() && a.top() < b.bottom() && b.top() < a.bottom()
}

#[gpui::test]
fn coverage_marks_draw_in_the_lane_beside_the_change_bar(cx: &mut TestAppContext) {
    for (font_size, strip) in [(13., 2.), (20., 3.)] {
        let plain = {
            let (view, cx) = editor(font_size, Vec::new(), cx);
            drawn(&view, cx)
        };
        // A changed run over rows 0 through 3, its rows covered,
        // uncovered, partly covered and covered.
        let decorations = vec![
            bar(0..4),
            coverage(0, CoverageMark::Covered),
            coverage(1, CoverageMark::Uncovered),
            coverage(2, CoverageMark::Partial),
            coverage(3, CoverageMark::Covered),
        ];
        let (view, cx) = editor(font_size, decorations, cx);
        let theme = view.read_with(cx, |view, _| view.theme().clone());
        let drawn = drawn(&view, cx);
        // No column moved or widened: the marks take no room of their own.
        assert_eq!(drawn.columns, plain.columns, "{font_size}");
        assert_eq!(drawn.text_left, plain.text_left, "{font_size}");
        if font_size == 13. {
            let widths: Vec<f32> = (drawn.columns.iter())
                .map(|column| column.size.width.into())
                .collect();
            assert_eq!(widths, [4., 38., 10., 16., 3., 10.]);
        }
        assert_eq!(drawn.bars.len(), 1, "{font_size}: {:?}", drawn.bars);
        let (bar, _) = drawn.bars[0];
        let gap = drawn.columns[5];

        let want = [
            (CoverageMark::Covered, theme.coverage_covered, 1),
            (CoverageMark::Uncovered, theme.coverage_uncovered, 1),
            (CoverageMark::Partial, theme.coverage_partial, 2),
            (CoverageMark::Covered, theme.coverage_covered, 1),
        ];
        let mut per_row = Vec::new();
        for (row, (mark, color, pieces)) in (0..).zip(want) {
            let span = row_span(&view, row, cx);
            let marks = marks_on(&drawn, span);
            assert_eq!(marks.len(), pieces, "{font_size}: row {row}: {marks:?}");
            for &(bounds, kind, drawn_color) in &marks {
                assert_eq!((kind, drawn_color), (mark, color), "{font_size}: row {row}");
                // A thin strip in the lane's gap: right of the bar, never
                // over it, short of the code.
                assert_eq!(bounds.size.width, px(strip), "{font_size}: row {row}");
                assert!(
                    bounds.left() >= bar.right(),
                    "{font_size}: {bounds:?} {bar:?}"
                );
                assert!(
                    bounds.left() >= gap.left(),
                    "{font_size}: {bounds:?} {gap:?}"
                );
                assert!(bounds.right() <= drawn.text_left, "{font_size}: {bounds:?}");
                assert!(!intersect(bounds, bar), "{font_size}: {bounds:?} {bar:?}");
            }
            per_row.push(marks);
        }
        // Every mark in one lane, at one x.
        let left = drawn.coverage[0].0.left();
        assert!(drawn.coverage.iter().all(|(b, _, _)| b.left() == left));
        assert_eq!(drawn.coverage.len(), 5, "{font_size}");

        // Covered and uncovered are one strip down the row; partial is two
        // dashes, one above the other with a gap, each under half as tall.
        let whole = per_row[0][0].0;
        assert_eq!(per_row[1][0].0.size, whole.size, "{font_size}");
        let [(upper, _, _), (lower, _, _)] = per_row[2][..] else {
            unreachable!()
        };
        assert_eq!(upper.size, lower.size, "{font_size}");
        assert!(
            lower.top() > upper.bottom(),
            "{font_size}: {upper:?} {lower:?}"
        );
        assert!(upper.size.height * 2. < whole.size.height, "{font_size}");
        // The two dashes span what one strip would on its row.
        let (top, _) = row_span(&view, 0, cx);
        let (partial_top, _) = row_span(&view, 2, cx);
        assert_eq!(upper.top() - partial_top, whole.top() - top, "{font_size}");
        assert_eq!(
            lower.bottom() - upper.top(),
            whole.size.height,
            "{font_size}"
        );
    }
}

#[gpui::test]
fn coverage_marks_do_not_hide_or_move_the_change_bar_or_agent_lane(cx: &mut TestAppContext) {
    // Rows 1 through 4 changed, with rows removed above row 5; an agent
    // wrote rows 3 and 4.
    let git = vec![
        bar(1..5),
        rows(
            5..5,
            DecorationKind::Gutter(GutterMark::Deletion),
            Tint::Removed,
        ),
    ];
    let marks = vec![
        coverage(1, CoverageMark::Covered),
        coverage(2, CoverageMark::Partial),
        coverage(3, CoverageMark::Uncovered),
        coverage(4, CoverageMark::Covered),
        coverage(5, CoverageMark::Uncovered),
    ];
    let with_agent = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            let snapshot = view.model().snapshot().clone();
            let text = snapshot.text();
            let rows = snapshot.anchor_before(text.row_start(3))
                ..snapshot.anchor_before(text.row_start(5));
            let annotations = GutterAnnotations {
                version: snapshot.version(),
                entries: Vec::new(),
                hover: Rc::new(|_, _| None),
                on_click: Rc::new(|_, _, _, _| {}),
            };
            view.set_gutter_annotations_with_agent_lane(annotations, vec![rows], cx);
        });
        draw(cx);
    };

    let before = {
        let (view, cx) = editor(13., git.clone(), cx);
        with_agent(&view, cx);
        drawn(&view, cx)
    };
    assert_eq!(before.coverage, []);

    let (view, cx) = editor(13., [git, marks].concat(), cx);
    with_agent(&view, cx);
    let drawn = drawn(&view, cx);

    // The bar, the agent lane's dots and the notch draw as without the
    // marks, in the same places and colours.
    assert_eq!(drawn.columns, before.columns);
    assert_eq!(drawn.bars, before.bars);
    assert_eq!(drawn.agent_dots, before.agent_dots);
    assert_eq!(drawn.agent_dot_color, before.agent_dot_color);
    assert_eq!(drawn.notches, before.notches);
    assert!(!drawn.bars.is_empty());
    assert!(drawn.agent_dots.len() >= 4, "{:?}", drawn.agent_dots);
    assert_eq!(drawn.notches.len(), 1, "{:?}", drawn.notches);

    // A row with a git bar and a coverage mark draws both: rows 1 and 2 keep
    // the person's bar beside their marks; the agent's rows 3 and 4 their
    // dots beside theirs.
    let span = row_span(&view, 1, cx);
    let (bar, _) = drawn.bars[0];
    assert!(
        bar.top() <= span.1 && span.0 <= bar.bottom(),
        "{bar:?} on row 1"
    );
    for row in 1..=5 {
        let span = row_span(&view, row, cx);
        let marks = marks_on(&drawn, span);
        assert!(!marks.is_empty(), "row {row} has its mark");
        for &(bounds, _, _) in &marks {
            for &(bar, _) in &drawn.bars {
                assert!(
                    !intersect(bounds, bar),
                    "row {row}: {bounds:?} over {bar:?}"
                );
            }
            for &dot in &drawn.agent_dots {
                assert!(
                    !intersect(bounds, dot),
                    "row {row}: {bounds:?} over {dot:?}"
                );
            }
            let (notch, _) = drawn.notches[0];
            assert!(
                !intersect(bounds, notch),
                "row {row}: {bounds:?} over {notch:?}"
            );
        }
    }
    let agent_rows = (row_span(&view, 3, cx).0, row_span(&view, 4, cx).1);
    assert!(
        (drawn.agent_dots.iter())
            .any(|dot| agent_rows.0 <= dot.top() && dot.bottom() <= agent_rows.1)
    );
    assert_eq!(marks_on(&drawn, agent_rows).len(), 2);
}
