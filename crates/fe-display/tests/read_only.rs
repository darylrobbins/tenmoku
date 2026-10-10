//! A read-only `DisplayMap` over a `FileText`: rows are laid out at file
//! byte offsets, across `\r\n` breaks and invalid UTF-8, a long row is sliced
//! as in a buffer, and a later snapshot adds rows without moving the others.

use std::{
    fs,
    ops::Range,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::ResolvedSelection;
use fe_display::{
    CharWidth, DisplayMap, ExpandedRow, LongRows,
    layout::{
        Frame, FrameInput, FrameStyles, MAX_SHAPED_ROW_BYTES, MeasuredLine, StyleRun, TextMeasure,
        Vec2, layout_frame,
    },
};
use fe_fileview::{FileText, FileView, IndexState, LineIndex};
use fe_text::TextSource;
use rand::{Rng, SeedableRng, rngs::StdRng};

const MIB: usize = 1024 * 1024;
const CELL: f32 = 10.;
const LINE: f32 = 20.;
const WIDTH: f32 = 2000.;
const HEIGHT: f32 = 600.;

/// Every char a cell wide.
struct Cells;

impl CharWidth for Cells {
    fn char_width(&mut self, _: char) -> f32 {
        CELL
    }
}

/// A line of one-cell chars: the byte offset where each char starts.
struct Line {
    starts: Vec<usize>,
    len: usize,
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.starts.len() as f32 * CELL
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.starts.partition_point(|&start| start < index) as f32 * CELL
    }

    fn index_for_x(&self, x: f32) -> usize {
        let chars = (x / CELL).round().max(0.) as usize;
        self.starts.get(chars).copied().unwrap_or(self.len)
    }
}

/// Shapes chars a cell wide, recording the length of each text shaped.
#[derive(Default)]
struct Shaper {
    shaped: Vec<usize>,
}

impl TextMeasure for Shaper {
    type Style = ();
    type Line = Line;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> Line {
        self.shaped.push(text.len());
        Line {
            starts: text.char_indices().map(|(ix, _)| ix).collect(),
            len: text.len(),
        }
    }
}

/// A file in the temp directory, removed on drop.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "fe-display-read-only-{}-{name}",
            std::process::id()
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn view(&self) -> Arc<FileView> {
        Arc::new(FileView::open(&self.0).unwrap())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        _ = fs::remove_file(&self.0);
    }
}

/// A snapshot of a complete index of `view`.
fn complete(view: &Arc<FileView>) -> Arc<dyn TextSource> {
    let index = Arc::new(LineIndex::build(Arc::clone(view)));
    let deadline = Instant::now() + Duration::from_secs(60);
    while index.progress().state == IndexState::Building {
        assert!(Instant::now() < deadline, "still building");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(index.progress().state, IndexState::Complete);
    Arc::new(FileText::new(Arc::clone(view), index))
}

/// About `len` bytes of rows of 0 to 60 pieces, each ending in `\r\n`, then
/// `end` with no line break.
fn crlf_rows(len: usize, rng: &mut StdRng) -> Vec<u8> {
    const PIECES: [&[u8]; 9] = [
        b"text ",
        b"x",
        b"\t",
        "é".as_bytes(),
        "😀".as_bytes(),
        b"\xff",
        b"\xc3",
        b"\xe2\x82",
        b"\r",
    ];
    let mut bytes = Vec::with_capacity(len + 1024);
    while bytes.len() < len {
        for _ in 0..rng.random_range(0..60) {
            bytes.extend_from_slice(PIECES[rng.random_range(0..PIECES.len())]);
        }
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"end");
    bytes
}

/// `bytes` as `FileText` reads them: valid UTF-8 as itself and each byte of
/// an invalid sequence as `?`.
fn decode(mut bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                text.push_str(valid);
                return text;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                text.push_str(std::str::from_utf8(&bytes[..valid]).unwrap());
                let invalid = error.error_len().unwrap_or(bytes.len() - valid);
                text.extend(std::iter::repeat_n('?', invalid));
                bytes = &bytes[valid + invalid..];
            }
        }
    }
}

/// A read-only display of a file, and the long rows kept between its frames.
struct Viewer {
    display: DisplayMap,
    long_rows: LongRows,
    /// The ranges the last frame asked highlights for.
    highlighted: Vec<Range<usize>>,
}

impl Viewer {
    fn new(text: Arc<dyn TextSource>) -> Self {
        Self {
            display: DisplayMap::read_only(text, 4),
            long_rows: LongRows::default(),
            highlighted: Vec::new(),
        }
    }

