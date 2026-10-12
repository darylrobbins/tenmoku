//! Caret moves on rows over 1 MiB read only a window of the row. Each is
//! compared with an oracle that does what the code did before: segment or
//! scan the whole row.

use std::ops::Range;

use fe_buffer::Buffer;
use fe_editor::{EditorModel, Movement};
use unicode_segmentation::UnicodeSegmentation;

/// How far either side of the caret a grapheme move first reads.
const WINDOW: usize = 1024;
const FILLER: &str = "lorem ipsum_ dolor, sit-amet ";

fn filler(len: usize) -> String {
    FILLER.chars().cycle().take(len).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Whitespace,
    Word,
    Punctuation,
}

fn class(ch: char) -> Class {
    if ch.is_whitespace() {
        Class::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        Class::Word
    } else {
        Class::Punctuation
    }
}

/// A row, with its model and the whole-row oracle.
struct Fixture {
    model: EditorModel,
    row: String,
    boundaries: Vec<usize>,
}

impl Fixture {
    fn new(row: String) -> Self {
        assert!(row.len() > 1 << 20);
        // Read as from disk, so the row ends in a CRLF that loading turns
        // into a line break, right where the last window edge falls.
        let text = format!("{row}\r\nnext\r\n");
        let boundaries = row.grapheme_indices(true).map(|(ix, _)| ix).collect();
        Fixture {
            model: EditorModel::new(Buffer::from_file_contents(&text), None),
            row,
            boundaries,
        }
    }

    fn moved(&mut self, at: usize, movement: Movement) -> usize {
        self.model.set_cursor(at);
        self.model.move_cursors(movement, false);
        self.model.primary_selection().range.start
    }

    fn prev_grapheme(&self, col: usize) -> usize {
        if col == 0 {
            return 0;
        }
        self.boundaries[self.boundaries.partition_point(|&ix| ix < col) - 1]
    }

    fn next_grapheme(&self, col: usize) -> usize {
        let i = self.boundaries.partition_point(|&ix| ix <= col);
        self.boundaries.get(i).copied().unwrap_or(self.row.len())
    }

    fn next_word(&self, col: usize) -> usize {
        let mut chars = self.row[col..].chars().peekable();
        let mut col = col;
        while let Some(&ch) = chars.peek()
            && ch.is_whitespace()
        {
            col += ch.len_utf8();
            chars.next();
        }
        if let Some(&first) = chars.peek() {
            while let Some(&ch) = chars.peek()
                && class(ch) == class(first)
            {
                col += ch.len_utf8();
                chars.next();
            }
        }
        col
    }

    fn prev_word(&self, col: usize) -> usize {
        let mut chars = self.row[..col].chars().rev().peekable();
        let mut col = col;
        while let Some(&ch) = chars.peek()
            && ch.is_whitespace()
        {
            col -= ch.len_utf8();
            chars.next();
        }
        if let Some(&first) = chars.peek() {
            while let Some(&ch) = chars.peek()
                && class(ch) == class(first)
            {
                col -= ch.len_utf8();
                chars.next();
            }
        }
        col
    }

    fn word_range(&self, col: usize) -> Range<usize> {
        let row = &self.row;
        let class_at = |ix: usize| row[ix..].chars().next().map(class);
        let prev = row[..col].char_indices().next_back().map(|(ix, _)| ix);
        let prev_class = prev.and_then(class_at);
        let mut col = col;
        let class = match (class_at(col), prev_class) {
            (None | Some(Class::Whitespace), Some(prev_class))
                if prev_class != Class::Whitespace =>
            {
                col = prev.unwrap();
                prev_class
            }
            (Some(class), _) => class,
            (None, Some(prev_class)) => prev_class,
            (None, None) => return col..col,
        };
        let mut start = col;
        while let Some((ix, ch)) = row[..start].char_indices().next_back()
            && self::class(ch) == class
        {
            start = ix;
        }
        let end = row[col..]
            .char_indices()
            .find(|&(_, ch)| self::class(ch) != class)
            .map_or(row.len(), |(ix, _)| col + ix);
        start..end
    }

    fn indent(&self) -> usize {
        self.row.len() - self.row.trim_start().len()
    }

    /// Checks every move at each of `cols`, which are in the row.
    fn check(&mut self, cols: impl IntoIterator<Item = usize>) {
        for col in cols {
            if !self.row.is_char_boundary(col) {
                continue;
            }
            let len = self.row.len();
            let at = |what: &str, got: usize, want: usize| {
                assert_eq!(got, want, "{what} at {col}");
            };
            let want = if col == 0 { 0 } else { self.prev_grapheme(col) };
            let got = self.moved(col, Movement::Left);
            at("left", got, want);
            if col < len {
                let want = self.next_grapheme(col);
                let got = self.moved(col, Movement::Right);
                at("right", got, want);
                let want = self.next_word(col);
                let got = self.moved(col, Movement::WordRight);
                at("word right", got, want);
            }
            if col > 0 {
                let want = self.prev_word(col);
                let got = self.moved(col, Movement::WordLeft);
                at("word left", got, want);
            }
            let indent = self.indent();
            let want = if col == indent { 0 } else { indent };
            let got = self.moved(col, Movement::LineStart);
            at("line start", got, want);
            let got = self.model.word_range_at(col);
            assert_eq!(got, self.word_range(col), "word range at {col}");
        }
    }
}

