//! The §4.4 emoji cases through `to_lsp` and `from_lsp`.

use fe_text::Text;
use lsp_types::Position;

use crate::{PositionEncoding, from_lsp, to_lsp};

/// A §4.4 case: a text, a byte offset in it, and the offset's position
/// (row, column) under utf-8 and under utf-16.
struct Case {
    number: u32,
    text: Text,
    offset: usize,
    utf8: (u32, u32),
    utf16: (u32, u32),
}

fn case(number: u32, text: &str, offset: usize, utf8: (u32, u32), utf16: (u32, u32)) -> Case {
    Case {
        number,
        text: Text::new(text),
        offset,
        utf8,
        utf16,
    }
}

fn cases() -> Vec<Case> {
    let (crlf, _) = Text::from_file_contents("a\r\n\u{1F600}b\r\n");
    assert_eq!(crlf.to_string(), "a\n\u{1F600}b\n");
    vec![
        case(1, "a\u{1F600}b", 5, (0, 5), (0, 3)),
        case(2, "\u{1F600}\u{1F600}x", 8, (0, 8), (0, 4)),
        case(
            3,
            "x\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}y",
            19,
            (0, 19),
            (0, 9),
        ),
        case(4, "\u{1F44D}\u{1F3FD}x", 8, (0, 8), (0, 4)),
        case(5, "\u{1F1E8}\u{1F1E6}x", 8, (0, 8), (0, 4)),
        case(6, "\u{2764}\u{FE0F}x", 6, (0, 6), (0, 2)),
        case(7, "e\u{301}x", 3, (0, 3), (0, 2)),
        case(8, "\u{E9}x", 2, (0, 2), (0, 1)),
        case(9, "\u{4E2D}x", 3, (0, 3), (0, 1)),
        Case {
            number: 10,
            text: crlf,
            offset: 6,
            utf8: (1, 4),
            utf16: (1, 2),
        },
    ]
}

#[test]
fn emoji_cases_round_trip() {
    for case in cases() {
        let text = &case.text;
        let row = text.offset_to_point(case.offset).row;
        // The offset's row end: its `\n`, or the text's end.
        let row_end = text.row_start(row) + text.row_len(row) as usize;
        let row_text = text.row_to_string(row);
        for (encoding, (line, character), row_width) in [
            (PositionEncoding::Utf8, case.utf8, row_text.len()),
            (
                PositionEncoding::Utf16,
                case.utf16,
                row_text.encode_utf16().count(),
            ),
        ] {
            let what = format!("case {} under {encoding:?}", case.number);
            let expected = Position::new(line, character);
            assert_eq!(to_lsp(text, case.offset, encoding), expected, "{what}");
            assert_eq!(from_lsp(text, expected, encoding), case.offset, "{what}");

            let end = Position::new(row, row_width as u32);
            assert_eq!(to_lsp(text, row_end, encoding), end, "{what}: row end");
            assert_eq!(from_lsp(text, end, encoding), row_end, "{what}: row end");
        }
    }
}

#[test]
fn server_positions_clamp() {
    let two_rows = Text::new("a\u{1F600}\nz");
    let one_row = Text::new("a\u{1F600}b");
    for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
        // Case 11: past the end of a line, to the row's end.
        assert_eq!(from_lsp(&two_rows, Position::new(0, 99), encoding), 5);
        // Case 12: past the end of the text, to the text's end.
        assert_eq!(from_lsp(&two_rows, Position::new(9, 0), encoding), 7);
    }
    // Case 13: inside a char, to its start.
    assert_eq!(
        from_lsp(&one_row, Position::new(0, 2), PositionEncoding::Utf16),
        1
    );
    assert_eq!(
        from_lsp(&one_row, Position::new(0, 3), PositionEncoding::Utf8),
        1
    );
}
