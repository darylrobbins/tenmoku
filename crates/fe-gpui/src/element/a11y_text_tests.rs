//! The text area's accessible text, loaded in `accesskit_consumer` as a
//! platform adapter loads it, and asked what accesskit_macos asks for
//! VoiceOver: the document's text and length, lines, the selection, the
//! line of the caret, and bounds (design section 9, shape 1).
//!
//! Frames are laid out with every char a 10 px cell and 20 px rows, the
//! frame's top-left corner at (100, 50) in the window.

use std::{ops::Range, sync::Arc};

use accesskit_consumer::Tree;
use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{
    CharWidth, DisplayMap, LongRows,
    layout::{
        Frame, FrameInput, FrameStyles, MeasuredLine, StyleRun, TextMeasure, Vec2, layout_frame,
    },
};
use gpui::accesskit::{self, Node, NodeId, Point, Role, TreeId, TreeUpdate};

use super::a11y_text::*;

const CELL: f32 = 10.;
const LINE: f32 = 20.;
const WIDTH: f32 = 800.;
const ORIGIN: (f32, f32) = (100., 50.);
const ROOT: NodeId = NodeId(0);
const EDITOR: NodeId = NodeId(1);

#[derive(Clone, Debug, PartialEq)]
struct Line(String);

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.0.chars().count() as f32 * CELL
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.0[..index.min(self.0.len())].chars().count() as f32 * CELL
    }

    fn index_for_x(&self, x: f32) -> usize {
        let column = (x / CELL).round().max(0.) as usize;
        (self.0.char_indices().nth(column)).map_or(self.0.len(), |(ix, _)| ix)
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

fn display(text: &str) -> DisplayMap {
    DisplayMap::new(Buffer::new(text).current(), 4)
}

/// Soft wrapped at `columns` cells.
fn wrapped(text: &str, columns: usize) -> DisplayMap {
    let mut display = display(text);
    display.set_wrap_width(Some(columns as f32 * CELL), &mut Mono);
    while display.wrap_pending(0, usize::MAX, &mut Mono) {}
    display
}

/// A view of `display`: `rows` rows tall, scrolled to display row `top`,
/// with the selection from `anchor` to `head`.
struct View {
    top: u32,
    rows: u32,
    anchor: usize,
    head: usize,
    cap: Cap,
    read_only: bool,
    scale: f32,
}

impl View {
    fn new(top: u32, rows: u32) -> Self {
        Self {
            top,
            rows,
            anchor: 0,
            head: 0,
            cap: Cap::EDITOR,
            read_only: false,
            scale: 1.,
        }
    }

    fn caret(self, offset: usize) -> Self {
        self.select(offset, offset)
    }

    fn select(self, anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            ..self
        }
    }

    fn cap(self, rows: usize, bytes: usize) -> Self {
        Self {
            cap: Cap { rows, bytes },
            ..self
        }
    }

    fn frame(&self, display: &DisplayMap, long_rows: &mut LongRows) -> Frame<Line> {
        let selections = [ResolvedSelection {
            range: self.anchor.min(self.head)..self.anchor.max(self.head),
            reversed: self.head < self.anchor,
        }];
        let input = FrameInput {
            display,
            tab_size: 4,
            selections: &selections,
            primary_head: self.head,
            scroll: Vec2 {
                x: 0.,
                y: self.top as f32 * LINE,
            },
            autoscroll: false,
            width: WIDTH,
            height: self.rows as f32 * LINE,
            line_height: LINE,
            em: CELL,
            styles: FrameStyles {
                text: (),
                line_number: (),
                active_line_number: (),
            },
        };
        layout_frame(
            &input,
            &mut Mono,
            long_rows,
            &mut Mono,
            |_| Vec::new(),
            |_| Vec::new(),
        )
    }

    /// The accessible text of `display` in this view, and the frame.
    fn build(&self, display: &DisplayMap) -> (A11yText, Frame<Line>) {
        let mut long_rows = LongRows::default();
        let frame = self.frame(display, &mut long_rows);
        let text = build(Input {
            display,
            frame: &frame,
            long_rows: &mut long_rows,
            widths: &mut Mono,
            tab_size: 4,
            anchor: self.anchor,
            head: self.head,
            origin: ORIGIN,
            scale: self.scale,
            cap: self.cap,
            read_only: self.read_only,
        });
        (text, frame)
    }

    fn text(&self, display: &DisplayMap) -> A11yText {
        self.build(display).0
    }
}

fn run_id(key: RunKey) -> NodeId {
    NodeId(((u64::from(key.buffer_row) + 1) << 20) | u64::from(key.segment))
}

