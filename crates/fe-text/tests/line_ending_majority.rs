//! A file's line ending is the majority of its breaks, as in VS Code.

use fe_text::{LineEnding, Text};

fn endings(bytes: &[u8]) -> [LineEnding; 3] {
    let text = std::str::from_utf8(bytes).unwrap();
    let (_, from_contents) = Text::from_file_contents(text);
    let (_, from_reader) = Text::from_reader(bytes).unwrap();
    let from_detecting = Text::from_reader_detecting(bytes).unwrap().line_ending;
    [from_contents, from_reader, from_detecting]
}

fn check(bytes: &[u8], expected: LineEnding) {
    assert_eq!(
        endings(bytes),
        [expected; 3],
        "{:?}",
        String::from_utf8_lossy(&bytes[bytes.len().min(70_000)..])
    );
}

fn with_head(x: usize, tail: &str) -> Vec<u8> {
    let mut bytes = vec![b'x'; x];
    bytes.extend_from_slice(tail.as_bytes());
    bytes
}

#[test]
fn line_ending_follows_the_majority_of_breaks() {
    check(b"a\nb\r\nc\r\nd\r\n", LineEnding::CrLf);
    check(b"a\r\nb\nc\nd\n", LineEnding::Lf);
    check(b"a\rb\rc\r", LineEnding::CrLf);
    check(b"abc", LineEnding::Lf);

    let normalised = Text::from_reader(&b"a\nb\r\nc\rd"[..]).unwrap().0;
    assert_eq!(normalised.to_string(), "a\nb\nc\nd");

    // One LF first, then 70,000 break-free bytes: every CRLF is past the first
    // 64 KiB read of either reader.
    let mut big = b"a\n".to_vec();
    big.resize(2 + 70_000, b'x');
    for _ in 0..40_000 {
        big.extend_from_slice(b"line\r\n");
    }
    assert!(big.len() > 200_000);
    check(&big, LineEnding::CrLf);

    for x in [65_535, 65_536] {
        check(&with_head(x, "\r\na\n"), LineEnding::Lf);
        check(&with_head(x, "\r\n\r\na\n"), LineEnding::CrLf);
    }
}
