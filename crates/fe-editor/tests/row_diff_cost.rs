//! Row diffs of thousands of interleaved changes stay cheap, and the
//! algorithm choice (`row_diff`) is by the size of the changed region.

use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_editor::row_changes::RowChangeKind;
use fe_editor::{EditorModel, TextDiff};
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

struct Rows<'a>(Vec<&'a str>);

impl<'a> TokenSource for Rows<'a> {
    type Token = &'a str;
    type Tokenizer = std::vec::IntoIter<&'a str>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.clone().into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        self.0.len() as u32
    }
}

fn rows(text: &str) -> Rows<'_> {
    Rows(text.split('\n').collect())
}

/// `TextDiff` hunks as (left, right) ranges of `algorithm` plus
/// `postprocess_lines`.
fn expected(
    left: &str,
    right: &str,
    algorithm: Algorithm,
) -> Vec<(std::ops::Range<u32>, std::ops::Range<u32>)> {
    let input = InternedInput::new(rows(left), rows(right));
    let mut diff = Diff::compute(algorithm, &input);
    diff.postprocess_lines(&input);
    diff.hunks().map(|h| (h.before, h.after)).collect()
}

fn actual(left: &str, right: &str) -> Vec<(std::ops::Range<u32>, std::ops::Range<u32>)> {
    TextDiff::new(left, right)
        .hunks()
        .iter()
        .map(|h| (h.left.clone(), h.right.clone()))
        .collect()
}

/// `n` rows of repeated braces, blanks and a few distinct rows, and the same
/// rows with blocks swapped: Histogram and Myers match them differently.
fn shuffled(n: usize, seed: u64) -> (String, String) {
    let mut state = seed;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    let toks = ["}", "", "{", "else", "x", "y"];
    let left: Vec<String> = (0..n)
        .map(|i| {
            if next() % 3 == 0 {
                toks[next() % 6].to_string()
            } else {
                (i % 40).to_string()
            }
        })
        .collect();
    let mut right = left.clone();
    for _ in 0..n / 8 {
        right.swap(next() % n, next() % n);
    }
    (left.join("\n"), right.join("\n"))
}

/// A shuffled input of `n` rows on which the two algorithms differ.
fn differing(n: usize) -> (String, String) {
    (0..)
        .map(|seed| shuffled(n, seed))
        .find(|(l, r)| expected(l, r, Algorithm::Histogram) != expected(l, r, Algorithm::Myers))
        .unwrap()
}

#[test]
fn small_regions_keep_histogram_hunks() {
    let (left, right) = differing(600);
    assert_eq!(
        actual(&left, &right),
        expected(&left, &right, Algorithm::Histogram)
    );
}

#[test]
fn large_regions_use_myers() {
    // 1,500 shuffled rows: a region of about 1,500 x 3,000 rows, over the
    // bound of 1 << 21.
    let (left, right) = differing(1500);
    assert_eq!(
        actual(&left, &right),
        expected(&left, &right, Algorithm::Myers)
    );
}

#[test]
fn interleaved_changes_diff_in_linear_time() {
    const ROWS: usize = 20_000;
    let left: Vec<String> = (0..ROWS).map(|i| i.to_string()).collect();
    let right: Vec<String> = (0..ROWS)
        .map(|i| {
            if i % 2 == 1 {
                format!("x{i}")
            } else {
                i.to_string()
            }
        })
        .collect();
    let (left, right) = (left.join("\n"), right.join("\n"));

    let start = Instant::now();
    let diff = TextDiff::new(&left, &right);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(diff.hunks().len(), ROWS / 2);
    let all: Vec<usize> = (0..diff.hunks().len()).collect();
    assert_eq!(diff.apply(&left, &right, &all), right);

    let mut model = EditorModel::new(Buffer::new(&right), None);
    let start = Instant::now();
    model.set_diff_base(Some(left.as_str().into()));
    let changes = model.row_changes();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(changes.len(), ROWS / 2);
    assert!(changes.iter().all(|c| c.kind == RowChangeKind::Modified));
}