/// `text` as a tree: a window holding the text area, focused, holding the
/// runs.
fn tree(text: &A11yText) -> Tree {
    let mut editor = Node::new(Role::MultilineTextInput);
    let runs = text.write(&mut editor, run_id);
    editor.set_children(runs.iter().map(|(id, _)| *id).collect::<Vec<_>>());
    let mut root = Node::new(Role::Window);
    root.set_children(vec![EDITOR]);
    let mut nodes = runs;
    nodes.push((EDITOR, editor));
    nodes.push((ROOT, root));
    let update = TreeUpdate {
        nodes,
        tree: Some(accesskit::Tree::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: EDITOR,
    };
    Tree::new(update, true)
}

/// What a screen reader reads of `text`.
struct Read {
    document: String,
    utf16_len: usize,
    lines: Vec<String>,
    selected: String,
    /// The focus's line, and its UTF-16 index in the document.
    focus_line: usize,
    focus_utf16: usize,
    anchor_utf16: usize,
    read_only: bool,
}

fn read(text: &A11yText) -> Read {
    let tree = tree(text);
    let state = tree.state();
    let editor = state.focus().expect("the text area is focused");
    assert!(editor.supports_text_ranges());
    let document = editor.document_range();
    // Each run is a line (none are linked); past the last, the consumer
    // gives an empty range at the end.
    let lines = (0..text.runs.len())
        .map(|index| editor.line_range_from_index(index).expect("a line").text())
        .collect();
    let selection = editor.text_selection().expect("a selection");
    let focus = editor.text_selection_focus().unwrap();
    let anchor = editor.text_selection_anchor().unwrap();
    Read {
        document: document.text(),
        utf16_len: document.end().to_global_utf16_index(),
        lines,
        selected: selection.text(),
        focus_line: focus.to_line_index(),
        focus_utf16: focus.to_global_utf16_index(),
        anchor_utf16: anchor.to_global_utf16_index(),
        read_only: editor.is_read_only(),
    }
}

/// The buffer's text over `range`.
fn slice(display: &DisplayMap, range: Range<usize>) -> String {
    display.text().slice_to_string(range)
}

/// `n` numbered rows: `row 0\nrow 1\n…`, no break after the last.
fn numbered(n: usize) -> String {
    (0..n)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn window_is_the_viewport_and_as_much_again_each_side_in_order() {
    let display = display(&numbered(100));
    let view = View::new(40, 10).caret(display.text().row_start(45));
    let (text, frame) = view.build(&display);
    let visible = frame.rows.len() as u32;
    let (first, last) = (frame.first_row, frame.first_row + visible - 1);
    let rows: Vec<u32> = text.runs.iter().map(|run| run.display_row).collect();
    let expected: Vec<u32> = (first - visible..=last + visible).collect();
    assert_eq!(rows, expected, "margins on both sides, in display order");
    let read = read(&text);
    // The document is the buffer's text in order, from the first run's row
    // through the last's break.
    let start = display.text().row_start(first - visible);
    let end = display.text().row_start(last + visible + 1);
    assert_eq!(read.document, slice(&display, start..end));
    // Lines follow the rows, each with its break.
    assert_eq!(read.lines.len(), expected.len());
    for (line, row) in read.lines.iter().zip(&expected) {
        assert_eq!(line, &format!("row {row}\n"));
    }
    assert_eq!(read.lines[(45 - (first - visible)) as usize], "row 45\n");
    assert_eq!(read.focus_line, (45 - (first - visible)) as usize);
}

#[test]
fn window_is_clipped_to_the_file() {
    let display = display(&numbered(12));
    let view = View::new(0, 10).caret(0);
    let (text, frame) = view.build(&display);
    assert_eq!(frame.first_row, 0);
    let rows: Vec<u32> = text.runs.iter().map(|run| run.display_row).collect();
    assert_eq!(rows, (0..12).collect::<Vec<_>>());
    let read = read(&text);
    assert_eq!(read.document, numbered(12));
    // The last line has no break after it.
    assert_eq!(read.lines.last().unwrap(), "row 11");
}

#[test]
fn wrapped_row_is_its_own_line() {
    // Wrapped at 10 cells: "aaaa bbbb " | "cccc"
    let display = wrapped("short\naaaa bbbb cccc\nend", 10);
    let text = View::new(0, 10).caret(16).text(&display);
    let keys: Vec<RunKey> = text.runs.iter().map(|run| run.key).collect();
    let key = |buffer_row, segment| RunKey {
        buffer_row,
        segment,
    };
    assert_eq!(keys, [key(0, 0), key(1, 0), key(1, 1), key(2, 0)]);
    let read = read(&text);
    // Only a line's last segment ends in its break.
    assert_eq!(read.lines, ["short\n", "aaaa bbbb ", "cccc\n", "end"]);
    assert_eq!(read.document, "short\naaaa bbbb cccc\nend");
    // The caret in "cccc" is on the wrapped line's second row.
    assert_eq!(read.focus_line, 2);
    assert_eq!(text.focus, (key(1, 1), 0));
}

#[test]
fn fold_hides_its_rows_and_the_header_has_no_marker() {
    let mut display = display("fn a() {\n    one\n    two\n}\nafter");
    display.fold(0..=2);
    let text = View::new(0, 10).caret(0).text(&display);
    let read = read(&text);
    assert_eq!(read.lines, ["fn a() {\n", "}\n", "after"]);
    assert!(!read.document.contains(fe_display::layout::FOLD_MARKER));
}

#[test]
fn an_end_in_a_fold_is_at_the_end_of_the_headers_text() {
    let source = "fn a() {\n    one\n    two\n}\nafter";
    let mut display = display(source);
    display.fold(0..=2);
    let inside = source.find("one").unwrap() + 1;
    // A caret hidden by the fold, and a selection from inside it on.
    let text = View::new(0, 10).caret(inside).text(&display);
    let header = text.runs[0].key;
    assert_eq!(text.focus, (header, "fn a() {".len()));
    let read = read(&text);
    assert_eq!(read.focus_line, 0);
    assert_eq!(read.focus_utf16, "fn a() {".len());
    let after = source.find("after").unwrap() + 2;
    let text = View::new(0, 10).select(inside, after).text(&display);
    assert_eq!(read_selected(&text), "\n}\naf");
}

fn read_selected(text: &A11yText) -> String {
    read(text).selected
}

#[test]
fn crlf_files_read_as_lf_in_the_editor() {
    let buffer = Buffer::from_file_contents("one\r\n\r\ntwo\r\n");
    let display = DisplayMap::new(buffer.current(), 4);
    let text = View::new(0, 10).caret(4).text(&display);
    let read = read(&text);
    assert_eq!(read.lines, ["one\n", "\n", "two\n", ""]);
    assert_eq!(read.focus_line, 1);
    // The caret on the blank line has a box.
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    let caret = editor.text_selection().unwrap();
    assert_eq!(caret.bounding_boxes().len(), 1);
}

/// A complete viewer source over `bytes`, which keeps the file's line
/// breaks as they are, a CRLF two bytes.
fn file_text(bytes: &[u8]) -> (tempfile::TempDir, Arc<dyn fe_text::TextSource>) {
    use fe_fileview::{FileText, FileView, IndexState, LineIndex};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.txt");
    std::fs::write(&path, bytes).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let index = Arc::new(LineIndex::build(Arc::clone(&view)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while index.progress().state == IndexState::Building {
        assert!(std::time::Instant::now() < deadline, "still indexing");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    (dir, Arc::new(FileText::new(view, index)))
}

#[test]
fn a_crlf_is_one_character() {
    let (_dir, source) = file_text(b"one\r\n\r\ntwo");
    let display = DisplayMap::read_only(source, 4);
    // On the blank line, then on the break after "one".
    let text = View::new(0, 10).caret(5).text(&display);
    assert_eq!(text.runs[0].text, "one\r\n");
    assert_eq!(text.runs[0].character_lengths, [1, 1, 1, 2]);
    assert_eq!(text.runs[1].text, "\r\n");
    assert_eq!(text.runs[1].character_lengths, [2]);
    let read = read(&text);
    assert_eq!(read.lines, ["one\r\n", "\r\n", "two"]);
    assert_eq!(read.utf16_len, 10);
    assert_eq!(read.focus_line, 1);
    // A caret before a CRLF can't land between its bytes: an offset inside
    // it is the break's start.
    let text = View::new(0, 10).caret(4).text(&display);
    assert_eq!(text.focus, (text.runs[0].key, 3));
    for caret in [3, 5] {
        let text = View::new(0, 10).caret(caret).text(&display);
        let tree = tree(&text);
        let state = tree.state();
        let editor = state.focus().unwrap();
        let boxes = editor.text_selection().unwrap().bounding_boxes();
        assert_eq!(boxes.len(), 1, "a box for the caret at {caret}");
    }
}

#[test]
fn non_bmp_characters_count_two_utf16_units() {
    let display = display("a😀b\nc");
    let after_emoji = "a😀".len();
    let text = View::new(0, 10).select(1, after_emoji).text(&display);
    assert_eq!(text.runs[0].character_lengths, [1, 4, 1, 1]);
    let read = read(&text);
    assert_eq!(read.utf16_len, 1 + 2 + 1 + 1 + 1);
    assert_eq!(read.selected, "😀");
    assert_eq!((read.anchor_utf16, read.focus_utf16), (1, 3));
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    let position = editor.text_position_from_global_utf16_index(5).unwrap();
    assert_eq!(position.to_line_index(), 1);
}

#[test]
fn a_cluster_is_one_character() {
    // e and a combining acute; a flag of two regional indicators.
    let display = display("e\u{301}x🇨🇦");
    let text = View::new(0, 10).caret(0).text(&display);
    assert_eq!(text.runs[0].character_lengths, [3, 1, 8]);
}

#[test]
fn an_empty_file_has_one_empty_run() {
    let display = display("");
    let (text, _) = View::new(0, 10).build(&display);
    assert_eq!(text.runs.len(), 1);
    let run = &text.runs[0];
    assert!(run.text.is_empty() && run.character_lengths.is_empty());
    assert!(run.positions.is_empty() && run.widths.is_empty());
    assert_eq!(run.bounds.x0, run.bounds.x1, "zero wide");
    let read = read(&text);
    assert_eq!(read.document, "");
    assert_eq!(read.utf16_len, 0);
    assert_eq!((read.focus_line, read.focus_utf16), (0, 0));
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    assert_eq!(editor.text_selection().unwrap().bounding_boxes().len(), 1);
}

#[test]
fn vims_normal_mode_is_a_collapsed_caret_read_only() {
    let display = display("let x = 1;\n");
    // The block is over "x", the model's caret before it.
    let mut view = View::new(0, 10).caret(4);
    view.read_only = true;
    let text = view.text(&display);
    let read = read(&text);
    assert!(read.read_only);
    assert_eq!(read.selected, "");
    assert_eq!(read.focus_utf16, 4);
    // What VoiceOver speaks: the character after the insertion point.
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    let caret = editor.text_selection_focus().unwrap();
    let mut next = caret.to_degenerate_range();
    next.set_end(caret.forward_to_character_end());
    assert_eq!(next.text(), "x");
    // Insert mode takes typing.
    let text = View::new(0, 10).caret(4).text(&display);
    assert!(!read_text(&text).read_only);
}

fn read_text(text: &A11yText) -> Read {
    read(text)
}

#[test]
fn rows_cap_keeps_the_visible_rows_and_alternates_margins() {
    let display = display(&numbered(1000));
    let view = View::new(500, 10)
        .cap(16, usize::MAX)
        .caret(display.text().row_start(505));
    let (text, frame) = view.build(&display);
    assert_eq!(text.runs.len(), 16);
    let visible = frame.rows.len() as u32;
    let first = frame.first_row;
    let rows: Vec<u32> = text.runs.iter().map(|run| run.display_row).collect();
    let margin = (16 - visible) / 2;
    let start = first - margin;
    assert_eq!(rows, (start..start + 16).collect::<Vec<_>>());
}

#[test]
fn the_editors_cap_is_300_rows_and_64_kib() {
    let display = display(&numbered(10_000));
    // A tall view of tiny rows: more than 300 rows with their margins.
    let view = View::new(5000, 200).caret(display.text().row_start(5050));
    let text = view.text(&display);
    assert_eq!(text.runs.len(), 300);
    let bytes: usize = text.runs.iter().map(|run| run.text.len()).sum();
    assert!(bytes <= 64 * 1024);
    // Long rows: the byte cap stops the margins.
    let long: String = (0..1000)
        .map(|row| format!("{row:04}{}", "x".repeat(4996)))
        .collect::<Vec<_>>()
        .join("\n");
    let display = self::display(&long);
    let (text, frame) = View::new(500, 10).build(&display);
    let bytes: usize = text.runs.iter().map(|run| run.text.len()).sum();
    assert!(bytes <= 64 * 1024, "{bytes}");
    assert!(text.runs.len() < 3 * frame.rows.len());
    assert!(text.runs.len() > frame.rows.len());
    // Margin rows are whole; visible ones aren't cut while the budget lasts.
    assert!(text.runs.iter().all(|run| !run.truncated));
}

#[test]
fn a_visible_row_past_the_byte_cap_is_cut_and_an_end_past_it_is_its_end() {
    let row = "x".repeat(100 * 1024);
    let source = format!("{row}\nnext");
    let display = display(&source);
    // A caret at the row's end, past the cut, and a selection over it all.
    let text = View::new(0, 10).caret(row.len()).text(&display);
    let first = &text.runs[0];
    assert!(first.truncated);
    assert_eq!(first.text.len(), 64 * 1024);
    assert_eq!(first.character_lengths.len(), first.positions.len());
    assert_eq!(text.focus, (first.key, 64 * 1024));
    // The next row is visible too, but nothing of it is left.
    assert_eq!(text.runs.len(), 2);
    assert_eq!(text.runs[1].text, "");
    let read = read(&text);
    assert_eq!(read.focus_utf16, 64 * 1024);
    let text = View::new(0, 10).select(10, source.len()).text(&display);
    let read = self::read(&text);
    assert_eq!(read.selected.len(), 64 * 1024 - 10);
}

#[test]
fn a_cut_leaves_a_cluster_across_it_whole() {
    // "e" and a combining acute across the cap's byte.
    let row = format!("{}e\u{301}tail", "x".repeat(100));
    let display = display(&row);
    let text = View::new(0, 10).cap(300, 101).caret(0).text(&display);
    assert!(text.runs[0].truncated);
    assert_eq!(text.runs[0].text, "x".repeat(100));
}

#[test]
fn select_all_over_the_cap_is_the_whole_window() {
    let display = display(&numbered(10_000));
    let len = display.text().len();
    let text = View::new(5000, 30).select(0, len).text(&display);
    let read = read(&text);
    assert_eq!(read.selected, read.document);
    assert_eq!((read.anchor_utf16, read.focus_utf16), (0, read.utf16_len));
    // Backwards, it stays backwards.
    let text = View::new(5000, 30).select(len, 0).text(&display);
    let read = self::read(&text);
    assert_eq!(read.selected, read.document);
    assert_eq!((read.anchor_utf16, read.focus_utf16), (read.utf16_len, 0));
}

#[test]
fn a_selection_from_above_to_below_the_window_is_all_of_it() {
    let display = display(&numbered(1000));
    let (above, below) = (
        display.text().row_start(10) + 2,
        display.text().row_start(900) + 3,
    );
    let text = View::new(500, 10).select(above, below).text(&display);
    let read = read(&text);
    assert_eq!(read.selected, read.document);
    // One end in the window, the other below it.
    let inside = display.text().row_start(505) + 1;
    let text = View::new(500, 10).select(inside, below).text(&display);
    let read = self::read(&text);
    assert!(read.document.ends_with(&read.selected));
    assert!(read.selected.starts_with("ow 505\n"));
}

#[test]
fn a_caret_scrolled_off_is_at_the_nearer_edge() {
    let display = display(&numbered(1000));
    let text = View::new(500, 10)
        .caret(display.text().row_start(3) + 1)
        .text(&display);
    assert_eq!(text.focus, (text.runs[0].key, 0));
    let read = read(&text);
    assert_eq!(
        (read.focus_line, read.focus_utf16, read.selected.as_str()),
        (0, 0, "")
    );

    let text = View::new(500, 10)
        .caret(display.text().row_start(990))
        .text(&display);
    let last = text.runs.last().unwrap();
    assert_eq!(text.focus, (last.key, last.character_lengths.len()));
    let read = self::read(&text);
    assert_eq!(read.focus_utf16, read.utf16_len);
    assert_eq!(read.selected, "");
}

#[test]
fn every_end_names_an_emitted_run() {
    // Folds, wrap, a cut row and the file's ends: no projection panics the
    // consumer, whatever the selection.
    let row = "y".repeat(70 * 1024);
    let source = format!(
        "{}\nfn f() {{\n  a\n  b\n}}\n{row}\n{}",
        numbered(400),
        numbered(50)
    );
    let mut display = wrapped(&source, 30);
    let header = 400;
    display.fold(header..=header + 2);
    let len = display.text().len();
    let offsets = [0, 1, 500, len / 3, len / 2, len - 5, len];
    for top in [0, 200, 390, 400, display.row_count() - 5] {
        for &anchor in &offsets {
            for &head in &offsets {
                let text = View::new(top, 12).select(anchor, head).text(&display);
                for (key, _) in [text.anchor, text.focus] {
                    assert!(text.run(key).is_some(), "{key:?} at top {top}");
                }
                let read = read(&text);
                assert!(read.document.contains(&read.selected));
            }
        }
    }
}

#[test]
fn every_run_has_its_geometry() {
    let display = display("\tab\n\nc😀\nlast");
    let mut view = View::new(0, 10).caret(0);
    view.scale = 2.;
    let text = view.text(&display);
    for run in &text.runs {
        assert_eq!(run.positions.len(), run.character_lengths.len());
        assert_eq!(run.widths.len(), run.character_lengths.len());
    }
    // The tab is a tab stop wide; a break is zero wide at the text's end;
    // everything is in device pixels.
    let first = &text.runs[0];
    assert_eq!(first.positions, [0., 80., 100., 120.]);
    assert_eq!(first.widths, [80., 20., 20., 0.]);
    assert_eq!(first.bounds.y1 - first.bounds.y0, f64::from(LINE * 2.));
    // A blank line's break is at x 0, in a zero-wide box at the row's start.
    let blank = &text.runs[1];
    assert_eq!(
        (blank.positions.as_slice(), blank.widths.as_slice()),
        ([0.].as_slice(), [0.].as_slice())
    );
    assert_eq!(blank.bounds.x0, blank.bounds.x1);
    assert!(blank.bounds.y0 >= first.bounds.y1);
}

/// The boxes of the caret at `offset`, in a 10-row view of `display` from
/// display row `top`.
fn caret_boxes(display: &DisplayMap, top: u32, offset: usize) -> Vec<accesskit::Rect> {
    let text = View::new(top, 10).caret(offset).text(display);
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    editor.text_selection().unwrap().bounding_boxes()
}

#[test]
fn a_caret_has_one_box_anywhere() {
    let source = "abc def\n\nend\n";
    let display = display(source);
    // Mid-row, at a row's end before its break, on a blank line, and on the
    // empty last line.
    for offset in [2, 7, 8, source.len()] {
        let boxes = caret_boxes(&display, 0, offset);
        assert_eq!(boxes.len(), 1, "the caret at {offset}");
    }
    // Mid-row, the box is at the character's left.
    let boxes = caret_boxes(&display, 0, 2);
    let text = View::new(0, 10).text(&display);
    let x0 = text.runs[0].bounds.x0;
    assert_eq!(boxes[0].x0, x0 + f64::from(2. * CELL));
    // The empty last line is an empty run.
    let text = View::new(0, 10).caret(source.len()).text(&display);
    let last = text.runs.last().unwrap();
    assert!(last.text.is_empty());
    assert_eq!(text.focus, (last.key, 0));
}

#[test]
fn visible_and_margin_rows_have_boxes() {
    let display = display(&numbered(100));
    let (text, frame) = View::new(40, 10).build(&display);
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    let first_visible = frame.first_row;
    let line = |row: u32| {
        let index = text
            .runs
            .iter()
            .position(|run| run.display_row == row)
            .unwrap();
        editor.line_range_from_index(index).unwrap()
    };
    // A visible row, a margin row above, one below, and a range across both.
    let visible = line(first_visible + 2).bounding_boxes();
    assert_eq!(visible.len(), 1);
    let above = line(first_visible - 3).bounding_boxes();
    assert_eq!(above.len(), 1);
    assert!(above[0].y1 <= f64::from(ORIGIN.1), "above the view");
    let below = line(first_visible + frame.rows.len() as u32 + 2).bounding_boxes();
    assert_eq!(below.len(), 1);
    assert!(below[0].y0 >= f64::from(ORIGIN.1 + frame.height));
    let mut across = line(first_visible - 1);
    across.set_end(line(first_visible + 1).end());
    assert_eq!(across.bounding_boxes().len(), 3);
    // Margin rows lie where they would be drawn: a row's height apart.
    let (a, b) = (
        line(first_visible - 2).bounding_boxes(),
        line(first_visible - 1).bounding_boxes(),
    );
    assert_eq!(b[0].y0 - a[0].y0, f64::from(LINE));
}

#[test]
fn a_point_on_a_visible_character_is_that_character() {
    let display = display(&numbered(100));
    let (text, frame) = View::new(40, 10).build(&display);
    let tree = tree(&text);
    let state = tree.state();
    let editor = state.focus().unwrap();
    // "row 43": the "4", the fifth character, at its centre.
    let row = 43;
    let run = text.runs.iter().find(|run| run.display_row == row).unwrap();
    let x = run.bounds.x0 + f64::from(4.5 * CELL);
    let y = f64::from(ORIGIN.1 + frame.row_top(row) + LINE / 2.);
    let position = editor.text_position_at_point(Point { x, y });
    let mut range = position.to_degenerate_range();
    range.set_end(position.forward_to_character_end());
    assert_eq!(range.text(), "4");
    let line = text
        .runs
        .iter()
        .position(|run| run.display_row == row)
        .unwrap();
    assert_eq!(position.to_line_index(), line);
}

#[test]
fn a_screen_readers_selection_maps_to_buffer_offsets() {
    let display = wrapped("short\naaaa bbbb cccc\nend", 10);
    let text = View::new(0, 10).text(&display);
    let second = text.runs[2].key;
    assert_eq!(text.offset(second, 0), Some(16));
    assert_eq!(text.offset(second, 2), Some(18));
    // Past the end, the run's end.
    assert_eq!(text.offset(second, 99), Some(21));
    let missing = RunKey {
        buffer_row: 9,
        segment: 0,
    };
    assert_eq!(text.offset(missing, 0), None);
}

#[test]
fn run_ids_are_stable_across_a_scroll() {
    let display = display(&numbered(200));
    let one = View::new(50, 10).text(&display);
    let two = View::new(51, 10).text(&display);
    let keys = |text: &A11yText| text.runs.iter().map(|run| run.key).collect::<Vec<_>>();
    let (one, two) = (keys(&one), keys(&two));
    assert_eq!(
        one[1..],
        two[..two.len() - 1],
        "a row added and one removed"
    );
}

/// The editor's nodes, as the recorder sees them: the "Editor" group (its
/// focus div) and, inside it, the text area named for the file.
#[gpui::test]
fn the_editor_is_a_group_around_a_text_area_named_for_the_file(cx: &mut gpui::TestAppContext) {
    use fe_editor::EditorModel;

    use crate::{EditorTheme, EditorView};
    cx.update(crate::init);
    let path = std::path::PathBuf::from("/project/src/main.rs");
    let (_view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new("fn main() {}\n"), Some(path));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    let roles: Vec<(Role, String)> = (nodes.into_iter())
        .map(|node| (node.role, node.label.to_string()))
        .collect();
    let group = roles
        .iter()
        .position(|node| *node == (Role::Group, "Editor".into()));
    let area =
        (roles.iter()).position(|node| *node == (Role::MultilineTextInput, "main.rs".into()));
    assert!(group.is_some() && area.is_some(), "{roles:?}");
    assert!(
        group < area,
        "the group is drawn first, around the text area"
    );
}

/// A source that counts the bytes read from it, and its shares too.
#[derive(Clone)]
struct CountingSource {
    inner: Arc<dyn fe_text::TextSource>,
    read: Arc<std::sync::atomic::AtomicUsize>,
}

impl CountingSource {
    fn new(text: &str) -> Self {
        Self {
            inner: Arc::new(fe_text::Text::new(text)),
            read: Arc::default(),
        }
    }

    fn take(&self) -> usize {
        self.read.swap(0, std::sync::atomic::Ordering::SeqCst)
    }
}

impl fe_text::TextSource for CountingSource {
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn row_count(&self) -> u32 {
        self.inner.row_count()
    }

    fn row_start(&self, row: u32) -> usize {
        self.inner.row_start(row)
    }

    fn row_len(&self, row: u32) -> u32 {
        self.inner.row_len(row)
    }

    fn clip_offset(&self, offset: usize, bias: fe_text::Bias) -> usize {
        self.inner.clip_offset(offset, bias)
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        self.inner.is_char_boundary(offset)
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        self.inner.char_at(offset)
    }

    fn chunks_in_range(
        &self,
        range: Range<usize>,
    ) -> Box<dyn Iterator<Item = std::borrow::Cow<'_, str>> + '_> {
        let read = self.read.clone();
        Box::new(self.inner.chunks_in_range(range).inspect(move |chunk| {
            read.fetch_add(chunk.len(), std::sync::atomic::Ordering::SeqCst);
        }))
    }

    fn share(&self) -> Arc<dyn fe_text::TextSource> {
        Arc::new(self.clone())
    }
}

/// Every char a cell wide, counting what it's asked.
#[derive(Default)]
struct CountingWidths(usize);

impl CharWidth for CountingWidths {
    fn char_width(&mut self, _: char) -> f32 {
        self.0 += 1;
        CELL
    }
}

/// Builds `view` of `display`, counting the bytes `source` gives and the
/// chars measured while building only.
fn build_counting(
    view: &View,
    display: &DisplayMap,
    source: &CountingSource,
) -> (A11yText, usize, usize) {
    let mut long_rows = LongRows::default();
    let frame = view.frame(display, &mut long_rows);
    source.take();
    let mut widths = CountingWidths::default();
    let text = build(Input {
        display,
        frame: &frame,
        long_rows: &mut long_rows,
        widths: &mut widths,
        tab_size: 4,
        anchor: view.anchor,
        head: view.head,
        origin: ORIGIN,
        scale: 1.,
        cap: view.cap,
        read_only: false,
    });
    (text, source.take(), widths.0)
}

#[test]
fn a_huge_row_beside_the_view_is_neither_read_nor_measured() {
    let huge = "é".repeat(2 * 1024 * 1024);
    let source = CountingSource::new(&format!("one\ntwo\n{huge}\nfour"));
    let display = DisplayMap::read_only(Arc::new(source.clone()), 4);
    // Two rows shown: the margin's next row is 4 MiB.
    let (text, read, measured) = build_counting(&View::new(0, 2), &display, &source);
    assert!(
        text.runs.iter().all(|run| run.text.len() < 16),
        "not emitted"
    );
    assert!(read < 1024, "{read} bytes read");
    assert!(measured < 1024, "{measured} chars measured");
}

#[test]
fn a_huge_visible_row_is_read_and_measured_to_its_cut() {
    // Unicode, so the long-row index measures every char, not ASCII runs.
    let huge = "é".repeat(2 * 1024 * 1024);
    let source = CountingSource::new(&format!("{huge}\nnext"));
    let display = DisplayMap::read_only(Arc::new(source.clone()), 4);
    let (text, read, measured) = build_counting(&View::new(0, 3), &display, &source);
    let first = &text.runs[0];
    assert!(first.truncated);
    assert_eq!(first.text.len(), 64 * 1024);
    let chars = first.character_lengths.len();
    // Read and measured about once each, not from a checkpoint per char.
    assert!(read < 4 * 64 * 1024, "{read} bytes read");
    assert!(
        measured < 3 * chars,
        "{measured} measured for {chars} chars"
    );
    // Each char a cell, from the row's start.
    assert_eq!(first.positions[1000], 1000. * CELL);
    assert_eq!(first.widths[chars - 1], CELL);
}

#[test]
fn a_tab_heavy_row_is_placed_by_its_tab_stops() {
    let row: String = (0..2000).map(|_| "\ta").collect();
    let display = display(&row);
    let (text, frame) = View::new(0, 3).build(&display);
    let mut long_rows = LongRows::default();
    let run = &text.runs[0];
    // As the frame places each char, one at a time.
    let laid_out = &frame.rows[0];
    for (ix, offset) in [0, 1, 2, 3, 1001, 2 * 1024, 3999].into_iter().enumerate() {
        let x = laid_out.x_for_offset(offset, &mut long_rows, &mut Mono);
        assert_eq!(f64::from(run.positions[offset]), x, "char {offset} ({ix})");
    }
}

/// An editor view of `text`, drawn, with vim on if `vim`.
fn editor_view<'a>(
    text: &str,
    vim: bool,
    cx: &'a mut gpui::TestAppContext,
) -> (
    gpui::Entity<crate::EditorView>,
    &'a mut gpui::VisualTestContext,
) {
    use fe_editor::EditorModel;
    use gpui::Focusable as _;

    use crate::{EditorTheme, EditorView};
    cx.update(crate::init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| crate::set_vim_enabled(cx, vim));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

/// What the adapter would give a screen reader of `view`'s last frame.
fn adapter_text(
    view: &gpui::Entity<crate::EditorView>,
    cx: &mut gpui::VisualTestContext,
) -> A11yText {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |view, _| {
        let mut layout = view.layout.take().expect("drawn");
        let text = super::build_a11y_text(&mut layout, view, 1.);
        view.layout = Some(layout);
        text
    })
}

fn primary(
    view: &gpui::Entity<crate::EditorView>,
    cx: &mut gpui::VisualTestContext,
) -> ResolvedSelection {
    view.read_with(cx, |view, _| view.model().primary_selection())
}

#[gpui::test]
fn a_read_only_editor_is_read_only_without_vim(cx: &mut gpui::TestAppContext) {
    let (view, cx) = editor_view("let x = 1;\n", false, cx);
    assert!(!adapter_text(&view, cx).read_only);
    view.update(cx, |view, cx| view.set_read_only(true, cx));
    assert!(adapter_text(&view, cx).read_only);
    view.update(cx, |view, cx| view.set_read_only(false, cx));
    assert!(!adapter_text(&view, cx).read_only);
}

#[gpui::test]
fn a_screen_readers_caret_leaves_vims_visual_mode(cx: &mut gpui::TestAppContext) {
    let (view, cx) = editor_view("one two three\n", true, cx);
    assert!(adapter_text(&view, cx).read_only, "normal mode");
    cx.simulate_keystrokes("v l l");
    view.update(cx, |view, cx| view.select_from_a11y(8, 8, cx));
    cx.run_until_parked();
    // A frame's visual sync doesn't bring the old selection back.
    let text = adapter_text(&view, cx);
    let caret = primary(&view, cx);
    assert_eq!(caret.range, 8..8);
    assert_eq!(text.focus.1, 8);
    let mode = view.read_with(cx, |view, _| view.vim.as_ref().map(|vim| vim.mode_name()));
    assert_eq!(mode, Some("normal"));
}

#[gpui::test]
fn a_screen_readers_selection_survives_vims_visual_sync(cx: &mut gpui::TestAppContext) {
    let (view, cx) = editor_view("one two three\n", true, cx);
    cx.simulate_keystrokes("v l");
    view.update(cx, |view, cx| view.select_from_a11y(4, 7, cx));
    cx.run_until_parked();
    let text = adapter_text(&view, cx);
    assert_eq!(primary(&view, cx).range, 4..7);
    assert_eq!((text.anchor.1, text.focus.1), (4, 7));
}

#[gpui::test]
fn a_screen_readers_caret_drops_a_pending_operator(cx: &mut gpui::TestAppContext) {
    let (view, cx) = editor_view("one two three\n", true, cx);
    cx.simulate_keystrokes("d");
    view.update(cx, |view, cx| view.select_from_a11y(8, 8, cx));
    cx.run_until_parked();
    // `w` is a motion now, not `dw`.
    cx.simulate_keystrokes("w");
    let text = view.read_with(cx, |view, _| view.model().text().to_string());
    assert_eq!(text, "one two three\n");
    assert_eq!(primary(&view, cx).range, 14..14, "on the empty last line");
}
