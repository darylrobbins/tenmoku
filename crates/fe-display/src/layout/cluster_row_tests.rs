//! Long rows with grapheme clusters that a shaper draws as units: ZWJ emoji
//! sequences and combining marks. A fake measurer and shaper stand in for a
//! real one: a multi-char cluster is as wide as its first char, so a ZWJ
//! family is 2 cells though its chars sum to 11, and `e` with a combining
//! acute accent is 1 cell, not 2.

use fe_buffer::Buffer;
use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

use super::{
    tests::{CELL, GUTTER, LINE, WIDTH, advance},
    *,
};
use crate::long_row::CHUNK;

const TEXT_WIDTH: f32 = WIDTH - GUTTER;
pub(super) const FAMILY: &str = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
pub(super) const E_ACUTE: &str = "e\u{301}";

pub(super) fn cluster_advance(cluster: &str) -> f32 {
    advance(cluster.chars().next().expect("not empty"))
}

/// Measures clusters whole, counting calls.
#[derive(Default)]
pub(super) struct Clusters {
    chars: usize,
    clusters: usize,
    /// The longest cluster measured.
    pub(super) longest: usize,
}

impl CharWidth for Clusters {
    fn char_width(&mut self, ch: char) -> f32 {
        self.chars += 1;
        advance(ch)
    }

    fn cluster_width(&mut self, cluster: &str) -> f32 {
        self.clusters += 1;
        self.longest = self.longest.max(cluster.len());
        cluster_advance(cluster)
    }
}

/// A line shaped cluster by cluster, each `scale` times as wide as
/// [`Clusters`] measures it.
#[derive(Clone, Debug)]
struct ClusterLine {
    /// The x after each cluster, by its end.
    ends: Vec<(usize, f32)>,
}

impl ClusterLine {
    fn boundaries(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        [(0, 0.)].into_iter().chain(self.ends.iter().copied())
    }
}

impl MeasuredLine for ClusterLine {
    fn width(&self) -> f32 {
        self.ends.last().map_or(0., |&(_, x)| x)
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.boundaries()
            .take_while(|&(end, _)| end <= index)
            .last()
            .map_or(0., |(_, x)| x)
    }

    fn index_for_x(&self, x: f32) -> usize {
        self.boundaries()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |(ix, _)| ix)
    }
}

/// Shapes by [`Clusters`]' rule, scaled, recording the length of each text
/// shaped.
struct Shaper {
    scale: f32,
    /// The scale of multi-char clusters.
    cluster_scale: f32,
    shaped: Vec<usize>,
}

impl Shaper {
    fn new(scale: f32) -> Self {
        Self {
            scale,
            cluster_scale: scale,
            shaped: Vec::new(),
        }
    }
}

impl TextMeasure for Shaper {
    type Style = &'static str;
    type Line = ClusterLine;

    fn shape(&mut self, text: &str, _: &[StyleRun<&'static str>]) -> ClusterLine {
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
                x += cluster_advance(cluster) * scale;
                (ix + cluster.len(), x)
            })
            .collect();
        ClusterLine { ends }
    }
}

struct Editor {
    display: DisplayMap,
    long_rows: LongRows,
    widths: Clusters,
    width: f32,
}

impl Editor {
    fn new(text: &str) -> Self {
        Self {
            display: DisplayMap::new(Buffer::new(text).current(), 4),
            long_rows: LongRows::default(),
            widths: Clusters::default(),
            width: WIDTH,
        }
    }

    /// Lays out a frame with a caret at `head`, recording highlight queries.
    fn lay_out(
        &mut self,
        head: usize,
        scroll_x: f32,
        autoscroll: bool,
        shaper: &mut Shaper,
        queries: &mut Vec<Range<usize>>,
    ) -> Frame<ClusterLine> {
        let selections = [ResolvedSelection {
            range: head..head,
            reversed: false,
        }];
        let input = FrameInput {
            display: &self.display,
            tab_size: 4,
            selections: &selections,
            primary_head: head,
            scroll: Vec2 { x: scroll_x, y: 0. },
            autoscroll,
            width: self.width,
            height: 120.,
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
            shaper,
            &mut self.long_rows,
            &mut self.widths,
            |range| {
                queries.push(range);
                Vec::new()
            },
            |_| Vec::new(),
        )
    }
}

