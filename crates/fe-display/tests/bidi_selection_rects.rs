//! A selection over right-to-left text is drawn as one rect per visual
//! piece, covering exactly the selected glyphs, with a selected line break
//! marked once, right of the row's text.

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{
    CharWidth, DisplayMap, LongRows,
    layout::{
        Frame, FrameInput, FrameStyles, MeasuredLine, StyleRun, TextMeasure, Vec2, layout_frame,
    },
};

const EM: f32 = 10.;
const LINE: f32 = 20.;

/// Every char an em wide.
struct Cells;

impl CharWidth for Cells {
    fn char_width(&mut self, _: char) -> f32 {
        EM
    }
}

fn is_rtl(c: char) -> bool {
    ('\u{590}'..='\u{5ff}').contains(&c)
}

/// A line of one-em chars, with each run of Hebrew letters painted right to
/// left, and carets on a letter's logical edge.
struct Line {
    /// Each char's byte offset, column it's painted in and direction.
    chars: Vec<(usize, usize, bool)>,
    len: usize,
}

impl Line {
    fn new(text: &str) -> Self {
        let mut chars: Vec<(usize, usize, bool)> = (text.char_indices().enumerate())
            .map(|(col, (ix, c))| (ix, col, is_rtl(c)))
            .collect();
        let mut run = 0;
        while run < chars.len() {
            let end = run + chars[run..].iter().take_while(|c| c.2).count();
            for (ix, c) in chars[run..end].iter_mut().enumerate() {
                c.1 = end - 1 - ix;
            }
            run = end.max(run + 1);
        }
        Self {
            chars,
            len: text.len(),
        }
    }

    /// The char starting at byte `index`.
    fn char_at(&self, index: usize) -> Option<&(usize, usize, bool)> {
        self.chars.iter().find(|c| c.0 == index)
    }
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.chars.len() as f32 * EM
    }

    fn x_for_index(&self, index: usize) -> f32 {
        let col = match self.char_at(index) {
            // A right-to-left letter starts at its right edge.
            Some(&(_, col, rtl)) => col + usize::from(rtl),
            None => match self.chars.last() {
                // The end, past the last letter: its left edge if it's
                // right to left.
                Some(&(_, col, rtl)) if index >= self.len => col + usize::from(!rtl),
                _ => 0,
            },
        };
        col as f32 * EM
    }

    fn index_for_x(&self, x: f32) -> usize {
        (self.chars.iter().map(|c| c.0))
            .chain([self.len])
            .min_by(|&a, &b| {
                let a = (self.x_for_index(a) - x).abs();
                let b = (self.x_for_index(b) - x).abs();
                a.total_cmp(&b)
            })
            .unwrap_or(0)
    }

    fn visual_ranges(&self, range: Range<usize>) -> Vec<Range<f32>> {
        let mut cols: Vec<usize> = (self.chars.iter())
            .filter(|c| range.contains(&c.0))
            .map(|c| c.1)
            .collect();
        cols.sort_unstable();
        let mut pieces: Vec<Range<usize>> = Vec::new();
        for col in cols {
            match pieces.last_mut() {
                Some(last) if last.end == col => last.end += 1,
                _ => pieces.push(col..col + 1),
            }
        }
        if pieces.is_empty() {
            let x = self.x_for_index(range.start);
            return vec![x..x];
        }
        (pieces.into_iter())
            .map(|piece| piece.start as f32 * EM..piece.end as f32 * EM)
            .collect()
    }
}

struct Shaper;

impl TextMeasure for Shaper {
    type Style = ();
    type Line = Line;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> Line {
        Line::new(text)
    }
}

/// A frame of `text` with `range` selected.
fn lay_out(text: &str, range: Range<usize>) -> Frame<Line> {
    lay_out_scrolled(text, range, 0.)
}

