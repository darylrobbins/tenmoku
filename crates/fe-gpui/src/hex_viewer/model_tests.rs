//! A hex view's row text: offset, hex and ASCII columns, offsets past
//! 4 GiB, a partial last row and an empty file.

use std::sync::Arc;

use fe_fileview::FileView;
use gpui::TestAppContext;

use super::model::{Column, Columns, ascii, hex_pairs, parse_hex_query, parse_offset, row_text};
use super::*;

#[test]
fn row_text_formats_offset_hex_and_ascii() {
    let bytes: Vec<u8> = b"Hello\0\n\r\xFF ~\x7FABCD".to_vec();
    assert_eq!(bytes.len(), 16);
    let columns = Columns::for_len(0x1000);
    let text = row_text(columns, 0x10, &bytes);
    assert_eq!(
        text,
        "00000010  48 65 6C 6C 6F 00 0A 0D  FF 20 7E 7F 41 42 43 44  Hello.... ~.ABCD"
    );
    assert_eq!(text.len(), columns.row_chars());
    // Each byte's cells, in both columns.
    assert_eq!(&text[columns.cell(Column::Hex, 0)], "48");
    assert_eq!(&text[columns.cell(Column::Hex, 7)], "0D");
    assert_eq!(&text[columns.cell(Column::Hex, 8)], "FF");
    assert_eq!(&text[columns.cell(Column::Hex, 15)], "44");
    assert_eq!(&text[columns.cell(Column::Ascii, 0)], "H");
    assert_eq!(&text[columns.cell(Column::Ascii, 9)], " ");
    assert_eq!(&text[columns.cell(Column::Ascii, 10)], "~");
    // Hits land on the byte whose cell, or the gap after it, holds them.
    assert_eq!(
        columns.hit(columns.cell(Column::Hex, 8).start as f32 + 0.5),
        (Column::Hex, 8)
    );
    assert_eq!(
        columns.hit(columns.cell(Column::Hex, 7).end as f32 + 0.5),
        (Column::Hex, 7)
    );
    assert_eq!(columns.hit(0.), (Column::Hex, 0));
    assert_eq!(
        columns.hit(columns.cell(Column::Ascii, 3).start as f32 + 0.2),
        (Column::Ascii, 3)
    );
    assert_eq!(columns.hit(500.), (Column::Ascii, 15));
    for byte in 0..=255u8 {
        let shown = ascii(byte);
        assert_eq!(shown == '.', !(0x20..=0x7E).contains(&byte) || byte == b'.');
    }
    assert_eq!(hex_pairs(&[0xDE, 0xAD, 0xBE, 0xEF]), "DE AD BE EF");
    assert_eq!(
        parse_hex_query("de ad BE EF"),
        Ok(vec![0xDE, 0xAD, 0xBE, 0xEF])
    );
    assert_eq!(
        parse_hex_query("deadbeef"),
        Ok(vec![0xDE, 0xAD, 0xBE, 0xEF])
    );
    assert_eq!(parse_offset(" 0x1A ", 100), Ok(26));
    assert_eq!(parse_offset("26", 100), Ok(26));
}

#[test]
fn offsets_widen_past_4_gib() {
    assert_eq!(Columns::for_len(0xFFFF_FFFF).offset_digits, 8);
    let wide = Columns::for_len(0x1_0000_0000);
    assert_eq!(wide.offset_digits, 16);
    let text = row_text(wide, 0xFFFF_FFF0, &[0xAB; 16]);
    assert!(text.starts_with("00000000FFFFFFF0  AB AB"), "{text}");
    assert_eq!(text.len(), wide.row_chars());
    assert_eq!(&text[wide.cell(Column::Hex, 0)], "AB");
    assert_eq!(&text[wide.cell(Column::Ascii, 15)], ".");
}

#[test]
fn last_row_is_partial() {
    let columns = Columns::for_len(19);
    let text = row_text(columns, 16, b"xyz");
    // The hex column is padded, so the ASCII column lines up.
    let full = row_text(columns, 0, &[b'a'; 16]);
    assert_eq!(text.find("xyz"), Some(columns.ascii_start()));
    assert_eq!(full.find("aaaa"), Some(columns.ascii_start()));
    assert_eq!(
        text,
        format!("00000010  78 79 7A{}  xyz", " ".repeat(48 - 8))
    );
    assert_eq!(model::row_count(19), 2);
    assert_eq!(model::row_count(32), 2);
    assert_eq!(model::row_count(0), 0);
}

#[gpui::test]
fn an_empty_file_says_so(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.bin");
    std::fs::write(&path, b"").unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    cx.update(crate::init);
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let family = theme.font_family.clone();
    let (viewer, cx) = cx.add_window_view(|_, cx| HexViewer::new(view, theme, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    viewer.read_with(cx, |viewer, _| {
        let rows: Vec<_> = viewer.painted.iter().map(|row| row.text.clone()).collect();
        assert_eq!(rows, ["Empty file"]);
        // In the theme's mono font.
        assert_eq!(viewer.painted_font, family);
        assert_eq!(viewer.selected(), 0..0);
        assert!(viewer.highlighted.is_empty());
    });
    // Nothing to select.
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.simulate_keystrokes("right shift-down");
    viewer.read_with(cx, |viewer, _| assert_eq!(viewer.selected(), 0..0));
}
