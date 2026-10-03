//! Differential tests on texts large enough to span many rope leaves, so
//! chunk boundaries, tree rebalancing and the line/UTF-16 indexes are all
//! exercised. Every query is checked against a plain `String` with
//! precomputed indexes.

use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::*;

/// A `String` plus the indexes needed to answer `Text` queries quickly.
struct Reference {
    text: String,
    row_starts: Vec<usize>,
    /// UTF-16 length of `text[..ix]` at every char boundary `ix`.
    utf16_at: std::collections::HashMap<usize, usize>,
}

impl Reference {
    fn new(text: String) -> Self {
        let mut row_starts = vec![0];
        let mut utf16_at = std::collections::HashMap::with_capacity(text.len() + 1);
        let mut utf16 = 0;
        for (ix, ch) in text.char_indices() {
            utf16_at.insert(ix, utf16);
            utf16 += ch.len_utf16();
            if ch == '\n' {
                row_starts.push(ix + 1);
            }
        }
        utf16_at.insert(text.len(), utf16);
        Self {
            text,
            row_starts,
            utf16_at,
        }
    }

    fn row_end(&self, row: usize) -> usize {
        self.row_starts
            .get(row + 1)
            .map_or(self.text.len(), |next| next - 1)
    }

    fn point(&self, offset: usize) -> Point {
        let row = self.row_starts.partition_point(|&start| start <= offset) - 1;
        Point::new(row as u32, (offset - self.row_starts[row]) as u32)
    }

    fn point_utf16(&self, offset: usize) -> PointUtf16 {
        let point = self.point(offset);
        let row_start = self.row_starts[point.row as usize];
        PointUtf16::new(
            point.row,
            (self.utf16_at[&offset] - self.utf16_at[&row_start]) as u32,
        )
    }

    fn boundary(&self, rng: &mut impl Rng) -> usize {
        let mut ix = rng.random_range(0..=self.text.len());
        while !self.text.is_char_boundary(ix) {
            ix -= 1;
        }
        ix
    }
}

/// Up to `max` bytes of text with the shapes that stress an editor: long and
/// empty rows, runs of multi-byte and astral characters, and long ASCII
/// stretches.
fn random_text(rng: &mut impl Rng, max: usize) -> String {
    let target = rng.random_range(0..=max);
    const PIECES: &[&str] = &[
        "fn main() { println!(\"hello\"); }\n",
        "    let x = 42;\n",
        "\n",
        "\n\n\n",
        "héllo wörld ",
        "中文字符",
        "😀🎉👍🏽",
        "e\u{301}",
        "\t\tindented\n",
    ];
    let mut text = String::with_capacity(target + 64);
    while text.len() < target {
        if rng.random_bool(0.02) {
            // A very long row.
            text.extend(std::iter::repeat_n('x', rng.random_range(1_000..20_000)));
        } else {
            text.push_str(PIECES[rng.random_range(0..PIECES.len())]);
        }
    }
    text
}

