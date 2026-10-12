//! Properties of the block layer: display rows sit below the block rows
//! anchored at or above them and are otherwise laid out as without blocks,
//! and every offset shown, and every point on a block, goes where hit
//! testing says.
//!
//! Documents are a few rows of ASCII, multi-byte chars and tabs, sometimes
//! with a row past [`WRAP_SLICE_BYTES`], soft wrapped or not, sometimes
//! folded, scrolled both ways. Cells are 10 px and rows 20 px.

use std::fmt;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{
    Block, BlockMap, CharWidth, DisplayMap, LongRows, ScreenRow, WRAP_SLICE_BYTES,
    layout::{
        BlockLayout, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, RowLayout, StyleRun,
        TextMeasure, Vec2, layout_frame, layout_frame_with_blocks,
    },
};
use proptest::{prelude::*, sample::Index};

const CELL: f32 = 10.;
const LINE: f32 = 20.;
const WIDTH: f32 = 400.;

/// A line of chars a cell wide each, with where each char starts, so that
/// measuring every offset of a long row's slice stays cheap.
#[derive(Clone)]
struct Line {
    text: String,
    starts: Vec<usize>,
}

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.text.fmt(f)
    }
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.starts.len() as f32 * CELL
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.starts.partition_point(|&start| start < index) as f32 * CELL
    }

    fn index_for_x(&self, x: f32) -> usize {
        let column = (x / CELL).round().max(0.) as usize;
        self.starts.get(column).copied().unwrap_or(self.text.len())
    }
}

/// Every char a cell wide.
struct Mono;

impl CharWidth for Mono {
    fn char_width(&mut self, _: char) -> f32 {
        CELL
    }
}

impl TextMeasure for Mono {
    type Style = ();
    type Line = Line;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> Line {
        Line {
            text: text.to_string(),
            starts: text.char_indices().map(|(ix, _)| ix).collect(),
        }
    }
}

/// ASCII, a space and a tab, and chars of two, three and four bytes; each
/// its own grapheme, so a char is a cell.
fn any_char() -> impl Strategy<Value = char> {
    prop_oneof![
        8 => prop::char::range('a', 'z'),
        2 => Just(' '),
        2 => Just('\t'),
        1 => Just('é'),
        1 => Just('λ'),
        1 => Just('中'),
        1 => Just('😀'),
    ]
}

/// What a frame is laid out from, kept small to print: the long row is
/// built from `long`'s unit.
#[derive(Clone, Debug)]
struct Doc {
    rows: Vec<String>,
    /// A row replaced by its unit repeated past [`WRAP_SLICE_BYTES`].
    long: Option<(Index, String)>,
    /// Soft wrap at this many cells.
    wrap: Option<u32>,
    /// Buffer rows from the index, folded with the next this many.
    folds: Vec<(Index, u32)>,
    caret: Index,
    autoscroll: bool,
    /// Fractions of how far each way it can scroll.
    scroll: (f64, f64),
    /// In rows.
    height: f32,
}

fn doc() -> impl Strategy<Value = Doc> {
    let row = prop::collection::vec(any_char(), 0..48).prop_map(String::from_iter);
    let unit = prop::collection::vec(any_char(), 1..8).prop_map(String::from_iter);
    (
        prop::collection::vec(row, 1..24),
        prop::option::weighted(0.25, (any::<Index>(), unit)),
        prop::option::of(4u32..40),
        prop_oneof![
            Just(Vec::new()),
            prop::collection::vec((any::<Index>(), 1u32..4), 1..3)
        ],
        any::<Index>(),
        any::<bool>(),
        (0.0..1.0, 0.0..1.0),
        (3u32..12, 0u32..4),
    )
        .prop_map(
            |(rows, long, wrap, folds, caret, autoscroll, scroll, (rows_high, quarters))| Doc {
                rows,
                long,
                wrap,
                folds,
                caret,
                autoscroll,
                scroll,
                height: rows_high as f32 + quarters as f32 / 4.,
            },
        )
}

/// A [`Doc`] built: its text, display map and caret.
struct Editor {
    text: String,
    display: DisplayMap,
    selections: Vec<ResolvedSelection>,
    /// The scroll as fractions, and how far across it goes.
    scroll: (f64, f64),
    widest: f64,
    autoscroll: bool,
    height: f32,
}

