//! Long rows with hints and the inlay in them, scrolled anywhere: the slice
//! shaped for the view fills it, whichever of the texts come before it.

use fe_buffer::Buffer;

use super::*;

const VIEW: f64 = (WIDTH - GUTTER) as f64;

fn text(len: usize) -> Inlay<&'static str> {
    Inlay {
        offset: 0,
        text: "i".repeat(len),
        style: "hint",
    }
}

/// A row of 9,000 characters with `inlay` and `hints` (offset, length) in
/// it, laid out at `scroll_x`.
fn lay_out(
    inlay: Option<(usize, usize)>,
    hint_list: &[(usize, usize)],
    scroll_x: f32,
    long_rows: &mut LongRows,
) -> Frame<Line> {
    let buffer = Buffer::new(&"a".repeat(9_000));
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let inlay = inlay.map(|(offset, len)| Inlay {
        offset,
        style: "ghost",
        ..text(len)
    });
    let hint_list: Vec<_> = (hint_list.iter())
        .map(|&(offset, len)| Inlay {
            offset,
            ..text(len)
        })
        .collect();
    layout_frame_with_hints(
        &input(&display, &selections, scroll_x),
        &BlockMap::default(),
        inlay.as_ref(),
        &hint_list,
        &mut Mono,
        long_rows,
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    )
}

/// Whether the row's laid-out text starts at or before the view and ends at
/// or after it, or at the row's end.
fn fills(row: &RowLayout<Line>, scroll_x: f64) -> bool {
    let start = row.start_x();
    let end = start + f64::from(row.line.width());
    start <= scroll_x && (end >= scroll_x + VIEW || row.shown().end == row.end)
}

#[test]
fn an_inlay_before_a_hint_still_fills_the_view_past_the_hint() {
    let frame = lay_out(
        Some((0, 100)),
        &[(8_250, 100)],
        83_000.,
        &mut LongRows::default(),
    );
    let row = &frame.rows[0];
    assert!(row.slice.is_some(), "a long row");
    assert!(fills(row, 83_000.), "{:?} {}", row.shown(), row.start_x());
    assert_eq!(row.hints.len(), 1);
    assert_eq!(frame.hints.len(), 1);
    assert_eq!(frame.hints[0].0, 0);
}

#[test]
fn long_rows_fill_the_view_with_hints_and_the_inlay_anywhere() {
    // The inlay before, between, after and at the same offset as a hint.
    let hints = [(3_000, 40), (6_000, 60)];
    for inlay in [1_000, 3_000, 4_500, 6_000, 8_000] {
        let width = 9_000. * 10. + 100. + 40. * 10. + 60. * 10.;
        // A coarse sweep, plus views on either side of the hints and the inlay.
        let mut scrolls = Vec::new();
        let mut scroll = 0.;
        while scroll <= width {
            scrolls.push(scroll);
            scroll += 11_000.;
        }
        scrolls.push(width);
        // Each text's span in the laid-out row: where it starts with the
        // texts before it counted, and where it ends with those at its own
        // offset counted too (whichever order they come in).
        let spans = [(3_000, 400.), (6_000, 600.), (inlay, 100.)];
        for (at, _) in spans {
            let before: f64 = (spans.iter())
                .filter(|&&(other, _)| other < at)
                .map(|&(_, wide)| wide)
                .sum();
            let tied: f64 = (spans.iter())
                .filter(|&&(other, _)| other == at)
                .map(|&(_, wide)| wide)
                .sum();
            let start = at as f64 * 10. + before;
            let end = start + tied;
            scrolls.extend([start - 400., start, end - 400., end, end + 400.]);
        }
        for scroll in scrolls {
            let mut long_rows = LongRows::default();
            let frame = lay_out(Some((inlay, 10)), &hints, scroll as f32, &mut long_rows);
            let row = &frame.rows[0];
            assert!(
                fills(row, scroll),
                "inlay {inlay} scroll {scroll}: {:?} from {}",
                row.shown(),
                row.start_x()
            );
            let shown = row.shown();
            let offsets = (shown.start..shown.end)
                .step_by(13)
                .chain(std::iter::once(shown.end));
            for offset in offsets {
                let x = row.x_for_offset(offset, &mut long_rows, &mut Mono);
                assert_eq!(
                    row.offset_for_x(x, &mut long_rows, &mut Mono),
                    offset,
                    "inlay {inlay} scroll {scroll}"
                );
            }
        }
    }
}
