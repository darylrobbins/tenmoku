//! Blocks: whole rows placed between display rows. Rows below a block move
//! down by its height, hit testing and scrolling count block rows, and with
//! no blocks a frame is as it was.
//!
//! The text is ten rows of five chars, `row0` to `row9` and a space, so
//! display row `r` starts at buffer offset `6 * r`. Cells are 10 px and rows
//! 20 px.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{
    Block, BlockMap, CharWidth, DisplayMap, LongRows, ScreenRow,
    layout::{
        BlockLayout, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, TextMeasure,
        Vec2, layout_frame, layout_frame_with_blocks,
    },
};

const CELL: f32 = 10.;
const LINE: f32 = 20.;
const WIDTH: f32 = 400.;
const ROWS: u32 = 10;

#[derive(Clone, Debug, PartialEq)]
struct Line(String);

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.0.chars().count() as f32 * CELL
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.0[..index].chars().count() as f32 * CELL
    }

    fn index_for_x(&self, x: f32) -> usize {
        let column = (x / CELL).round().max(0.) as usize;
        self.0
            .char_indices()
            .nth(column)
            .map_or(self.0.len(), |(ix, _)| ix)
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
        Line(text.to_string())
    }
}

struct Editor {
    display: DisplayMap,
    selections: Vec<ResolvedSelection>,
}

impl Editor {
    /// The ten rows, with a caret at each of `carets` and a selection over
    /// `selected`.
    fn new(carets: &[usize], selected: Option<std::ops::Range<usize>>) -> Self {
        let text: String = (0..ROWS).map(|row| format!("row{row} \n")).collect();
        let text = text.trim_end_matches('\n');
        let mut selections: Vec<_> = carets
            .iter()
            .map(|&offset| ResolvedSelection {
                range: offset..offset,
                reversed: false,
            })
            .collect();
        selections.extend(selected.map(|range| ResolvedSelection {
            range,
            reversed: false,
        }));
        Self {
            display: DisplayMap::new(Buffer::new(text).current(), 4),
            selections,
        }
    }

    fn input(&self, scroll_y: f32, autoscroll: bool, height: f32) -> FrameInput<'_, ()> {
        FrameInput {
            display: &self.display,
            tab_size: 4,
            selections: &self.selections,
            primary_head: self.selections[0].head(),
            scroll: Vec2 { x: 0., y: scroll_y },
            autoscroll,
            width: WIDTH,
            height,
            line_height: LINE,
            em: CELL,
            styles: FrameStyles {
                text: (),
                line_number: (),
                active_line_number: (),
            },
        }
    }

    /// Lays out a frame `height` tall scrolled to `scroll_y`, with `blocks`
    /// or, given none, with [`layout_frame`]. Rows 1 and 7 head fold
    /// candidates.
    fn lay_out(
        &self,
        blocks: Option<&BlockMap>,
        scroll_y: f32,
        autoscroll: bool,
        height: f32,
    ) -> Frame<Line> {
        let input = self.input(scroll_y, autoscroll, height);
        let candidates = |rows: std::ops::Range<u32>| {
            [1, 7]
                .into_iter()
                .filter(|row| rows.contains(row))
                .collect()
        };
        let long_rows = &mut LongRows::default();
        match blocks {
            Some(blocks) => layout_frame_with_blocks(
                &input,
                blocks,
                &mut Mono,
                long_rows,
                &mut Mono,
                |_| Vec::new(),
                candidates,
            ),
            None => layout_frame(
                &input,
                &mut Mono,
                long_rows,
                &mut Mono,
                |_| Vec::new(),
                candidates,
            ),
        }
    }

    fn offset_at(&self, frame: &Frame<Line>, x: f32, y: f32) -> usize {
        frame.offset_at(
            self.display.text(),
            Vec2 { x, y },
            &mut LongRows::default(),
            &mut Mono,
        )
    }
}

fn block(above: u32, rows: u32, id: u64) -> Block {
    Block { above, rows, id }
}

#[test]
fn screen_rows_round_trip() {
    // Out of order, with a block of no rows that's dropped.
    let blocks = BlockMap::new([
        block(3, 2, 11),
        block(ROWS, 3, 13),
        block(0, 1, 10),
        block(5, 0, 99),
        block(3, 1, 12),
    ]);
    assert_eq!(blocks.block_rows(), 7);
    let screen_rows = ROWS + blocks.block_rows();

    let text_rows: Vec<u32> = (0..ROWS).map(|row| blocks.screen_row(row)).collect();
    for (row, &screen_row) in (0..).zip(&text_rows) {
        assert_eq!(blocks.at_screen_row(screen_row), ScreenRow::Text(row));
    }
    let block_rows: Vec<ScreenRow> = (0..screen_rows)
        .filter(|screen_row| !text_rows.contains(screen_row))
        .map(|screen_row| blocks.at_screen_row(screen_row))
        .collect();
    let expected = [
        (10, 0),
        (11, 0),
        (11, 1),
        (12, 0),
        (13, 0),
        (13, 1),
        (13, 2),
    ]
    .map(|(id, line)| ScreenRow::Block { id, line });
    assert_eq!(block_rows, expected);
    assert_eq!(
        text_rows.len() as u32 + block_rows.len() as u32,
        screen_rows
    );
    assert_eq!(blocks.at_screen_row(screen_rows), ScreenRow::Text(ROWS));
}