/// The shaped part of `row`, placed at its slice's x.
fn shaped(row: &RowLayout<ClusterLine>) -> Range<f64> {
    row.start_x()..row.start_x() + f64::from(row.line.width())
}

#[test]
fn index_measures_clusters_as_units() {
    // A cluster straddles the first 4 KB mark, then one straddles the mark
    // 4 KB after the checkpoint past it. Families fill the row up to them,
    // keeping the boundaries to check few.
    let mix = format!("ab{FAMILY}c{E_ACUTE}de").repeat(20);
    let fill = |row: &mut String, to: usize| {
        while row.len() + FAMILY.len() <= to {
            row.push_str(FAMILY);
        }
        while row.len() < to {
            row.push('x');
        }
    };
    let mut row = mix.clone();
    fill(&mut row, CHUNK - 6);
    row += FAMILY;
    let mark = row.len() + CHUNK;
    row += &mix;
    fill(&mut row, mark - 1);
    row += E_ACUTE;
    row += &mix;
    assert!(row.len() > MAX_SHAPED_ROW_BYTES);

    let text = Text::new(&format!("before\n{row}\nafter"));
    let start = 7;
    // (offset, x) of every cluster boundary, measuring the prefix.
    let mut boundaries = vec![(start, 0.)];
    let mut x = 0.;
    for (ix, cluster) in row.grapheme_indices(true) {
        x += f64::from(cluster_advance(cluster));
        boundaries.push((start + ix + cluster.len(), x));
    }
    let is_boundary = |offset| boundaries.iter().any(|&(at, _)| at == offset);
    assert!(!is_boundary(start + CHUNK) && !is_boundary(start + mark));

    let measure = &mut Clusters::default();
    let mut index = RowIndex::new(&text, start..start + row.len(), 4, measure);
    for pair in boundaries.windows(2) {
        let [(offset, x), (next, next_x)] = [pair[0], pair[1]];
        assert_eq!(index.x_for_offset(offset, measure), x, "x of {offset}");
        for inside in (offset + 1..next).filter(|&at| text.is_char_boundary(at)) {
            assert_eq!(index.x_for_offset(inside, measure), x, "{inside} clamps");
        }
        assert_eq!(index.offset_for_x(x, measure), offset);
        assert_eq!(index.offset_for_x((x + next_x) / 2., measure), offset);
        assert_eq!(index.offset_for_x(next_x.next_down(), measure), offset);
    }
    assert_eq!(index.width(measure), x);

    let view = f64::from(TEXT_WIDTH);
    for left in (0..(x as usize)).step_by(997) {
        let left = left as f64;
        let slice = index.slice(left..left + view, 200., measure);
        assert!(is_boundary(slice.range.start), "{slice:?}");
        assert!(is_boundary(slice.range.end), "{slice:?}");
        assert!(slice.start_x <= (left - 200.).max(0.), "{slice:?}");
        let end_x = index.x_for_offset(slice.range.end, measure);
        assert!(end_x >= left + view + 200. || end_x == x, "{slice:?}");
    }
}

