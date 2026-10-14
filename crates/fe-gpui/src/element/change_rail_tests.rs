//! The git change indicator end to end, in a headless window: an editor
//! whose own model has a diff base lays out thin bars and a deletion mark
//! in the change lane and tints no row.

use fe_buffer::Buffer;
use fe_display::layout::Rect;
use fe_editor::{
    EditorModel,
    decorations::{DecorationKind, GutterMark},
};
use gpui::{Bounds, Pixels, TestAppContext, px};

use super::*;
use crate::{EditorTheme, EditorView, init};

const BASE: &str = "0\n1\n2\n3\n4\n5\n6\n7\n";
// Row 1 modified, the row "3" deleted before row 3, rows 6 and 7 added.
const TEXT: &str = "0\nONE\n2\n4\n5\n6\nx\ny\n7\n";

#[gpui::test]
fn changes_are_bars_in_the_lane_and_no_row_is_tinted(cx: &mut TestAppContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_diff_base(Some(BASE.into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    PAINTED.with(|painted| painted.borrow_mut().clear());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let painted = PAINTED.with(|painted| painted.take());
    assert!(!painted.contains(&"backgrounds"), "{painted:?}");

    let (all, theme, lane, tops, line_height) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let Rect { x, width, .. } = layout.frame.change_lane;
        let lane = layout.bounds(layout.frame.change_lane);
        assert_eq!(lane.origin.x, layout.origin.x + px(x));
        assert_eq!(lane.size.width, px(width));
        let tops: Vec<Pixels> = (layout.frame.rows.iter())
            .map(|row| layout.origin.y + px(layout.frame.row_top(row.row)))
            .collect();
        (
            layout.decorations.placed.clone(),
            view.theme.clone(),
            lane,
            tops,
            px(layout.frame.line_height),
        )
    });
    assert!(
        all.iter()
            .all(|placed| placed.kind != DecorationKind::RowBackground)
    );

    let bar = DecorationKind::Gutter(GutterMark::Bar);
    let bars: Vec<_> = all.iter().filter(|placed| placed.kind == bar).collect();
    assert_eq!(bars.len(), 2, "one bar per change");
    let colors = [theme.change_modified, theme.change_added];
    let spans = [(1usize, 1usize), (6, 2)];
    for ((bar, color), (row, rows)) in bars.iter().zip(colors).zip(spans) {
        assert_eq!(bar.color, color.opacity(0.6));
        assert_eq!(
            bar.bounds,
            Bounds::new(
                gpui::point(lane.origin.x, tops[row] + px(1.)),
                gpui::size(px(3.), line_height * rows as f32 - px(2.)),
            )
        );
        assert!(bar.bounds.left() >= lane.left() && bar.bounds.right() <= lane.right());
    }

    let deletion = DecorationKind::Gutter(GutterMark::Deletion);
    let marks: Vec<_> = all
        .iter()
        .filter(|placed| placed.kind == deletion)
        .collect();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].color, theme.change_deleted.opacity(1.0));
    assert_eq!(
        marks[0].bounds,
        Bounds::new(
            gpui::point(lane.origin.x, tops[3] - line_height / 4.),
            gpui::size(lane.size.width / 2., line_height / 2.),
        )
    );
    assert_eq!(marks[0].bounds.center().y, tops[3]);
}