impl Editor {
    fn new(doc: &Doc) -> Self {
        let mut rows = doc.rows.clone();
        if let Some((row, unit)) = &doc.long {
            let repeat = WRAP_SLICE_BYTES / unit.len() + 1;
            *row.get_mut(&mut rows) = unit.repeat(repeat);
        }
        let text = rows.join("\n");
        let mut display = DisplayMap::new(Buffer::new(&text).current(), 4);
        if let Some(cells) = doc.wrap {
            display.set_wrap_width(Some(cells as f32 * CELL), &mut Mono);
            while display.wrap_pending(0, WRAP_SLICE_BYTES, &mut Mono) {}
        }
        let buffer_rows = rows.len();
        display.fold_many(doc.folds.iter().map(|(start, len)| {
            let start = start.index(buffer_rows) as u32;
            start..=start + len
        }));
        let caret = pick_boundary(&text, doc.caret);
        let widest = rows.iter().map(|row| row.chars().count()).max();
        Self {
            scroll: doc.scroll,
            widest: 1.2 * widest.unwrap_or(0) as f64 * f64::from(CELL),
            selections: vec![ResolvedSelection {
                range: caret..caret,
                reversed: false,
            }],
            autoscroll: doc.autoscroll,
            height: doc.height * LINE,
            display,
            text,
        }
    }

    /// Lays out a frame with `blocks`, scrolled down as far into the rows
    /// and block rows as the doc says, or, given none, with
    /// [`layout_frame`].
    fn lay_out(&self, blocks: Option<&BlockMap>, long_rows: &mut LongRows) -> Frame<Line> {
        let screen_rows = self.display.row_count() + blocks.map_or(0, BlockMap::block_rows);
        let input = FrameInput {
            display: &self.display,
            tab_size: 4,
            selections: &self.selections,
            primary_head: self.selections[0].head(),
            scroll: Vec2 {
                x: quarter_px(self.scroll.0 * self.widest),
                y: quarter_px(self.scroll.1 * f64::from(screen_rows) * f64::from(LINE)),
            },
            autoscroll: self.autoscroll,
            width: WIDTH,
            height: self.height,
            line_height: LINE,
            em: CELL,
            styles: FrameStyles {
                text: (),
                line_number: (),
                active_line_number: (),
            },
        };
        match blocks {
            Some(blocks) => layout_frame_with_blocks(
                &input,
                blocks,
                &mut Mono,
                long_rows,
                &mut Mono,
                |_| Vec::new(),
                |_| Vec::new(),
            ),
            None => layout_frame(
                &input,
                &mut Mono,
                long_rows,
                &mut Mono,
                |_| Vec::new(),
                |_| Vec::new(),
            ),
        }
    }
}

/// Rounds to a quarter pixel, which an `f32` holds exactly at these sizes.
fn quarter_px(x: f64) -> f32 {
    ((x * 4.).round() / 4.) as f32
}

/// The char boundary of `text` at or before byte `index`, counting its end.
fn pick_boundary(text: &str, index: Index) -> usize {
    text.floor_char_boundary(index.index(text.len() + 1))
}

/// The char boundaries in `range` a caret can be at on `row`: not the soft
/// wrap point that ends it, which starts the next row.
fn offsets_on(text: &str, row: &RowLayout<Line>, range: std::ops::Range<usize>) -> Vec<usize> {
    let wrap_point = !row.last && range.end == row.end;
    (range.start..=range.end)
        .filter(|&ix| text.is_char_boundary(ix) && !(wrap_point && ix == range.end))
        .collect()
}

/// Where a block goes.
#[derive(Clone, Debug)]
enum Anchor {
    Anywhere(Index),
    /// This many display rows from the caret's, which autoscroll brings
    /// into view, so that blocks are often in a long document's frame.
    NearCaret(i32),
    /// After the last row.
    End,
}

/// 0 to 5 blocks of `rows` rows each.
fn blocks(rows: std::ops::RangeInclusive<u32>) -> impl Strategy<Value = Vec<(Anchor, u32)>> {
    let anchor = prop_oneof![
        3 => any::<Index>().prop_map(Anchor::Anywhere),
        3 => (-6..6).prop_map(Anchor::NearCaret),
        1 => Just(Anchor::End),
    ];
    prop::collection::vec((anchor, rows), 0..=5)
}