/// Offsets within a few bytes of each window edge of a caret placed at each
/// of `anchors`: that is, carets whose windows end at the anchor.
fn near_edges(anchors: &[usize]) -> Vec<usize> {
    let mut cols = Vec::new();
    for &anchor in anchors {
        for edge in [anchor.saturating_sub(WINDOW), anchor + WINDOW, anchor] {
            cols.extend(edge.saturating_sub(4)..=edge + 4);
        }
    }
    cols
}

/// Where each special run starts in the row `special_row` builds.
struct Special {
    row: String,
    runs: Vec<Range<usize>>,
}

fn special_row() -> Special {
    let runs_text = [
        // A ZWJ family emoji.
        "👨\u{200d}👩\u{200d}👧\u{200d}👦",
        // A run of combining marks.
        "e\u{301}\u{302}\u{303}\u{304}\u{316}\u{317}\u{318}",
        // Regional indicators: three flags, then an odd one out.
        "🇨🇦🇺🇸🇫🇷🇩",
        "x🇨🇦🇺🇸🇫🇷🇩🇪",
        // A cluster over 2 KiB.
        &format!("a{}", "\u{301}".repeat(1500)),
        // A word run longer than the window.
        &"x".repeat(3000),
    ];
    let mut row = String::new();
    let mut runs = Vec::new();
    for run in runs_text {
        row += &filler(2300 + runs.len() * 7);
        runs.push(row.len()..row.len() + run.len());
        row += run;
    }
    row += &filler((1 << 20) + 5000 - row.len());
    Special { row, runs }
}

#[test]
fn moves_near_window_edges_match_the_whole_row() {
    let Special { row, runs } = special_row();
    let mut fixture = Fixture::new(row);
    let mut cols = Vec::new();
    for run in &runs {
        // Carets whose window edge falls anywhere in the run, and the run's
        // own edges.
        let step = if run.len() > 200 { 97 } else { 1 };
        for edge in (run.start..=run.end).step_by(step) {
            cols.extend(near_edges(&[edge]));
        }
        cols.extend(run.start.saturating_sub(8)..run.end + 8);
        cols.extend(run.clone().step_by(13));
    }
    cols.extend(0..16);
    let end = fixture.row.len();
    cols.extend(end - 1100..=end);
    cols.sort_unstable();
    cols.dedup();
    fixture.check(cols);
}

#[test]
fn a_cluster_over_2_kib_ends_where_the_whole_row_says() {
    let Special { row, runs } = special_row();
    let cluster = runs[4].clone();
    let mut fixture = Fixture::new(row);
    assert_eq!(fixture.next_grapheme(cluster.start), cluster.end);
    assert_eq!(fixture.moved(cluster.start, Movement::Right), cluster.end);
    assert_eq!(fixture.moved(cluster.end, Movement::Left), cluster.start);
    assert_eq!(
        fixture.moved(cluster.start + 1500, Movement::Left),
        cluster.start
    );
}

#[test]
fn a_word_run_longer_than_the_window_is_one_word() {
    let Special { row, runs } = special_row();
    let run = runs[5].clone();
    let mut fixture = Fixture::new(row);
    // The run's neighbours in the filler are word characters too.
    let word = fixture.word_range(run.start + 1500);
    assert!(word.start <= run.start && run.end <= word.end);
    for col in [run.start, run.start + 1, run.start + 1500, run.end - 1] {
        assert_eq!(fixture.model.word_range_at(col), word, "{col}");
    }
    assert_eq!(fixture.moved(word.start, Movement::WordRight), word.end);
    assert_eq!(fixture.moved(word.end, Movement::WordLeft), word.start);
}

#[test]
fn line_start_and_newline_indent_match_the_whole_row() {
    let indent = 2500;
    let row = format!("{}{}", " ".repeat(indent), filler((1 << 20) + 100));
    let mut fixture = Fixture::new(row.clone());
    fixture.check(near_edges(&[0, indent]));
    for col in near_edges(&[0, indent]).into_iter().step_by(3) {
        let mut model = EditorModel::new(Buffer::new(&format!("{row}\nnext\n")), None);
        model.set_cursor(col);
        model.newline();
        let text = model.text();
        let start = text.row_start(1);
        let expected = row[..col.min(indent)].to_string();
        assert_eq!(
            text.row_len(1) as usize,
            expected.len() + row.len() - col,
            "at {col}"
        );
        assert_eq!(
            text.slice_to_string(start..start + expected.len()),
            expected,
            "at {col}"
        );
    }
}
