//! Shaping a long row stays bounded however its text is clustered or drawn:
//! no slice of it, and no cluster measured whole, is longer than
//! `MAX_SHAPED_ROW_BYTES`, even for one huge grapheme cluster or a shaper
//! that draws text far narrower than the index measures it.

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{
    CharWidth, DisplayMap, LongRows, RowIndex,
    layout::{
        FrameInput, FrameStyles, MAX_SHAPED_ROW_BYTES, MeasuredLine, StyleRun, TextMeasure, Vec2,
        layout_frame,
    },
};
use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

/// The most a shape or a cluster measured whole may take: the cap, plus a
/// char's slack for cutting at a char boundary.
const MOST: usize = MAX_SHAPED_ROW_BYTES + 4;
const CELL: f32 = 10.;
const FAMILY: &str = "👨\u{200d}👩\u{200d}👧\u{200d}👦";

/// Every char a cell wide, recording the length of each cluster measured
/// whole.
#[derive(Default)]
struct Widths {
    clusters: Vec<usize>,
}

impl CharWidth for Widths {
    fn char_width(&mut self, _: char) -> f32 {
        CELL
    }

    fn cluster_width(&mut self, cluster: &str) -> f32 {
        self.clusters.push(cluster.len());
        CELL
    }
}

/// A line drawn cluster by cluster: the x after each cluster, by its end.
struct Line {
    ends: Vec<(usize, f32)>,
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.ends.last().map_or(0., |&(_, x)| x)
    }

    fn x_for_index(&self, index: usize) -> f32 {
        let before = self.ends.partition_point(|&(end, _)| end <= index);
        before.checked_sub(1).map_or(0., |ix| self.ends[ix].1)
    }

    fn index_for_x(&self, x: f32) -> usize {
        let before = self.ends.partition_point(|&(_, end_x)| end_x <= x);
        before.checked_sub(1).map_or(0, |ix| self.ends[ix].0)
    }
}

/// Draws a single char `scale` cells wide and a multi-char cluster
/// `cluster_scale` cells, recording the length of each text shaped.
struct Shaper {
    scale: f32,
    cluster_scale: f32,
    shaped: Vec<usize>,
}

impl Shaper {
    fn new(scale: f32, cluster_scale: f32) -> Self {
        Self {
            scale,
            cluster_scale,
            shaped: Vec::new(),
        }
    }

    /// The lengths of the row's shapes: line numbers are shaped too, in a
    /// few bytes each.
    fn row_shapes(&self) -> Vec<usize> {
        self.shaped.iter().copied().filter(|&len| len > 8).collect()
    }
}

impl TextMeasure for Shaper {
    type Style = ();
    type Line = Line;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> Line {
        self.shaped.push(text.len());
        let mut x = 0.;
        let ends = text
            .grapheme_indices(true)
            .map(|(ix, cluster)| {
                let scale = if cluster.chars().nth(1).is_some() {
                    self.cluster_scale
                } else {
                    self.scale
                };
                x += CELL * scale;
                (ix + cluster.len(), x)
            })
            .collect();
        Line { ends }
    }
}

/// An editor's display and long rows, kept from frame to frame.
struct Editor {
    display: DisplayMap,
    long_rows: LongRows,
    widths: Widths,
}

impl Editor {
    fn new(text: &str) -> Self {
        Self {
            display: DisplayMap::new(Buffer::new(text).current(), 4),
            long_rows: LongRows::default(),
            widths: Widths::default(),
        }
    }

    /// Lays out a frame 1000 wide scrolled to `scroll_x`, returning the
    /// scroll it clamped to and what's shown of the first row.
    fn lay_out(&mut self, scroll_x: f32, shaper: &mut Shaper) -> (f32, Range<usize>) {
        let selections = [ResolvedSelection {
            range: 0..0,
            reversed: false,
        }];
        let input = FrameInput {
            display: &self.display,
            tab_size: 4,
            selections: &selections,
            primary_head: 0,
            scroll: Vec2 { x: scroll_x, y: 0. },
            autoscroll: false,
            width: 1000.,
            height: 100.,
            line_height: 20.,
            em: CELL,
            styles: FrameStyles {
                text: (),
                line_number: (),
                active_line_number: (),
            },
        };
        let frame = layout_frame(
            &input,
            shaper,
            &mut self.long_rows,
            &mut self.widths,
            |_| Vec::new(),
            |_| Vec::new(),
        );
        (frame.scroll.x, frame.rows[0].shown())
    }
}

fn assert_bounded(lengths: &[usize], context: &str) {
    let longest = lengths.iter().copied().max().unwrap_or(0);
    assert!(longest <= MOST, "{longest} bytes at once: {context}");
}

#[test]
fn huge_cluster_is_never_shaped_whole() {
    let rows = [
        format!("a{}", "\u{301}".repeat(1 << 20)),
        format!("{}👩", "👨\u{200d}".repeat(256 << 10)),
    ];
    for row in &rows {
        assert_eq!(row.graphemes(true).count(), 1);
        let mut editor = Editor::new(row);
        // The row's start, middle and end.
        for scroll_x in [0., 1e6, f32::MAX] {
            let shaper = &mut Shaper::new(1., 1.);
            let (scroll_x, shown) = editor.lay_out(scroll_x, shaper);
            let context = format!("shaped {} bytes at {scroll_x}: {shown:?}", row.len());
            assert_bounded(&shaper.shaped, &context);
            assert!(shown.len() <= MOST, "{context}");
        }
        assert_bounded(&editor.widths.clusters, "laying out, measured whole");

        // Slices over the cluster, from a fresh index.
        let text = Text::new(row);
        let widths = &mut Widths::default();
        let mut index = RowIndex::new(&text, 0..row.len(), 4, widths);
        let width = index.width(widths);
        for x in [0., 5000., width / 2., width - 800., width] {
            let slice = index.slice(x..x + 800., 320., widths);
            assert!(slice.range.len() <= MOST, "at {x}: {slice:?}");
        }
        assert_bounded(&widths.clusters, "slicing, measured whole");
    }
}

#[test]
fn widening_never_shapes_past_the_cap() {
    let row = format!("{}{}", "a".repeat(500), FAMILY.repeat(400_000));
    let mut editor = Editor::new(&row);
    // ASCII drawn at half the width the index measures, families at a
    // tenth; and everything drawn as nothing.
    for (scale, cluster_scale) in [(0.5, 0.1), (0., 0.)] {
        for scroll_x in [0., 5e5, 1e6, f32::MAX] {
            let shaper = &mut Shaper::new(scale, cluster_scale);
            let (scroll_x, shown) = editor.lay_out(scroll_x, shaper);
            let row_shapes = shaper.row_shapes();
            let context =
                format!("{row_shapes:?}, {scale}/{cluster_scale} at {scroll_x}: {shown:?}");
            assert!((1..=3).contains(&row_shapes.len()), "{context}");
            assert_bounded(&row_shapes, &context);
        }
    }
    assert_bounded(&editor.widths.clusters, "measured whole");
}