    fn lay_out(&mut self, scroll: Vec2, shaper: &mut Shaper) -> Frame<Line> {
        let selections = [ResolvedSelection {
            range: 0..0,
            reversed: false,
        }];
        let input = FrameInput {
            display: &self.display,
            tab_size: 4,
            selections: &selections,
            primary_head: 0,
            scroll,
            autoscroll: false,
            width: WIDTH,
            height: HEIGHT,
            line_height: LINE,
            em: CELL,
            styles: FrameStyles {
                text: (),
                line_number: (),
                active_line_number: (),
            },
        };
        self.highlighted.clear();
        layout_frame(
            &input,
            shaper,
            &mut self.long_rows,
            &mut Cells,
            |range| {
                self.highlighted.push(range);
                Vec::new()
            },
            |_| Vec::new(),
        )
    }

    /// A frame with display row `row` at the top.
    fn lay_out_at(&mut self, row: u32) -> Frame<Line> {
        let scroll = Vec2 {
            x: 0.,
            y: row as f32 * LINE,
        };
        self.lay_out(scroll, &mut Shaper::default())
    }
}

/// Each row of `frame` is its buffer row, at the file's byte offsets, and
/// shows the file's bytes there.
fn assert_rows_match_file(frame: &Frame<Line>, text: &dyn TextSource, file: &[u8]) {
    assert!(!frame.rows.is_empty());
    for (ix, row) in frame.rows.iter().enumerate() {
        let buffer_row = row.buffer_row;
        assert_eq!(buffer_row, frame.first_row + ix as u32);
        let start = text.row_start(buffer_row);
        assert_eq!(
            (row.start, row.end),
            (start, start + text.row_len(buffer_row) as usize)
        );
        assert_eq!(row.shown(), row.start..row.end);
        let bytes = &file[row.start..row.end];
        assert!(!bytes.contains(&b'\n'));
        if buffer_row + 1 < text.row_count() {
            assert_eq!(&file[row.end..row.end + 2], b"\r\n", "row {buffer_row}");
            assert_eq!(text.row_start(buffer_row + 1), row.end + 2);
        } else {
            assert_eq!(row.end, file.len());
        }
        let shown = decode(bytes);
        assert_eq!(shown, text.row_to_string(buffer_row));
        assert_eq!(
            row.expanded,
            ExpandedRow::new(&shown, 4),
            "row {buffer_row}"
        );
    }
}

/// A caret at every char boundary of every row of `frame` is placed where
/// clicking finds the same offset.
fn assert_carets_round_trip(frame: &Frame<Line>, viewer: &mut Viewer) {
    let text = viewer.display.text();
    for row in &frame.rows {
        let shown = text.slice_to_string(row.start..row.end);
        let boundaries = shown
            .char_indices()
            .map(|(ix, _)| row.start + ix)
            .chain([row.end]);
        for offset in boundaries {
            let caret = frame
                .caret_bounds(offset, &mut viewer.long_rows, &mut Cells)
                .unwrap_or_else(|| panic!("no caret at {offset}"));
            let position = Vec2 {
                x: caret.x,
                y: caret.y + LINE / 2.,
            };
            assert_eq!(
                frame.offset_at(text, position, &mut viewer.long_rows, &mut Cells),
                offset,
                "row {}",
                row.buffer_row
            );
        }
    }
}

#[test]
fn lays_out_a_file_text() {
    let file = crlf_rows(8 * MIB, &mut StdRng::seed_from_u64(272));
    let temp = TempFile::new("crlf.txt", &file);
    let text = complete(&temp.view());
    assert_eq!(text.len(), file.len());
    let mut viewer = Viewer::new(Arc::clone(&text));
    let rows = viewer.display.row_count();
    assert_eq!(rows, text.row_count());
    assert!(rows > 100_000, "{rows} rows");

    // The top, the middle, the last screenful, and past the end, which
    // scrolls the last row to the top.
    let visible = (HEIGHT / LINE) as u32;
    let mut shown = String::new();
    for top in [0, rows / 2, rows - visible, u32::MAX / 2] {
        let frame = viewer.lay_out_at(top);
        assert_eq!(frame.first_row, top.min(rows - 1));
        assert_rows_match_file(&frame, &*text, &file);
        shown.extend(frame.rows.iter().map(|row| row.expanded.text()));
        // Highlights are asked for once, across the `\r\n` between rows.
        let (first, last) = (&frame.rows[0], frame.rows.last().unwrap());
        assert_eq!(viewer.highlighted.len(), 1);
        assert_eq!(viewer.highlighted[0], first.start..last.end);
        assert_carets_round_trip(&frame, &mut viewer);
    }
    // Invalid bytes, lone `\r`s and non-ASCII were laid out.
    for ch in ['?', '\r', 'é', '😀'] {
        assert!(shown.contains(ch), "no {ch:?} shown");
    }
}

