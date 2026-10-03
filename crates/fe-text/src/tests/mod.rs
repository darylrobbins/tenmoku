use proptest::prelude::*;

use super::*;

mod large;

#[test]
fn round_trips() {
    let text = Text::new("fn main() {}\n");
    assert_eq!(text.len(), 13);
    assert_eq!(text.to_string(), "fn main() {}\n");
    assert_eq!(text.row_count(), 2);
    assert_eq!(text.max_point(), Point::new(1, 0));
}

#[test]
fn normalises_line_endings() {
    let (text, ending) = Text::from_file_contents("a\r\nb\r\n");
    assert_eq!(ending, LineEnding::CrLf);
    assert_eq!(text.to_string(), "a\nb\n");

    let mut saved = Vec::new();
    text.write_to(ending, &mut saved).unwrap();
    assert_eq!(saved, b"a\r\nb\r\n");

    let (text, ending) = Text::from_file_contents("a\rb\n");
    assert_eq!(ending, LineEnding::Lf);
    assert_eq!(text.to_string(), "a\nb\n");
}

#[test]
fn utf16_points_handle_astral_chars() {
    // '😀' is 4 bytes and 2 UTF-16 units.
    let text = Text::new("a😀b\nc");
    assert_eq!(text.offset_to_point_utf16(5), PointUtf16::new(0, 3));
    assert_eq!(text.point_utf16_to_offset(PointUtf16::new(0, 3)), 5);
    // Column inside the surrogate pair falls back to the char start.
    assert_eq!(text.point_utf16_to_offset(PointUtf16::new(0, 2)), 1);
    // Column past the end of the row clamps to the row end.
    assert_eq!(text.point_utf16_to_offset(PointUtf16::new(0, 99)), 6);
    assert_eq!(text.point_utf16_to_offset(PointUtf16::new(1, 1)), 8);
    // Row past the end clamps to the end of the text.
    assert_eq!(text.point_utf16_to_offset(PointUtf16::new(9, 0)), 8);
}

#[test]
fn points_clamp() {
    let text = Text::new("héllo\nworld");
    assert_eq!(
        text.point_to_offset(Point::new(0, 2)),
        1,
        "inside 'é' clamps left"
    );
    assert_eq!(text.point_to_offset(Point::new(0, 99)), 6);
    assert_eq!(text.point_to_offset(Point::new(1, 2)), 9);
    assert_eq!(text.row_to_string(1), "world");
}

#[test]
fn range_eq_spans_chunks() {
    let source = "abcdefghij".repeat(10_000);
    let text = Text::new(&source);
    assert!(text.range_eq(500..90_000, &source[500..90_000]));
    assert!(!text.range_eq(500..90_000, &source[501..90_001]));
}

fn arb_text() -> impl Strategy<Value = String> {
    // Mix ASCII, multi-byte, astral chars and newlines.
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::char::range('a', 'z'),
            1 => Just('\n'),
            1 => Just('é'),
            1 => Just('中'),
            1 => Just('😀'),
        ],
        0..200,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

/// Reference implementation over a plain `String`.
fn reference_point(text: &str, offset: usize) -> (Point, PointUtf16) {
    let before = &text[..offset];
    let row = before.matches('\n').count() as u32;
    let row_start = before.rfind('\n').map_or(0, |ix| ix + 1);
    let column = (offset - row_start) as u32;
    let column_utf16 = text[row_start..offset].encode_utf16().count() as u32;
    (Point::new(row, column), PointUtf16::new(row, column_utf16))
}