/// The blocks anchored in `editor`'s display rows, with their ids.
fn anchor(editor: &Editor, blocks: &[(Anchor, u32)]) -> Vec<Block> {
    let row_count = editor.display.row_count();
    let caret_row = editor.display.row_for_offset(editor.selections[0].head());
    (0..)
        .zip(blocks)
        .map(|(id, (anchor, rows))| {
            let above = match anchor {
                Anchor::Anywhere(at) => at.index(row_count as usize + 1) as u32,
                Anchor::NearCaret(by) => {
                    (i64::from(caret_row) + i64::from(*by)).clamp(0, i64::from(row_count)) as u32
                }
                Anchor::End => row_count,
            };
            Block {
                above,
                rows: *rows,
                id,
            }
        })
        .collect()
}

/// Each block with the screen row it starts at, in screen order: blocks
/// above the same row in the order given, each above the rows of the ones
/// after it.
fn block_starts(blocks: &[Block]) -> Vec<(u32, Block)> {
    let mut sorted = blocks.to_vec();
    sorted.sort_by_key(|block| block.above);
    let mut before = 0;
    sorted
        .into_iter()
        .map(|block| {
            let start = block.above + before;
            before += block.rows;
            (start, block)
        })
        .collect()
}

/// Where a click on a block above display row `above` goes: the end of the
/// row above it (before a soft wrap, that's before the row's last char), or
/// the start of the text.
fn block_offset(editor: &Editor, above: u32) -> usize {
    let Some(row) = above.checked_sub(1) else {
        return 0;
    };
    let row = editor.display.row(row);
    if row.last {
        return row.range.end;
    }
    let mut end = row.range.end - 1;
    while !editor.text.is_char_boundary(end) {
        end -= 1;
    }
    end.max(row.range.start)
}