#[test]
fn emoji_row_slice_covers_the_view() {
    let row = FAMILY.repeat(2000);
    let mut editor = Editor::new(&row);
    let row_width = 2000. * 2. * CELL;
    let shaper = &mut Shaper::new(1.);
    for scroll_x in [row_width / 2., f32::MAX] {
        let frame = editor.lay_out(0, scroll_x, false, shaper, &mut Vec::new());
        let row = &frame.rows[0];
        assert!(row.slice.is_some());
        let view = f64::from(frame.scroll.x)..f64::from(frame.scroll.x + TEXT_WIDTH);
        let shaped = shaped(row);
        assert!(shaped.start <= view.start, "{shaped:?} covers {view:?}");
        assert!(
            shaped.end >= view.end.min(f64::from(row_width)),
            "{shaped:?} covers {view:?}"
        );
    }

    // Moving to the row's end scrolls its caret into the text area.
    let end = row.len();
    let frame = editor.lay_out(end, 0., true, shaper, &mut Vec::new());
    let caret = frame
        .caret_bounds(end, &mut editor.long_rows, &mut Clusters::default())
        .expect("on screen");
    assert!((GUTTER..=WIDTH).contains(&caret.x), "{caret:?}");
    assert_eq!(frame.cursors[0], caret);
    assert_eq!(caret.x, frame.view_x(f64::from(row_width)));
}

#[test]
fn short_slice_is_widened() {
    let row = format!("abc{FAMILY}d{E_ACUTE}").repeat(1000);
    let mut editor = Editor::new(&row);
    let row_width = 1000. * 7. * CELL;
    // The shaper draws everything a half to a tenth as wide as the index
    // measures it, in a narrow and a wide editor. (A tenth in the wide one
    // needs a slice longer than `MAX_SHAPED_ROW_BYTES`, which is capped:
    // `slices_cover_the_view_however_narrowing_changes` checks that.)
    for (scale, width) in [(0.5, WIDTH), (0.25, WIDTH), (0.5, 4070.), (0.1, WIDTH)] {
        editor.width = width;
        let text_width = width - GUTTER;
        for scroll_x in [row_width / 2., row_width - 3. * text_width, f32::MAX] {
            let shaper = &mut Shaper::new(scale);
            let mut queries = Vec::new();
            let frame = editor.lay_out(0, scroll_x, false, shaper, &mut queries);
            let row = &frame.rows[0];
            let view_end = f64::from(frame.scroll.x + text_width);
            let shaped = shaped(row);
            let context = format!("{shaped:?} reaches {view_end}, {scale} at {scroll_x}");
            assert!(
                shaped.end >= view_end || row.shown().end == row.end,
                "{context}"
            );
            // Line numbers are shaped too, in one shape each.
            let row_shapes = shaper.shaped.iter().filter(|&&len| len > 8).count();
            assert!(
                (1..=3).contains(&row_shapes),
                "{row_shapes} shapes: {context}"
            );
            if scroll_x == row_width / 2. {
                assert!(row_shapes > 1, "the slice was widened: {context}");
            }
            // Narrowing this even is covered, margin and all, by the first
            // widening: it never takes a third shape.
            let margin = f64::from(SLICE_MARGIN_EMS * CELL);
            assert!(row_shapes <= 2, "{row_shapes} shapes: {context}");
            assert!(
                shaped.end >= view_end + margin || row.shown().end == row.end,
                "{context}"
            );
            // The last highlight query covers what was shaped.
            assert_eq!(queries.len(), row_shapes);
            assert_eq!(queries.last(), Some(&row.shown()));
        }
    }
}

#[test]
fn clicks_outside_the_slice_land_on_cluster_boundaries() {
    let row = FAMILY.repeat(2000);
    let mut editor = Editor::new(&row);
    let shaper = &mut Shaper::new(1.);
    let frame = editor.lay_out(0, 20_000., false, shaper, &mut Vec::new());
    let shown = frame.rows[0].shown();
    let widths = &mut Clusters::default();
    // Families far left and far right of the slice, each 2 cells wide.
    for family in [100, shown.end / FAMILY.len() + 100] {
        let start = family * FAMILY.len();
        let x = frame.view_x(family as f64 * 2. * f64::from(CELL));
        for (into, want) in [(0., start), (8., start), (12., start + FAMILY.len())] {
            let position = Vec2 {
                x: x + into,
                y: LINE / 2.,
            };
            let text = editor.display.text();
            let offset = frame.offset_at(text, position, &mut editor.long_rows, widths);
            assert_eq!(offset, want, "{into} px into family {family}");
        }
    }
}