proptest! {
    #[test]
    fn conversions_match_reference(source in arb_text()) {
        let text = Text::new(&source);
        prop_assert_eq!(text.to_string(), source.clone());
        prop_assert_eq!(text.row_count() as usize, source.matches('\n').count() + 1);
        prop_assert_eq!(text.len_utf16(), source.encode_utf16().count());

        for (offset, _) in source.char_indices().chain([(source.len(), ' ')]) {
            let (point, point_utf16) = reference_point(&source, offset);
            prop_assert_eq!(text.offset_to_point(offset), point);
            prop_assert_eq!(text.point_to_offset(point), offset);
            prop_assert_eq!(text.offset_to_point_utf16(offset), point_utf16);
            prop_assert_eq!(text.point_utf16_to_offset(point_utf16), offset);
            let utf16 = source[..offset].encode_utf16().count();
            prop_assert_eq!(text.offset_to_utf16(offset), utf16);
            prop_assert_eq!(text.utf16_to_offset(utf16), offset);
        }
    }

    #[test]
    fn replace_matches_string(source in arb_text(), insert in arb_text(), a in 0usize..400, b in 0usize..400) {
        let mut expected = source.clone();
        let clip = |ix: usize| {
            let mut ix = ix.min(expected.len());
            while !expected.is_char_boundary(ix) { ix -= 1; }
            ix
        };
        let (start, end) = (clip(a.min(b)), clip(a.max(b)));
        let mut text = Text::new(&source);
        text.replace(start..end, &insert);
        expected.replace_range(start..end, &insert);
        prop_assert_eq!(text.to_string(), expected);
    }
}

/// Hands out its bytes a few at a time, to split sequences across reads.
struct TrickleReader<'a> {
    bytes: &'a [u8],
    step: usize,
}

impl std::io::Read for TrickleReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.step.min(self.bytes.len()).min(buf.len());
        buf[..len].copy_from_slice(&self.bytes[..len]);
        self.bytes = &self.bytes[len..];
        Ok(len)
    }
}

#[test]
fn from_reader_handles_large_input() {
    let source = "line one\r\nline two é\r\n".repeat(20_000);
    let (text, ending) = Text::from_reader(source.as_bytes()).unwrap();
    assert_eq!(ending, LineEnding::CrLf);
    assert_eq!(text.to_string(), source.replace("\r\n", "\n"));
}

proptest! {
    #[test]
    fn from_reader_matches_lossy_decoding(
        bytes in proptest::collection::vec(
            prop_oneof![
                4 => proptest::num::u8::ANY,
                2 => Just(b'\r'),
                2 => Just(b'\n'),
                2 => Just(b'a'),
            ],
            0..300,
        ),
        step in 1usize..8,
    ) {
        let lossy = String::from_utf8_lossy(&bytes);
        let (expected, expected_ending) = Text::from_file_contents(&lossy);
        let (text, ending) = Text::from_reader(TrickleReader { bytes: &bytes, step }).unwrap();
        prop_assert_eq!(text.to_string(), expected.to_string());
        prop_assert_eq!(ending, expected_ending);
    }
}

#[test]
fn line_ending_strings() {
    assert_eq!(LineEnding::Lf.as_str(), "\n");
    assert_eq!(LineEnding::CrLf.as_str(), "\r\n");
}

#[test]
fn emptiness_and_char_boundaries() {
    assert!(Text::new("").is_empty());
    assert!(!Text::new("a").is_empty());
    let text = Text::new("a😀b");
    assert!(text.is_char_boundary(1));
    assert!(!text.is_char_boundary(2), "inside the emoji");
    assert!(!text.is_char_boundary(4));
    assert!(text.is_char_boundary(5));
}

#[test]
fn rows_past_the_end_clamp_to_the_last_row() {
    let text = Text::new("one\ntwo\nthree");
    assert_eq!(text.row_start(2), 8);
    assert_eq!(text.row_start(3), 8);
    assert_eq!(text.row_start(u32::MAX), 8);
    assert_eq!(text.row_len(99), 5);
}

#[test]
fn debug_shows_the_text() {
    assert_eq!(format!("{:?}", Text::new("a\nb")), "\"a\\nb\"");
}

/// Fails its first read with `Interrupted`, as signals can cause.
struct InterruptedOnce<'a> {
    interrupted: bool,
    bytes: &'a [u8],
}

impl std::io::Read for InterruptedOnce<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let len = self.bytes.len().min(buf.len());
        buf[..len].copy_from_slice(&self.bytes[..len]);
        self.bytes = &self.bytes[len..];
        Ok(len)
    }
}

/// Fails every read.
struct Broken;

impl std::io::Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::PermissionDenied.into())
    }
}

#[test]
fn from_reader_retries_interrupted_reads_and_reports_errors() {
    let reader = InterruptedOnce {
        interrupted: false,
        bytes: b"retried\n",
    };
    let (text, _) = Text::from_reader(reader).unwrap();
    assert_eq!(text.to_string(), "retried\n");
    let error = Text::from_reader(Broken).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}