#[test]
fn rows_below_a_block_move_down() {
    let editor = Editor::new(&[6 * 6 + 2], Some(6 * 4 + 1..6 * 5 + 3));
    let blocks = BlockMap::new([block(5, 2, 7)]);
    let before = editor.lay_out(None, 0., false, 400.);
    let after = editor.lay_out(Some(&blocks), 0., false, 400.);

    assert_eq!(after.row_count, before.row_count);
    assert_eq!(after.screen_rows, before.screen_rows + 2);
    assert_eq!(after.rows.len(), before.rows.len());
    for (old, new) in before.rows.iter().zip(&after.rows) {
        assert_eq!((new.row, new.start, new.end), (old.row, old.start, old.end));
        let shift = if new.row >= 5 { 2. * LINE } else { 0. };
        assert_eq!(after.row_top(new.row), before.row_top(old.row) + shift);
        assert_eq!(after.line_origin(new).y, before.line_origin(old).y + shift);
    }
    assert_eq!(
        after.blocks,
        [BlockLayout {
            id: 7,
            rows: 2,
            rect: Rect {
                x: after.text_left,
                y: before.row_top(5),
                width: WIDTH - after.text_left,
                height: 2. * LINE,
            },
        }]
    );
    // What's drawn on a row moves with it.
    assert_eq!(after.cursors[0].y, before.cursors[0].y + 2. * LINE);
    let tops = |frame: &Frame<Line>| {
        frame
            .selections
            .iter()
            .map(|rect| rect.y)
            .collect::<Vec<_>>()
    };
    assert_eq!(tops(&after), [before.row_top(4), after.row_top(5)]);
    let number_tops = |frame: &Frame<Line>| {
        frame
            .line_numbers
            .iter()
            .map(|(origin, _)| origin.y)
            .collect::<Vec<_>>()
    };
    let rows_tops: Vec<f32> = (0..ROWS).map(|row| after.row_top(row)).collect();
    assert_eq!(number_tops(&after), rows_tops);
    assert_eq!(
        after.fold_toggles[1].rect.y,
        before.fold_toggles[1].rect.y + 2. * LINE
    );
}

#[test]
fn hit_testing_skips_blocks() {
    let editor = Editor::new(&[0], None);
    let blocks = BlockMap::new([block(5, 2, 7)]);
    let frame = editor.lay_out(Some(&blocks), 0., false, 400.);
    let x = frame.text_left + 2. * CELL;
    let old_top_5 = LINE * 5.;
    let new_top_5 = LINE * 7.;

    // Inside the block: the end of row 4, wherever across.
    for y in [old_top_5, old_top_5 + LINE * 0.5, new_top_5 - 0.5] {
        assert_eq!(editor.offset_at(&frame, x, y), 6 * 4 + 5, "at {y}");
        assert_eq!(editor.offset_at(&frame, 0., y), 6 * 4 + 5, "at {y}");
    }
    assert_eq!(editor.offset_at(&frame, frame.text_left, new_top_5), 6 * 5);
    assert_eq!(editor.offset_at(&frame, x, new_top_5 + 1.), 6 * 5 + 2);
    assert_eq!(editor.offset_at(&frame, x, old_top_5 - 1.), 6 * 4 + 2);
    let caret = frame
        .caret_bounds(6 * 5 + 2, &mut LongRows::default(), &mut Mono)
        .unwrap();
    assert_eq!((caret.x, caret.y), (x, new_top_5));

    let at = |y: f32| frame.screen_row_at(y);
    assert_eq!(at(old_top_5 - 1.), ScreenRow::Text(4));
    assert_eq!(at(old_top_5), ScreenRow::Block { id: 7, line: 0 });
    assert_eq!(at(old_top_5 + LINE), ScreenRow::Block { id: 7, line: 1 });
    assert_eq!(at(new_top_5 - 0.5), ScreenRow::Block { id: 7, line: 1 });
    assert_eq!(at(new_top_5), ScreenRow::Text(5));

    // Scrolled so the block is partly in view at the top: the row above it
    // is out of the frame, but a click on the block still goes to its end.
    let frame = editor.lay_out(Some(&blocks), old_top_5 + LINE * 1.5, false, 100.);
    assert_eq!(frame.first_row, 5);
    assert_eq!(frame.blocks.len(), 1);
    assert_eq!(frame.screen_row_at(1.), ScreenRow::Block { id: 7, line: 1 });
    assert_eq!(editor.offset_at(&frame, x, 1.), 6 * 4 + 5);

    // A block above row 0 goes to the start of the text.
    let frame = editor.lay_out(Some(&BlockMap::new([block(0, 1, 3)])), 0., false, 400.);
    assert_eq!(frame.screen_row_at(5.), ScreenRow::Block { id: 3, line: 0 });
    assert_eq!(editor.offset_at(&frame, x, 5.), 0);
    assert_eq!(editor.offset_at(&frame, x, LINE + 1.), 2);
}

