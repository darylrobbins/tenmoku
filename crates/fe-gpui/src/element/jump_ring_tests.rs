//! Direct targets side by side, or a row apart: no border covers another
//! target's char or a label.

use gpui::TestAppContext;

use super::{jump_style_tests::*, *};

/// What two rects share, beyond rounding.
fn overlap(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
    let eps = px(0.01);
    a.origin.x < b.right() - eps
        && b.origin.x < a.right() - eps
        && a.origin.y < b.bottom() - eps
        && b.origin.y < a.bottom() - eps
}

#[gpui::test]
fn adjacent_borders_spare_chars_and_labels(cx: &mut TestAppContext) {
    // After `a`: `aa` at 2 and `ab` at 3 are direct, side by side; row 1's
    // `aa` and `ab`, right below them, are labels.
    let (view, cx) = editor(cx, "x aab", 2, 600., 10.);
    cx.simulate_keystrokes("g s a");
    draw(&view, cx);
    let laid = laid_out(&view, cx);
    let border = px(2.);
    let cells: Vec<Bounds<Pixels>> = (laid.iter())
        .filter(|(_, state)| *state == JumpLabelState::Direct)
        .map(|(ring, _)| {
            Bounds::from_corners(
                point(ring.origin.x + border, ring.origin.y + border),
                point(ring.right() - border, ring.bottom() - border),
            )
        })
        .collect();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].right(), cells[1].origin.x);
    let boxes: Vec<Bounds<Pixels>> = (label_paints(&paints()).into_iter())
        .map(|(bounds, ..)| bounds)
        .collect();
    assert_eq!(boxes.len(), 2);

    let paints = paints();
    let edges: Vec<Bounds<Pixels>> = (paints.iter())
        .filter_map(|paint| match *paint {
            JumpPaint::DirectEdge { bounds, .. } => Some(bounds),
            _ => None,
        })
        .collect();
    assert!(!edges.is_empty());
    for edge in &edges {
        for cell in cells.iter().chain(&boxes) {
            assert!(!overlap(*edge, *cell), "{edge:?} covers {cell:?}");
        }
    }
    // Each target is still ringed: the outer edge above its char shows.
    for cell in &cells {
        let above = (edges.iter())
            .any(|edge| edge.bottom() == cell.origin.y && edge.origin.x <= cell.origin.x);
        assert!(above, "{cell:?}: {edges:?}");
    }
}
