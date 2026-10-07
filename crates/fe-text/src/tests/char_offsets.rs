use crate::Text;

#[test]
fn offset_to_char_counts_chars() {
    // 'a' is 1 byte, 'é' 2, '😀' 4.
    let text = Text::new("aé😀\nb");
    for (offset, chars) in [(0, 0), (1, 1), (3, 2), (7, 3), (8, 4), (9, 5)] {
        assert_eq!(text.offset_to_char(offset), chars, "at byte {offset}");
    }
    assert_eq!(text.len(), 9);
    for past in [10, 100, usize::MAX] {
        assert_eq!(text.offset_to_char(past), 5, "at byte {past}");
    }
}