#[test]
fn clicks_past_a_narrowed_row_land_on_its_end() {
    // Drawn narrower than indexed, a slice that runs to the row's end stops
    // short of the view, scrolled to the row's end. A click past its text is
    // past the row. At the row's start, the slice is as long as a slice may
    // be and still stops short, so a click past its text lands where the
    // index puts it: 50 px past 500 `a`s and 307 families shaped 3114 px
    // wide.
    let row = format!("{}{}", "a".repeat(500), FAMILY.repeat(400));
    let mut editor = Editor::new(&row);
    editor.width = 4070.;
    for (scroll_x, end, click) in [(f32::MAX, row.len(), row.len()), (0., 8175, 316)] {
        let shaper = &mut Shaper {
            cluster_scale: 0.1,
            ..Shaper::new(0.5)
        };
        let frame = editor.lay_out(0, scroll_x, false, shaper, &mut Vec::new());
        assert_eq!(frame.rows[0].shown().end, end);
        let position = Vec2 {
            x: frame.view_x(shaped(&frame.rows[0]).end) + 50.,
            y: LINE / 2.,
        };
        assert!(position.x < 4070., "{position:?}");
        let text = editor.display.text();
        let widths = &mut Clusters::default();
        let offset = frame.offset_at(text, position, &mut editor.long_rows, widths);
        assert_eq!(offset, click, "at {scroll_x}");
    }
}

#[test]
fn ascii_rows_skip_cluster_measuring() {
    let row = "x = [1, 2, 3];".repeat(1 << 16);
    let text = Text::new(&row);
    // Whether printable ASCII is all one width or not, the first view scans
    // little, measuring the whole row scans each byte once, and a view after
    // that scans a few chunks more.
    let check_scans = |measure: &mut dyn CharWidth| {
        let mut index = RowIndex::new(&text, 0..row.len(), 4, measure);
        index.slice(0.0..800., 200., measure);
        assert!(index.scanned() <= 4 * CHUNK, "{} scanned", index.scanned());
        let mut index = RowIndex::new(&text, 0..row.len(), 4, measure);
        assert_eq!(index.width(measure), row.len() as f64 * f64::from(CELL));
        assert_eq!(index.scanned(), row.len());
        index.slice(50_000.0..50_800., 200., measure);
        let scanned = index.scanned();
        assert!(scanned <= row.len() + 3 * CHUNK, "{scanned} scanned");
    };
    let measure = &mut Clusters::default();
    check_scans(measure);
    assert_eq!((measure.chars, measure.clusters), (2 * 128, 0));
    // Proportional ASCII is measured per char, but never as clusters.
    let measure = &mut Proportional::default();
    check_scans(measure);
    assert_eq!(measure.0.clusters, 0);

    // Nor are they when laid out.
    let mut editor = Editor::new(&row);
    for (head, scroll_x) in [(0, 0.), (0, 10_000.), (row.len(), 0.)] {
        editor.lay_out(head, scroll_x, true, &mut Shaper::new(1.), &mut Vec::new());
    }
    assert_eq!((editor.widths.chars, editor.widths.clusters), (128, 0));
}

/// [`Clusters`] with a narrow `i`.
#[derive(Default)]
struct Proportional(Clusters);

impl CharWidth for Proportional {
    fn char_width(&mut self, ch: char) -> f32 {
        if ch == 'i' { 3. } else { self.0.char_width(ch) }
    }

    fn cluster_width(&mut self, cluster: &str) -> f32 {
        self.0.cluster_width(cluster)
    }
}