#[test]
fn scrolling_counts_block_rows() {
    let last_row = 6 * (ROWS as usize - 1) + 2;
    let editor = Editor::new(&[last_row], None);
    let blocks = BlockMap::new([block(2, 3, 1), block(6, 2, 2), block(ROWS, 1, 3)]);
    let height = 100.;

    // Autoscroll keeps the caret on the last row within the margins.
    let frame = editor.lay_out(Some(&blocks), 0., true, height);
    let margin = (LINE * 3.).min(height / 3.);
    let cursor = frame.cursors[0];
    assert!(cursor.y >= margin - 1e-3, "{cursor:?}");
    assert!(cursor.y + LINE <= height - margin + 1e-3, "{cursor:?}");
    assert_eq!(frame.row(ROWS - 1).map(|row| row.row), Some(ROWS - 1));

    // The last screen row, here a block's, can reach the top.
    let frame = editor.lay_out(Some(&blocks), 1e6, false, height);
    assert_eq!(frame.screen_rows, ROWS + 6);
    assert_eq!(frame.scroll.y, LINE * (frame.screen_rows - 1) as f32);
    assert_eq!(frame.screen_row_at(0.), ScreenRow::Block { id: 3, line: 0 });
    assert!(frame.rows.is_empty());
    assert_eq!(frame.blocks.iter().map(|b| b.id).collect::<Vec<_>>(), [3]);
    assert_eq!(frame.blocks[0].rect.y, 0.);

    // Only blocks at least partly in view are laid out: block 1 covers
    // 40..100 px and block 2 180..220.
    let ids = |scroll_y: f32| {
        let frame = editor.lay_out(Some(&blocks), scroll_y, false, height);
        frame.blocks.iter().map(|b| b.id).collect::<Vec<_>>()
    };
    assert_eq!(ids(50.), [1]);
    assert_eq!(ids(100.), [2]);
    assert_eq!(ids(170.), [2]);
    assert_eq!(ids(0.), [1]);
    assert_eq!(ids(85.), [1, 2]);

    // Display-row tops survive the trip through screen rows; tops in a
    // block go to the row below it.
    for top in [0., 0.5, 1., 2., 2.25, 5.5, 6., 9.75, 10.] {
        assert_eq!(blocks.to_display_top(blocks.to_screen_top(top)), top);
    }
    assert_eq!(blocks.to_screen_top(2.25), 5.25);
    assert_eq!(blocks.to_display_top(3.5), 2.);
    assert_eq!(blocks.to_display_top(15.5), 10.);
}

#[test]
fn no_blocks_is_unchanged() {
    let carets = [6 * 2 + 1, 6 * 8 + 4];
    let mut editor = Editor::new(&carets, Some(6 * 3 + 2..6 * 6 + 1));
    editor.display.fold(4..=5);
    let only_empty = BlockMap::new([block(3, 0, 1)]);
    for (scroll_y, autoscroll, height) in [(0., false, 400.), (30., false, 100.), (0., true, 60.)] {
        let plain = editor.lay_out(None, scroll_y, autoscroll, height);
        for blocks in [BlockMap::default(), only_empty.clone()] {
            let frame = editor.lay_out(Some(&blocks), scroll_y, autoscroll, height);
            let context = format!("scrolled to {scroll_y}, {height} tall");
            assert_eq!(frame.scroll, plain.scroll, "{context}");
            assert_eq!(frame.first_row, plain.first_row, "{context}");
            assert_eq!(frame.screen_rows, plain.row_count, "{context}");
            assert!(frame.blocks.is_empty(), "{context}");
            let rows = |frame: &Frame<Line>| {
                frame
                    .rows
                    .iter()
                    .map(|row| (row.row, frame.row_top(row.row), row.start, row.end))
                    .collect::<Vec<_>>()
            };
            assert_eq!(rows(&frame), rows(&plain), "{context}");
            assert_eq!(frame.selections, plain.selections, "{context}");
            assert_eq!(frame.cursors, plain.cursors, "{context}");
            assert_eq!(frame.line_numbers, plain.line_numbers, "{context}");
            assert_eq!(frame.fold_toggles, plain.fold_toggles, "{context}");
            assert_eq!(frame.active_lines, plain.active_lines, "{context}");
        }
    }
}
