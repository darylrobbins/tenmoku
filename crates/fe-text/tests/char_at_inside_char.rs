//! `Text::char_at` answers `None` for an offset inside a multi-byte char,
//! as it does past the end, instead of panicking: `TextSource`'s methods are
//! infallible, and fe-fileview's `FileText` already answers `None` there.

use fe_text::{Text, TextSource};

#[test]
fn char_at_inside_a_char_is_none() {
    let raw = "aé世😀\nz";
    let text = Text::new(raw);
    let source: &dyn TextSource = &text;
    for offset in 0..raw.len() + 2 {
        let want = match raw.is_char_boundary(offset) {
            true => raw.get(offset..).and_then(|rest| rest.chars().next()),
            false => None,
        };
        assert_eq!(text.char_at(offset), want, "Text::char_at({offset})");
        assert_eq!(
            source.char_at(offset),
            want,
            "TextSource::char_at({offset})"
        );
    }
}
