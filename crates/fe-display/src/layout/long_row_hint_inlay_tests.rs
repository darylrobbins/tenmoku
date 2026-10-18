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
        let mut scroll = 0.;
        while scroll <= width {
            let mut long_rows = LongRows::default();
            let frame = lay_out(Some((inlay, 10)), &hints, scroll as f32, &mut long_rows);
            let row = &frame.rows[0];
            assert!(
                fills(row, scroll),
                "inlay {inlay} scroll {scroll}: {:?} from {}",
                row.shown(),
                row.start_x()
            );
            for offset in row.shown().start..=row.shown().end {
                let x = row.x_for_offset(offset, &mut long_rows, &mut Mono);
                assert_eq!(
                    row.offset_for_x(x, &mut long_rows, &mut Mono),
                    offset,
                    "inlay {inlay} scroll {scroll}"
                );
            }
            scroll += 2_000.;
        }
    }
}