/// A frame of `text` with `range` selected, scrolled `scroll_x` right.
fn lay_out_scrolled(text: &str, range: Range<usize>, scroll_x: f32) -> Frame<Line> {
    let display = DisplayMap::new(Buffer::new(text).current(), 4);
    let selections = [ResolvedSelection {
        range,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: 0,
        scroll: Vec2 { x: scroll_x, y: 0. },
        autoscroll: false,
        width: 1000.,
        height: 400.,
        line_height: LINE,
        em: EM,
        styles: FrameStyles {
            text: (),
            line_number: (),
            active_line_number: (),
        },
    };
    layout_frame(
        &input,
        &mut Shaper,
        &mut LongRows::default(),
        &mut Cells,
        |_| Vec::new(),
        |_| Vec::new(),
    )
}

/// The selection rects of `frame` on display row `row`, as x ranges in ems
/// from the text's origin, left to right.
fn row_rects(frame: &Frame<Line>, row: u32) -> Vec<Range<f32>> {
    let top = frame.row_top(row);
    let ems = |x: f32| ((x - frame.text_left) / EM * 100.).round() / 100.;
    let mut rects: Vec<_> = (frame.selections.iter())
        .filter(|rect| rect.y == top)
        .map(|rect| ems(rect.x)..ems(rect.x + rect.width))
        .collect();
    rects.sort_by(|a, b| a.start.total_cmp(&b.start));
    rects
}

/// Painted a b ם ו ל ש c d.
const MIXED: &str = "ab\u{5e9}\u{5dc}\u{5d5}\u{5dd}cd";

#[test]
fn rtl_selection_across_a_direction_change_draws_one_rect_per_visual_piece() {
    // b and ש: b, then ש at the run's right end; not the letters between.
    let frame = lay_out(MIXED, 1..4);
    assert_eq!(row_rects(&frame, 0), [1. ..2., 5. ..6.]);
    // ש through c: the whole run, then c, which touch: one rect.
    let frame = lay_out(MIXED, 2..11);
    assert_eq!(row_rects(&frame, 0), [2. ..7.]);
}

#[test]
fn rtl_selection_wholly_inside_a_run_is_one_rect_over_its_glyphs() {
    // ל and ו.
    let frame = lay_out(MIXED, 4..8);
    assert_eq!(row_rects(&frame, 0), [3. ..5.]);
}

#[test]
fn selected_line_break_after_rtl_text_is_marked_once_at_the_right_end() {
    // Painted a b ל ש: the row ends at ל's left edge, but its text at ש's
    // right.
    let text = "ab\u{5e9}\u{5dc}\nx";
    // ל and the line break: ל, then the mark right of ש.
    let frame = lay_out(text, 4..7);
    assert_eq!(row_rects(&frame, 0), [2. ..3., 4. ..4.6]);
    // The whole row and the line break: one rect, the mark at its end.
    let frame = lay_out(text, 0..7);
    assert_eq!(row_rects(&frame, 0), [0. ..4.6]);
    // Only the line break.
    let frame = lay_out(text, 6..7);
    assert_eq!(row_rects(&frame, 0), [4. ..4.6]);
}

#[test]
fn ltr_selection_rects_are_unchanged() {
    let frame = lay_out("abc\ndef", 1..6);
    assert_eq!(row_rects(&frame, 0), [1. ..3.6]);
    assert_eq!(row_rects(&frame, 1), [0. ..2.]);
}

/// Every selection rect of `frame` is outside `row`'s shaped slice as drawn,
/// both clamped as the rects are; there's at least one.
fn assert_rects_outside_the_slice(frame: &Frame<Line>, row: usize) {
    let row = &frame.rows[row];
    let clamp = |x: f32| x.clamp(frame.text_left - EM, frame.width + EM);
    let left = frame.text_left + (row.start_x() as f32 - frame.scroll.x);
    let (left, right) = (clamp(left), clamp(left + row.line.width()));
    assert!(left < right, "the slice is in view");
    assert!(!frame.selections.is_empty());
    for rect in &frame.selections {
        assert!(
            rect.x + rect.width <= left || rect.x >= right,
            "{rect:?} is over the slice, {left}..{right}"
        );
    }
}

/// A scrolled row of 5,000 Hebrew letters, the scroll, and the part of it
/// shaped.
fn long_rtl_row() -> (String, f32, Range<usize>) {
    let text = "\u{5e9}".repeat(5000);
    let scroll_x = 2000. * EM;
    let shown = lay_out_scrolled(&text, 0..0, scroll_x).rows[0].shown();
    assert!(0 < shown.start && shown.end < text.len(), "{shown:?}");
    (text, scroll_x, shown)
}

#[test]
fn rtl_selection_up_to_a_long_rows_slice_stops_at_its_left_edge() {
    let (text, scroll_x, shown) = long_rtl_row();
    let frame = lay_out_scrolled(&text, 0..shown.start, scroll_x);
    assert_eq!(frame.rows[0].shown(), shown);
    assert_rects_outside_the_slice(&frame, 0);
}

#[test]
fn rtl_selection_from_a_long_rows_slice_end_starts_at_its_right_edge() {
    let (text, scroll_x, shown) = long_rtl_row();
    let frame = lay_out_scrolled(&text, shown.end..text.len(), scroll_x);
    assert_eq!(frame.rows[0].shown(), shown);
    assert_rects_outside_the_slice(&frame, 0);
}
