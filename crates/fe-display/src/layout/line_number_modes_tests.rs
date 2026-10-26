//! The gutter's numbers in each of 'number' and 'relativenumber''s modes,
//! against the fake monospace measurer of [`super::tests`].

use fe_buffer::Buffer;

use crate::LineNumbers;

use super::{
    tests::{CELL, GUTTER, LINE, Line, Mono, WIDTH},
    *,
};

const ON: LineNumbers = LineNumbers {
    number: true,
    relative: false,
};
const RELATIVE: LineNumbers = LineNumbers {
    number: false,
    relative: true,
};
const HYBRID: LineNumbers = LineNumbers {
    number: true,
    relative: true,
};
const OFF: LineNumbers = LineNumbers {
    number: false,
    relative: false,
};

/// Twelve rows, `l00` to `l11`, row `k` starting at offset `4 * k`.
fn twelve_rows() -> String {
    (0..12)
        .map(|k| format!("l{k:02}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The start of row `row` of [`twelve_rows`].
fn at(row: usize) -> usize {
    4 * row
}

/// A 400 × 240 editor's frame of `text`, scrolled down `scroll_y`, its rows
/// wrapped or folded by `configure`, with carets at `carets`, the first the
/// primary.
fn lay_out(
    text: &str,
    numbers: LineNumbers,
    configure: impl FnOnce(&mut DisplayMap),
    carets: &[usize],
    scroll_y: f32,
) -> Frame<Line> {
    let buffer = Buffer::new(text);
    let mut display = DisplayMap::new(buffer.current(), 4);
    configure(&mut display);
    display.set_line_numbers(numbers);
    assert_eq!(display.line_numbers(), numbers);
    let selections: Vec<_> = carets
        .iter()
        .map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        })
        .collect();
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: carets[0],
        scroll: Vec2 { x: 0., y: scroll_y },
        autoscroll: false,
        width: WIDTH,
        height: 240.,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: "text",
            line_number: "number",
            active_line_number: "active",
        },
    };
    layout_frame(
        &input,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    )
}

/// The frame's numbers, by screen row in the frame.
fn numbers(frame: &Frame<Line>) -> Vec<(u32, &str)> {
    frame
        .line_numbers
        .iter()
        .map(|(origin, line)| ((origin.y / LINE).round() as u32, line.text.as_str()))
        .collect()
}

/// Just the numbers' text, for a frame with one per row.
fn texts(frame: &Frame<Line>) -> Vec<&str> {
    numbers(frame).into_iter().map(|(_, text)| text).collect()
}

/// The text of the numbers styled as a caret's row's.
fn active(frame: &Frame<Line>) -> Vec<&str> {
    frame
        .line_numbers
        .iter()
        .filter(|(_, line)| line.runs[0].style == "active")
        .map(|(_, line)| line.text.as_str())
        .collect()
}

#[test]
fn the_four_modes() {
    let text = twelve_rows();
    let frame = |numbers| lay_out(&text, numbers, |_| {}, &[at(5) + 1], 0.);

    let on = frame(ON);
    assert_eq!(
        texts(&on),
        [
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"
        ]
    );
    assert_eq!(active(&on), ["6"]);

    // nvim's `nonu rnu`: the caret's row shows 0.
    let relative = frame(RELATIVE);
    assert_eq!(
        texts(&relative),
        ["5", "4", "3", "2", "1", "0", "1", "2", "3", "4", "5", "6"]
    );
    assert_eq!(active(&relative), ["0"]);

    // nvim's `nu rnu`: the caret's row shows its own number.
    let hybrid = frame(HYBRID);
    assert_eq!(
        texts(&hybrid),
        ["5", "4", "3", "2", "1", "6", "1", "2", "3", "4", "5", "6"]
    );
    assert_eq!(active(&hybrid), ["6"]);

    // No numbers, but the rest of the gutter stays.
    let off = frame(OFF);
    assert!(off.line_numbers.is_empty());
    assert_eq!(off.change_lane, on.change_lane);

    // The numbers are right-aligned where they always were, and the gutter
    // is as wide in every mode.
    for (mode, frame) in [("relative", &relative), ("hybrid", &hybrid), ("off", &off)] {
        assert_eq!(frame.text_left, GUTTER, "{mode}");
        assert_eq!(frame.text_left, on.text_left, "{mode}");
    }
    let right = |frame: &Frame<Line>| -> Vec<f32> {
        frame
            .line_numbers
            .iter()
            .map(|(origin, line)| origin.x + line.width())
            .collect()
    };
    assert_eq!(right(&relative), right(&on));
    assert_eq!(right(&hybrid), right(&on));
}

#[test]
fn relative_counts_rows_like_nvim() {
    let text = twelve_rows();

    // A closed fold counts as one row: rows 2 and 3 hide behind row 1.
    let fold = |display: &mut DisplayMap| {
        display.fold(1..=3);
    };
    let folded = lay_out(&text, RELATIVE, fold, &[at(6)], 0.);
    assert_eq!(
        numbers(&folded)[..5],
        [(0, "4"), (1, "3"), (2, "2"), (3, "1"), (4, "0")]
    );
    // A caret inside the fold counts from its header, which hybrid numbers.
    let inside = lay_out(&text, HYBRID, fold, &[at(2) + 1], 0.);
    assert_eq!(texts(&inside)[..4], ["1", "2", "1", "2"]);
    // Scrolled past the fold, with the caret above the frame.
    let scrolled = lay_out(&text, RELATIVE, fold, &[at(0)], 2. * LINE);
    assert_eq!(texts(&scrolled), ["2", "3", "4", "5", "6", "7", "8", "9"]);

    // A wrapped row counts once, its number on its first row only.
    let text_wrapped = "a\nb\ncccc dddd eeee\nd\ne\nf";
    let wrap = |display: &mut DisplayMap| {
        display.set_wrap_width(Some(6.), &mut crate::Monospace);
        display.wrap_pending(0, usize::MAX, &mut crate::Monospace);
    };
    let caret = text_wrapped.rfind('e').unwrap();
    let wrapped = lay_out(text_wrapped, RELATIVE, wrap, &[caret], 0.);
    assert_eq!(
        numbers(&wrapped),
        [(0, "4"), (1, "3"), (2, "2"), (5, "1"), (6, "0"), (7, "1")]
    );

    // Several carets count from the primary; each caret's row is active.
    let several = lay_out(&text, HYBRID, |_| {}, &[at(8), at(2), at(10)], 0.);
    assert_eq!(
        texts(&several),
        ["8", "7", "6", "5", "4", "3", "2", "1", "9", "1", "2", "3"]
    );
    assert_eq!(active(&several), ["6", "9", "2"]);

    // The first and last rows.
    let first = lay_out(&text, RELATIVE, |_| {}, &[at(0)], 0.);
    assert_eq!(
        texts(&first),
        ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"]
    );
    let first = lay_out(&text, HYBRID, |_| {}, &[at(0)], 0.);
    assert_eq!(texts(&first)[..2], ["1", "1"]);
    let last = lay_out(&text, RELATIVE, |_| {}, &[text.len()], 0.);
    assert_eq!(
        texts(&last),
        ["11", "10", "9", "8", "7", "6", "5", "4", "3", "2", "1", "0"]
    );
    let last = lay_out(&text, HYBRID, |_| {}, &[text.len()], 0.);
    assert_eq!(texts(&last)[10..], ["1", "12"]);
}