#[test]
fn slices_cover_the_view_however_narrowing_changes() {
    // ASCII shaped at a half, then families, from inside the first
    // widening, which is estimated from the ASCII.
    let row = format!("{}{}", "a".repeat(500), FAMILY.repeat(2000));
    let mut editor = Editor::new(&row);
    editor.width = 4070.;
    let mut lay_out = |scale, cluster_scale| {
        let shaper = &mut Shaper {
            cluster_scale,
            ..Shaper::new(scale)
        };
        let frame = editor.lay_out(0, 0., false, shaper, &mut Vec::new());
        let row_shapes = shaper.shaped.iter().filter(|&&len| len > 8).count();
        (
            row_shapes,
            frame.rows[0].shown(),
            shaped(&frame.rows[0]).end,
        )
    };
    // Families drawn at over half the ASCII's ratio, at three tenths, are
    // covered by the first widening, margin and all.
    let (row_shapes, shown, end) = lay_out(0.5, 0.3);
    assert_eq!(row_shapes, 2);
    assert!(
        shown.end < row.len() && end >= 4000. + 320.,
        "{shown:?} to {end}"
    );
    // At a tenth, the first widening is as long as a slice may be, the
    // `a`s and the families that fit in `MAX_SHAPED_ROW_BYTES`, and still
    // ends short of the view, whose rest is left blank.
    let capped = 0..500 + (MAX_SHAPED_ROW_BYTES - 500) / FAMILY.len() * FAMILY.len();
    let (row_shapes, shown, end) = lay_out(0.5, 0.1);
    assert_eq!((row_shapes, shown), (2, capped.clone()));
    assert!(end < 4000.);
    // A slice shaped to nothing gives no ratio to widen by: the first
    // widening is as long as a slice may be.
    let (row_shapes, shown, _) = lay_out(0., 0.);
    assert_eq!((row_shapes, shown), (2, capped));

    // Rows whose narrowing changes once, twice or all along them, with
    // ASCII or clusters the narrower, down to clusters drawn as nothing.
    // Each is scrolled to put changes near its start and far along it in
    // the view, at its right edge, in its margin, and past it, where the
    // first widening reaches or doesn't. A slice covers the view, or it
    // reaches the row's end or is as long as a slice may be.
    let rows = [
        format!("{}{}", "a".repeat(1500), FAMILY.repeat(2000)),
        format!(
            "{}{}{}",
            FAMILY.repeat(300),
            "a".repeat(600),
            FAMILY.repeat(2000)
        ),
        format!(
            "{}{}{}{}",
            "a".repeat(400),
            FAMILY.repeat(100),
            "a".repeat(400),
            E_ACUTE.repeat(3000)
        ),
        format!("{}{}", "a".repeat(37), FAMILY.repeat(23)).repeat(60),
    ];
    let text_width = 4000.;
    for row in &rows {
        let mut editor = Editor::new(row);
        editor.width = 4070.;
        let boundaries: Vec<usize> = row
            .grapheme_indices(true)
            .map(|(ix, _)| ix)
            .chain([row.len()])
            .collect();
        // The x of each change between single chars and clusters.
        let mut changes = Vec::new();
        let mut x = 0.;
        let mut was_cluster = None;
        for cluster in row.graphemes(true) {
            let is_cluster = cluster.chars().nth(1).is_some();
            if was_cluster.is_some_and(|was| was != is_cluster) {
                changes.push(x);
            }
            was_cluster = Some(is_cluster);
            x += f64::from(cluster_advance(cluster));
        }
        // The first changes, and the first three views or more along.
        let far = changes.partition_point(|&change| change < 3. * text_width);
        let mut scrolls: Vec<f64> = changes[..far.min(3)]
            .iter()
            .chain(changes[far..].iter().take(3))
            .flat_map(|change| [0.5, 1., 1.05, 1.5, 3.].map(|views| change - views * text_width))
            .filter(|&scroll_x| scroll_x > 0.)
            .chain([0., f64::from(f32::MAX)])
            .collect();
        scrolls.sort_by(f64::total_cmp);
        scrolls.dedup();
        for (scale, cluster_scale) in [(0.5, 0.1), (1., 0.), (0.1, 1.)] {
            for &scroll_x in &scrolls {
                let shaper = &mut Shaper {
                    cluster_scale,
                    ..Shaper::new(scale)
                };
                let mut queries = Vec::new();
                let frame = editor.lay_out(0, scroll_x as f32, false, shaper, &mut queries);
                let row = &frame.rows[0];
                let view_end = f64::from(frame.scroll.x) + text_width;
                let shaped = shaped(row);
                let shown = row.shown();
                let context = format!(
                    "{shaped:?} ({shown:?}) reaches {view_end}, {scale}/{cluster_scale} at {scroll_x}"
                );
                assert!(shown.len() <= MAX_SHAPED_ROW_BYTES, "{context}");
                if shaped.end < view_end && shown.end < row.end {
                    // The last cluster boundary within the cap.
                    let cap = shown.start + MAX_SHAPED_ROW_BYTES;
                    let limit = boundaries[boundaries.partition_point(|&at| at <= cap) - 1];
                    assert_eq!(shown.end, limit, "{context}");
                }
                let row_shapes = shaper.shaped.iter().filter(|&&len| len > 8).count();
                assert!(
                    (1..=3).contains(&row_shapes),
                    "{row_shapes} shapes: {context}"
                );
                assert_eq!(queries.len(), row_shapes);
                assert_eq!(queries.last(), Some(&row.shown()));
            }
        }
    }
}

