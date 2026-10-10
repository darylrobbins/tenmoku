//! Long rows whose slices are capped at [`MAX_SHAPED_ROW_BYTES`]: grapheme
//! clusters longer than [`MAX_CLUSTER_BYTES`], which the row index cuts
//! into pieces, measured with the fake measurer of
//! [`super::cluster_row_tests`], and widening that reaches the cap.

use fe_buffer::Buffer;
use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

use super::{
    cluster_row_tests::{Clusters, E_ACUTE, FAMILY, cluster_advance},
    tests::{CELL, advance},
    *,
};
use crate::{MAX_CLUSTER_BYTES, long_row::CHUNK};

/// The units the index measures in `row`, with their x: its grapheme
/// clusters, except that one longer than [`MAX_CLUSTER_BYTES`] is cut that
/// long, measured char by char, and the rest segmented afresh. The offset
/// and x of each unit's start, then the row's end.
fn units(row: &str) -> Vec<(usize, f64)> {
    let mut units = vec![(0, 0.)];
    let (mut at, mut x) = (0, 0.);
    while at < row.len() {
        let cluster = row[at..].graphemes(true).next().expect("not empty");
        let (len, width) = if cluster.len() > MAX_CLUSTER_BYTES {
            let len = cluster.floor_char_boundary(MAX_CLUSTER_BYTES);
            (len, cluster[..len].chars().map(advance).sum())
        } else {
            (cluster.len(), cluster_advance(cluster))
        };
        (at, x) = (at + len, x + f64::from(width));
        units.push((at, x));
    }
    units
}

#[test]
fn long_clusters_are_cut_into_pieces() {
    // A cluster of the longest measured whole, then a 301-byte one within a
    // chunk, and one longer than a chunk, which crosses marks.
    let tail = format!("ab{FAMILY}c{E_ACUTE}").repeat(30);
    let mut row = format!("xy{}", tail);
    let longest = row.len();
    row += &format!("a{}", "\u{301}".repeat((MAX_CLUSTER_BYTES - 1) / 2));
    row += &tail;
    let short = row.len();
    row += &format!("a{}", "\u{301}".repeat(150));
    row += &tail;
    let long = row.len();
    row += &format!("z{}", "\u{301}".repeat(CHUNK * 3 / 2));
    row += &tail;

    let text = Text::new(&row);
    let units = units(&row);
    let is_boundary = |offset| units.iter().any(|&(at, _)| at == offset);
    // The first is whole. The others' first piece is their base and the
    // 127 marks before the limit. The marks after it, segmented afresh, are
    // a cluster of their own, cut every 256 bytes, and what's left of them
    // is a unit too.
    assert!(is_boundary(longest) && is_boundary(longest + MAX_CLUSTER_BYTES - 1));
    assert!(!(longest + 1..longest + MAX_CLUSTER_BYTES - 1).any(is_boundary));
    assert!(is_boundary(short + 255) && is_boundary(short + 301));
    assert!(!(short + 1..short + 255).any(is_boundary));
    let pieces = (0..24).map(|piece| long + 255 + piece * 256);
    assert!(pieces.clone().all(is_boundary));
    assert!(is_boundary(long + 1 + 3 * CHUNK));
    let x_of = |offset| units.iter().find(|u| u.0 == offset).expect("a unit").1;
    assert_eq!(x_of(long + 255) - x_of(long), 128. * f64::from(CELL));

    let measure = &mut Clusters::default();
    let mut index = RowIndex::new(&text, 0..row.len(), 4, measure);
    for pair in units.windows(2) {
        let [(offset, x), (next, next_x)] = [pair[0], pair[1]];
        assert_eq!(index.x_for_offset(offset, measure), x, "x of {offset}");
        assert_eq!(index.offset_for_x(x, measure), offset);
        assert_eq!(index.offset_for_x(next_x.next_down(), measure), offset);
        assert_eq!(index.next_boundary(offset, measure), next);
        // Inside a unit, offsets clamp to its start.
        for inside in (offset + 1..next).filter(|&at| text.is_char_boundary(at)) {
            assert_eq!(index.x_for_offset(inside, measure), x, "{inside} clamps");
        }
    }
    let width = units.last().expect("the row's end").1;
    assert_eq!(index.width(measure), width);
    // Only clusters up to the limit are measured whole.
    assert_eq!(measure.longest, MAX_CLUSTER_BYTES - 1);

    // Slices over the long one end on unit boundaries.
    for x in [x_of(long) + 2., x_of(long + 255 + 2 * 256) + 200.] {
        let slice = index.slice(x..x + 6., 300., measure);
        assert!(is_boundary(slice.range.start) && is_boundary(slice.range.end));
        assert!(slice.range.len() <= 4 * 256, "{slice:?}");
    }
}

/// `a`s 10 px wide, and `z`s measured as nothing.
struct ZeroZ;

impl CharWidth for ZeroZ {
    fn char_width(&mut self, ch: char) -> f32 {
        if ch == 'z' { 0. } else { 10. }
    }
}

/// Draws every line 50 px wide, recording the length of each text shaped.
#[derive(Default)]
struct Flat(Vec<usize>);

struct FlatLine;

impl MeasuredLine for FlatLine {
    fn width(&self) -> f32 {
        50.
    }

    fn x_for_index(&self, _: usize) -> f32 {
        0.
    }

    fn index_for_x(&self, _: f32) -> usize {
        0
    }
}

impl TextMeasure for Flat {
    type Style = ();
    type Line = FlatLine;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> FlatLine {
        self.0.push(text.len());
        FlatLine
    }
}

#[test]
fn widening_to_the_caps_x_stops_at_the_cap() {
    // 6125 `a`s are 61250 px. The cap falls among the `z`s after them, so
    // the slice cap's x is 61250 too, as is every `z` boundary after it.
    let row = format!("{}{}", "a".repeat(6125), "z".repeat(1 << 20));
    let measure = &mut ZeroZ;
    let text = Text::new(&row);
    let mut index = RowIndex::new(&text, 0..row.len(), 4, measure);
    let slice = index.slice(0.0..930., 320., measure);
    assert_eq!(slice.range, 0..125);
    assert_eq!(index.slice_limit(0, measure), MAX_SHAPED_ROW_BYTES);
    // Exactly to the cap's x, and past it.
    for by in [60_000., 60_000.5, 1e9] {
        let widened = index.widen(&slice, by, measure);
        assert_eq!(widened.range, 0..MAX_SHAPED_ROW_BYTES, "by {by}");
    }

    // Laid out 1017.5 px wide (930 right of the gutter, with a 320 px margin
    // each side), the slice of 125 `a`s is shaped 50 px wide. The first
    // widening is by twice the 1200 px shortfall times 1250 / 50: exactly to
    // the cap's x.
    let display = DisplayMap::new(Buffer::new(&row).current(), 4);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: 0,
        scroll: Vec2::default(),
        autoscroll: false,
        width: 1017.5,
        height: 20.,
        line_height: 20.,
        em: CELL,
        styles: FrameStyles {
            text: (),
            line_number: (),
            active_line_number: (),
        },
    };
    let shaper = &mut Flat::default();
    let frame = layout_frame(
        &input,
        shaper,
        &mut LongRows::default(),
        measure,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    assert_eq!(frame.rows[0].shown(), 0..MAX_SHAPED_ROW_BYTES);
    // The row's slice and its widening, then the line number.
    assert_eq!(shaper.0, [125, MAX_SHAPED_ROW_BYTES, 1]);
}
