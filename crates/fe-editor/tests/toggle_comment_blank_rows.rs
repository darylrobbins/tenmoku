//! `EditorModel::toggle_comment` on blank rows alone, which it comments at
//! their ends as VS Code does.

use fe_buffer::Buffer;
use fe_editor::{CommentTokens, EditorModel};

const TOKENS: CommentTokens = CommentTokens {
    line: Some("//"),
    block: Some(("/*", "*/")),
};

fn toggled(text: &str, selection: std::ops::Range<usize>) -> (String, usize) {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.select_range(selection, false);
    model.toggle_comment(TOKENS);
    (model.text().to_string(), model.selections()[0].range.end)
}

#[test]
fn blank_rows_alone_are_commented_as_vs_code_does() {
    assert_eq!(toggled("", 0..0), ("// ".to_string(), 3));
    assert_eq!(toggled("a\n\nb", 2..2), ("a\n// \nb".to_string(), 5));
    assert_eq!(toggled("  ", 2..2), ("  // ".to_string(), 5));
    assert_eq!(toggled("  \n\t", 0..4).0, "  // \n\t// ");
    assert_eq!(toggled("a\n\nb", 0..4).0, "// a\n\n// b");
}