#[test]
fn long_row_is_sliced() {
    // Mostly ASCII, with tabs, multi-byte chars and invalid bytes along it.
    let mut rng = StdRng::seed_from_u64(2720);
    let mut file = Vec::with_capacity(32 * MIB + 64);
    while file.len() < 32 * MIB {
        let piece: &[u8] = match rng.random_range(0..40) {
            0 => b"\t",
            1 => "é".as_bytes(),
            2 => "😀".as_bytes(),
            3 => b"\xff",
            _ => b"text ",
        };
        file.extend_from_slice(piece);
    }
    let temp = TempFile::new("long-row.txt", &file);
    let text = complete(&temp.view());
    assert_eq!((text.row_count(), text.len()), (1, file.len()));
    let mut viewer = Viewer::new(Arc::clone(&text));

    // The end, which clamps the scroll to the row's width, then the middle
    // and the start.
    let mut lay_out = |scroll_x: f32| {
        let shaper = &mut Shaper::default();
        let frame = viewer.lay_out(Vec2 { x: scroll_x, y: 0. }, shaper);
        let row = &frame.rows[0];
        let shown = row.shown();
        let context = format!("at {scroll_x}: {shown:?}, shaped {:?}", shaper.shaped);
        let slice = row.slice.as_ref().expect(&context);
        assert!(shown.len() <= MAX_SHAPED_ROW_BYTES, "{context}");
        // The row is shaped once, as the file's bytes there read; line
        // numbers are shaped too, in a few bytes each.
        let shown_text = decode(&file[shown.clone()]);
        assert_eq!(text.slice_to_string(shown.clone()), shown_text, "{context}");
        let expanded = ExpandedRow::with_start_column(&shown_text, slice.start_column, 4);
        assert_eq!(row.expanded, expanded, "{context}");
        let shapes: Vec<_> = shaper.shaped.iter().filter(|&&len| len > 8).collect();
        assert_eq!(shapes, [&expanded.text().len()], "{context}");
        (frame.scroll.x, shown)
    };
    let (end_x, end) = lay_out(f32::MAX);
    assert_eq!(end.end, file.len());
    let (middle_x, middle) = lay_out(end_x / 2.);
    assert_eq!(middle_x, end_x / 2.);
    let (_, start) = lay_out(0.);
    assert_eq!(start.start, 0);
    // The middle frame shows the middle of the row, far from either end.
    let quarter = file.len() / 4;
    assert!(
        (quarter..3 * quarter).contains(&middle.start) && middle.end < 3 * quarter,
        "{middle:?}"
    );
}

#[test]
fn replace_text_adds_rows() {
    let file = crlf_rows(MIB, &mut StdRng::seed_from_u64(27200));
    let temp = TempFile::new("growing.txt", &file);
    let view = temp.view();
    let (index, mut steps) = LineIndex::build_stepped(Arc::clone(&view));
    let index = Arc::new(index);
    steps.next().expect("a first chunk");
    let first: Arc<dyn TextSource> = Arc::new(FileText::new(Arc::clone(&view), Arc::clone(&index)));
    let first_rows = first.row_count();
    let visible = (HEIGHT / LINE) as u32;
    assert!(first_rows > 2 * visible, "{first_rows} rows");

    let mut viewer = Viewer::new(Arc::clone(&first));
    assert_eq!(viewer.display.row_count(), first_rows);
    let tops = [0, first_rows / 2, first_rows - 1 - visible];
    let before: Vec<_> = tops.iter().map(|&top| viewer.lay_out_at(top)).collect();

    // Soft wrap stays off and nothing folds.
    viewer.display.set_wrap_width(Some(80.), &mut Cells);
    assert_eq!(viewer.display.wrap_width(), None);
    assert!(!viewer.display.fold(0..=3));
    assert!(viewer.display.folds().is_empty());
    assert_eq!(viewer.display.row_count(), first_rows);

    steps.for_each(drop);
    let second = Arc::new(FileText::new(Arc::clone(&view), Arc::clone(&index)));
    let second: Arc<dyn TextSource> = second;
    assert!(second.row_count() > first_rows);
    viewer.display.replace_text(Arc::clone(&second));
    assert_eq!(viewer.display.row_count(), second.row_count());
    assert_eq!(second.len(), file.len());

    // Rows before the first snapshot's last, which was still empty, lay out
    // as they did.
    for (&top, before) in tops.iter().zip(&before) {
        let after = viewer.lay_out_at(top);
        assert_eq!(after.first_row, before.first_row);
        for (old, new) in before.rows.iter().zip(&after.rows) {
            if old.buffer_row + 1 >= first_rows {
                continue;
            }
            assert_eq!(
                (old.row, old.buffer_row, old.start, old.end, old.last),
                (new.row, new.buffer_row, new.start, new.end, new.last)
            );
            assert_eq!(old.expanded, new.expanded);
            assert_eq!(old.line.starts, new.line.starts);
        }
        assert_rows_match_file(&after, &*second, &file);
    }

    // The new rows, through to the end.
    for top in [
        first_rows,
        (first_rows + second.row_count()) / 2,
        u32::MAX / 2,
    ] {
        let frame = viewer.lay_out_at(top);
        assert!(
            frame
                .rows
                .iter()
                .all(|row| row.buffer_row + 1 >= first_rows)
        );
        assert_rows_match_file(&frame, &*second, &file);
        assert_carets_round_trip(&frame, &mut viewer);
    }
}