fn check_all(text: &Text, reference: &Reference, rng: &mut impl Rng) -> Result<(), TestCaseError> {
    let source = &reference.text;
    prop_assert_eq!(text.len(), source.len());
    prop_assert_eq!(text.is_empty(), source.is_empty());
    prop_assert_eq!(text.len_utf16(), reference.utf16_at[&source.len()]);
    prop_assert_eq!(text.row_count() as usize, reference.row_starts.len());
    prop_assert_eq!(text.max_point(), reference.point(source.len()));

    // Whole-text iteration, which crosses every leaf boundary.
    let mut joined = String::with_capacity(source.len());
    for chunk in text.chunks() {
        prop_assert!(!chunk.is_empty() || source.is_empty());
        joined.push_str(chunk);
    }
    prop_assert!(joined == *source, "chunks don't reassemble the text");

    for _ in 0..40 {
        let offset = reference.boundary(rng);
        let point = reference.point(offset);
        prop_assert_eq!(text.offset_to_point(offset), point);
        prop_assert_eq!(text.point_to_offset(point), offset);
        let point_utf16 = reference.point_utf16(offset);
        prop_assert_eq!(text.offset_to_point_utf16(offset), point_utf16);
        prop_assert_eq!(text.point_utf16_to_offset(point_utf16), offset);
        let utf16 = reference.utf16_at[&offset];
        prop_assert_eq!(text.offset_to_utf16(offset), utf16);
        prop_assert_eq!(text.utf16_to_offset(utf16), offset);
        prop_assert_eq!(text.char_at(offset), source[offset..].chars().next());
        prop_assert!(text.is_char_boundary(offset));

        // Clipping a byte offset inside a character.
        let raw = (offset + 1).min(source.len());
        let mut left = raw;
        while !source.is_char_boundary(left) {
            left -= 1;
        }
        let mut right = raw;
        while !source.is_char_boundary(right) {
            right += 1;
        }
        prop_assert_eq!(text.clip_offset(raw, Bias::Left), left);
        prop_assert_eq!(text.clip_offset(raw, Bias::Right), right);

        // Ranges, which often span leaves.
        let other = reference.boundary(rng);
        let range = offset.min(other)..offset.max(other);
        let slice = &source[range.clone()];
        prop_assert!(text.slice_to_string(range.clone()) == slice);
        prop_assert!(text.range_eq(range.clone(), slice));
        let joined: String = text.chunks_in_range(range.clone()).collect();
        prop_assert!(joined == slice);
        if !slice.is_empty() {
            let mut different = slice.to_string();
            let last = different.pop().unwrap();
            different.push(if last == 'z' { 'y' } else { 'z' });
            prop_assert!(!text.range_eq(range.clone(), &different));
        }

        let row = rng.random_range(0..reference.row_starts.len());
        prop_assert_eq!(text.row_start(row as u32), reference.row_starts[row]);
        let row_len = reference.row_end(row) - reference.row_starts[row];
        prop_assert_eq!(text.row_len(row as u32) as usize, row_len);
        // Columns past the end of a row clamp to it.
        let past_end = Point::new(row as u32, row_len as u32 + 5);
        prop_assert_eq!(text.point_to_offset(past_end), reference.row_end(row));
    }
    Ok(())
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .map_or(24, |cases: u32| (cases / 10).max(1))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn large_text_matches_reference_through_edits(seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let size = if cfg!(debug_assertions) { 60_000 } else { 400_000 };
        let initial = random_text(&mut rng, size);
        let mut text = Text::new(&initial);
        let mut reference = Reference::new(initial);
        check_all(&text, &reference, &mut rng)?;

        for _ in 0..30 {
            let a = reference.boundary(&mut rng);
            let b = reference.boundary(&mut rng);
            let range = a.min(b)..a.max(b);
            // Mostly small edits; sometimes huge deletions or insertions.
            let insert = match rng.random_range(0..10) {
                0 => random_text(&mut rng, 50_000),
                1 => String::new(),
                _ => random_text(&mut rng, 40),
            };
            let range = if rng.random_bool(0.7) {
                // Keep most deletions short so the text doesn't collapse.
                range.start..reference.text.len().min(range.start + (range.len() % 200))
            } else {
                range
            };
            let mut end = range.end;
            while !reference.text.is_char_boundary(end) {
                end -= 1;
            }
            let range = range.start..end.max(range.start);
            text.replace(range.clone(), &insert);
            let mut source = std::mem::take(&mut reference.text);
            source.replace_range(range, &insert);
            reference = Reference::new(source);
            check_all(&text, &reference, &mut rng)?;
        }

        // Snapshots are unaffected by later edits.
        let snapshot = text.clone();
        let before = snapshot.to_string();
        text.replace(0..text.len(), "gone");
        prop_assert_eq!(snapshot.to_string(), before);
    }

    #[test]
    fn large_crlf_round_trips(seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let lf = random_text(&mut rng, 200_000);
        let crlf = lf.replace('\n', "\r\n");
        let (text, ending) = Text::from_file_contents(&crlf);
        prop_assert_eq!(text.to_string(), lf.clone());
        if lf.contains('\n') {
            prop_assert_eq!(ending, LineEnding::CrLf);
        }
        let mut saved = Vec::new();
        text.write_to(LineEnding::CrLf, &mut saved).unwrap();
        prop_assert!(saved == crlf.as_bytes());

        let (streamed, _) = Text::from_reader(crlf.as_bytes()).unwrap();
        prop_assert!(streamed.to_string() == lf);
    }
}