/// Proportional ASCII, with each multi-char cluster narrower than its first
/// char.
struct NarrowClusters;

impl CharWidth for NarrowClusters {
    fn char_width(&mut self, ch: char) -> f32 {
        if ch == 'i' { 3. } else { advance(ch) }
    }

    fn cluster_width(&mut self, _: &str) -> f32 {
        2.
    }
}

#[test]
fn proportional_ascii_finds_narrower_clusters() {
    // Combining marks on narrow and wide ASCII, alone, in runs and among
    // plain ASCII.
    let mut row = E_ACUTE.repeat(3000);
    let pieces = format!("ab{E_ACUTE}ci\u{301}i{E_ACUTE}{E_ACUTE}de");
    while row.len() < 3 * CHUNK {
        row += &pieces;
    }
    let text = Text::new(&row);
    let measure = &mut NarrowClusters;
    let mut boundaries = vec![(0, 0.)];
    let mut x = 0.;
    for (ix, cluster) in row.grapheme_indices(true) {
        x += f64::from(if cluster.chars().nth(1).is_some() {
            measure.cluster_width(cluster)
        } else {
            measure.char_width(cluster.chars().next().expect("not empty"))
        });
        boundaries.push((ix + cluster.len(), x));
    }

    let mut index = RowIndex::new(&text, 0..row.len(), 4, measure);
    // A query inside the first cluster finds its start, not the char past
    // it.
    assert_eq!(index.offset_for_x(2., measure), 3);
    assert_eq!(index.slice(0.0..5., 0., measure).range, 0..9);
    // The first view scans little.
    index.slice(0.0..800., 200., measure);
    assert!(index.scanned() <= 4 * CHUNK, "{} scanned", index.scanned());

    for pair in boundaries.windows(2) {
        let [(offset, x), (next, next_x)] = [pair[0], pair[1]];
        assert_eq!(index.x_for_offset(offset, measure), x, "x of {offset}");
        assert_eq!(index.offset_for_x(x, measure), offset);
        assert_eq!(index.offset_for_x((x + next_x) / 2., measure), offset);
        assert_eq!(index.offset_for_x(next_x.next_down(), measure), offset);
        assert_eq!(index.next_boundary(offset, measure), next);
    }
    assert_eq!(index.width(measure), x);

    let is_boundary = |offset| boundaries.iter().any(|&(at, _)| at == offset);
    for left in (0..(x as usize)).step_by(97) {
        let left = left as f64;
        for (view, margin) in [(5., 0.), (f64::from(TEXT_WIDTH), 200.)] {
            let slice = index.slice(left..left + view, margin, measure);
            assert!(is_boundary(slice.range.start), "{slice:?}");
            assert!(is_boundary(slice.range.end), "{slice:?}");
            assert!(slice.start_x <= (left - margin).max(0.), "{slice:?}");
            let end_x = index.x_for_offset(slice.range.end, measure);
            assert!(end_x >= left + view + margin || end_x == x, "{slice:?}");
        }
    }
}
