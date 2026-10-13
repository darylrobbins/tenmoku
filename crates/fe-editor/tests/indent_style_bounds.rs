//! Edges of indent style: a zero space width set from outside, and rows
//! of other whitespace than spaces and tabs.

use fe_buffer::{Buffer, Text};
use fe_editor::{EditorModel, IndentStyle, detect_indent_style};

fn model(text: &str) -> EditorModel {
    EditorModel::new(Buffer::new(text), None)
}

fn detect(text: &str) -> Option<IndentStyle> {
    detect_indent_style(&Text::from_file_contents(text).0)
}

#[test]
fn zero_space_width_tab_does_not_panic() {
    let mut model = model("a\n");
    model.set_indent_style(Some(IndentStyle::Spaces(0)));
    assert_eq!(model.indent_style(), None);
    let tab_size = model.indent_unit().len();
    model.tab();
    assert_eq!(
        model.text().to_string(),
        format!("{}a\n", " ".repeat(tab_size))
    );
    model.outdent_rows();
    model.indent_rows();
    assert_eq!(
        model.text().to_string(),
        format!("{}a\n", " ".repeat(tab_size))
    );
}

#[test]
fn positive_space_widths_are_kept() {
    let mut model = model("a\n");
    for width in [1, 3, 16] {
        model.set_indent_style(Some(IndentStyle::Spaces(width)));
        assert_eq!(model.indent_style(), Some(IndentStyle::Spaces(width)));
    }
}

#[test]
fn unicode_whitespace_rows_are_blank() {
    assert_eq!(detect(&"  \u{00a0}\n".repeat(5)), None);
    assert_eq!(detect(&"  \u{3000}\n".repeat(5)), None);
    assert_eq!(detect(&"  \x0c\n".repeat(5)), None);
    assert_eq!(detect(&"    \x0b\n".repeat(5)), None);
}

#[test]
fn real_rows_after_whitespace_rows_still_detect() {
    let text = format!("{}{}", "  \u{00a0}\n  \x0c\n", "a\n  b\n".repeat(5));
    assert_eq!(detect(&text), Some(IndentStyle::Spaces(2)));
}
