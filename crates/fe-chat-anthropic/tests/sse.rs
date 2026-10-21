use std::path::PathBuf;

use fe_chat_anthropic::{SseEvent, SseParser};

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut fixtures: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sse"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).expect("fixture"))
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn parse(chunks: &[&[u8]]) -> Vec<SseEvent> {
    let mut parser = SseParser::new();
    chunks.iter().flat_map(|chunk| parser.push(chunk)).collect()
}

fn event(event: &str, data: &str) -> SseEvent {
    SseEvent {
        event: event.to_owned(),
        data: data.to_owned(),
    }
}

/// `bytes` with each LF replaced by `line_end`.
fn with_line_ends(bytes: &[u8], line_end: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .flat_map(|&byte| {
            if byte == b'\n' {
                line_end.to_vec()
            } else {
                vec![byte]
            }
        })
        .collect()
}

#[test]
fn sse_parses_across_any_split() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 8, "every fixture is checked");
    let mut split_in_crlf = 0;
    let mut split_in_utf8 = 0;
    for (name, lf) in &fixtures {
        let whole = parse(&[lf]);
        let event_lines = lf.split(|&byte| byte == b'\n');
        let declared = event_lines
            .filter(|line| line.starts_with(b"event:"))
            .count();
        assert_eq!(whole.len(), declared, "{name}: one event per `event:` line");
        for event in &whole {
            serde_json::from_str::<serde_json::Value>(&event.data)
                .unwrap_or_else(|error| panic!("{name}: {:?} isn't JSON: {error}", event.data));
            assert_ne!(event.event, "message", "{name}: every event is named");
        }

        for line_end in [&b"\n"[..], b"\r\n", b"\r"] {
            let bytes = with_line_ends(lf, line_end);
            let label = format!("{name} with {line_end:?} line ends");
            assert_eq!(parse(&[&bytes]), whole, "{label}, whole");
            let bytes_one_at_a_time: Vec<&[u8]> = bytes.chunks(1).collect();
            assert_eq!(
                parse(&bytes_one_at_a_time),
                whole,
                "{label}, a byte at a time"
            );
            for split in 1..bytes.len() {
                let (head, tail) = bytes.split_at(split);
                assert_eq!(parse(&[head, tail]), whole, "{label}, split at {split}");
                if head.ends_with(b"\r") && tail.starts_with(b"\n") {
                    split_in_crlf += 1;
                }
                if (tail[0] & 0b1100_0000) == 0b1000_0000 {
                    split_in_utf8 += 1;
                }
            }
        }
    }
    assert!(split_in_crlf > 0, "some splits fall inside a CRLF");
    assert!(
        split_in_utf8 > 0,
        "some splits fall inside a UTF-8 sequence"
    );
    let text = &fixtures
        .iter()
        .find(|(name, _)| name == "text.sse")
        .unwrap()
        .1;
    let multibyte = parse(&[text]).iter().any(|event| event.data.contains('✓'));
    assert!(multibyte, "a multi-byte character survives");

    // Multi-line data joins with a newline, and an empty `data:` still
    // counts as data.
    assert_eq!(
        parse(&[b"event: a\ndata: one\ndata:two\ndata\n\n"]),
        [event("a", "one\ntwo\n")]
    );
    assert_eq!(parse(&[b"data:\n\n"]), [event("message", "")]);
    // Comments, `id`, `retry` and unknown fields are ignored, and a field
    // value loses one leading space only.
    assert_eq!(
        parse(&[
            b": a comment\nid: 7\nretry: 1000\nevent: b\nfoo: bar\nbaz\ndata:  two spaces\n\n"
        ]),
        [event("b", " two spaces")]
    );
    // A blank line with no data dispatches nothing, and forgets the event.
    assert_eq!(
        parse(&[b"event: c\n\ndata: d\n\n"]),
        [event("message", "d")]
    );
    // An event isn't dispatched until its blank line; a BOM is dropped.
    let mut parser = SseParser::new();
    assert!(
        parser
            .push("\u{feff}event: e\r\ndata: f\r\n".as_bytes())
            .is_empty()
    );
    assert_eq!(parser.push(b"\r\n"), [event("e", "f")]);
    // Invalid UTF-8 is replaced rather than an error.
    assert_eq!(parse(&[b"data: \xff\n\n"]), [event("message", "\u{fffd}")]);
}