proptest! {
    /// Each display row in the frame is the display map's row, placed below
    /// the rows of every block anchored at or above it; the rows and blocks
    /// in the frame are those in view, and the blocks are where their
    /// screen rows are.
    #[test]
    fn rows_placed_under_blocks(doc in doc(), blocks in blocks(1..=4)) {
        let editor = Editor::new(&doc);
        let blocks = anchor(&editor, &blocks);
        let starts = block_starts(&blocks);
        let block_map = BlockMap::new(blocks.iter().copied());
        let frame = editor.lay_out(Some(&block_map), &mut LongRows::default());
        let height = editor.height;
        let row_count = editor.display.row_count();
        let block_rows: u32 = blocks.iter().map(|block| block.rows).sum();
        prop_assert_eq!(frame.row_count, row_count);
        prop_assert_eq!(frame.screen_rows, row_count + block_rows);

        // The screen row of display row `row`, and its top in the frame.
        let screen_row = |row: u32| {
            row + blocks
                .iter()
                .filter(|block| block.above <= row)
                .map(|block| block.rows)
                .sum::<u32>()
        };
        let top = |row: u32| LINE * screen_row(row) as f32 - frame.scroll.y;
        for (row, layout) in (frame.first_row..).zip(&frame.rows) {
            prop_assert_eq!(layout.row, row);
            let display_row = editor.display.row(row);
            prop_assert_eq!(
                (layout.start, layout.end, layout.last),
                (display_row.range.start, display_row.range.end, display_row.last)
            );
            prop_assert_eq!(frame.row_top(row), top(row), "row {}", row);
            prop_assert_eq!(frame.line_origin(layout).y, top(row));
            prop_assert_eq!(block_map.at_screen_row(screen_row(row)), ScreenRow::Text(row));
            prop_assert!(top(row) < height && top(row) + LINE > 0., "row {} at {}", row, top(row));
        }
        // The rows just outside the frame are out of view.
        let end_row = frame.first_row + frame.rows.len() as u32;
        if let Some(before) = frame.first_row.checked_sub(1) {
            prop_assert!(top(before) + LINE <= 0., "row {} at {}", before, top(before));
        }
        if end_row < row_count {
            prop_assert!(top(end_row) >= height, "row {} at {}", end_row, top(end_row));
        }

        let in_view: Vec<BlockLayout> = starts
            .iter()
            .map(|(start, block)| BlockLayout {
                id: block.id,
                rows: block.rows,
                rect: Rect {
                    x: frame.text_left,
                    y: LINE * *start as f32 - frame.scroll.y,
                    width: WIDTH - frame.text_left,
                    height: LINE * block.rows as f32,
                },
            })
            .filter(|layout| layout.rect.y < height && layout.rect.y + layout.rect.height > 0.)
            .collect();
        prop_assert_eq!(&frame.blocks, &in_view);
        for (start, block) in &starts {
            for line in 0..block.rows {
                prop_assert_eq!(
                    block_map.at_screen_row(start + line),
                    ScreenRow::Block { id: block.id, line }
                );
            }
        }
    }

    /// With no blocks, or only blocks of no rows, the frame is the one
    /// `layout_frame` lays out.
    #[test]
    fn no_blocks_equals_layout_frame(doc in doc(), blocks in blocks(0..=0)) {
        let editor = Editor::new(&doc);
        let plain = editor.lay_out(None, &mut LongRows::default());
        let plain = format!("{plain:?}");
        let empty = BlockMap::new(anchor(&editor, &blocks));
        for blocks in [BlockMap::default(), empty] {
            let frame = editor.lay_out(Some(&blocks), &mut LongRows::default());
            prop_assert_eq!(&format!("{frame:?}"), &plain);
        }
    }

    /// Every offset shown in the frame is where `Frame::offset_at` puts its
    /// caret's position. A point on a block in the frame, even past the
    /// frame's edge, gives the end of the row above it, or the start of the
    /// text; one on a block outside the frame gives what the display row
    /// below it does.
    #[test]
    fn offset_at_round_trip(doc in doc(), blocks in blocks(1..=4)) {
        let editor = Editor::new(&doc);
        let blocks = anchor(&editor, &blocks);
        let block_map = BlockMap::new(blocks.iter().copied());
        let long_rows = &mut LongRows::default();
        let frame = editor.lay_out(Some(&block_map), long_rows);
        let text = editor.display.text();
        let offset_at = |x: f32, y: f32, long_rows: &mut LongRows| {
            frame.offset_at(text, Vec2 { x, y }, long_rows, &mut Mono)
        };
        for row in &frame.rows {
            let y = frame.row_top(row.row) + LINE / 2.;
            for offset in offsets_on(&editor.text, row, row.shown()) {
                let x = frame.view_x(row.x_for_offset(offset, long_rows, &mut Mono));
                prop_assert_eq!(offset_at(x, y, long_rows), offset, "row {} at {}", row.row, x);
            }
        }

        let xs = [0., frame.text_left + 3.5 * CELL, WIDTH - 1.];
        for layout in &frame.blocks {
            let above = blocks.iter().find(|block| block.id == layout.id).unwrap().above;
            let expected = block_offset(&editor, above);
            let rect = layout.rect;
            let ys = (0..layout.rows)
                .map(|line| rect.y + LINE * line as f32 + LINE / 2.)
                .chain([rect.y, rect.y + rect.height - 0.25])
                .filter(|y| (0. ..editor.height).contains(y));
            for y in ys {
                for x in xs {
                    prop_assert_eq!(
                        offset_at(x, y, long_rows),
                        expected,
                        "block {} at {}, {}", layout.id, x, y
                    );
                }
            }
        }

        // Block rows a few rows past either edge of the frame.
        let first = (frame.scroll.y / LINE).floor() as i64;
        let end = ((frame.scroll.y + editor.height) / LINE).ceil() as i64;
        for screen_row in (first - 4..first).chain(end..end + 4) {
            let Ok(screen_row) = u32::try_from(screen_row) else {
                continue;
            };
            let ScreenRow::Block { id, .. } = block_map.at_screen_row(screen_row) else {
                continue;
            };
            let above = blocks.iter().find(|block| block.id == id).unwrap().above;
            let y = LINE * screen_row as f32 - frame.scroll.y + LINE / 2.;
            let below = frame.row_top(above) + LINE / 2.;
            let in_frame = frame.blocks.iter().any(|layout| layout.id == id);
            for x in xs {
                let expected = if in_frame {
                    block_offset(&editor, above)
                } else {
                    offset_at(x, below, long_rows)
                };
                prop_assert_eq!(
                    offset_at(x, y, long_rows),
                    expected,
                    "block {} at {}, {}", id, x, y
                );
            }
        }
    }
}
