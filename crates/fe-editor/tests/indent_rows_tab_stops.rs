use fe_buffer::Buffer;
use fe_editor::{EditorModel, IndentStyle};

fn shifted(style: IndentStyle, text: &str, indent: bool) -> String {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.set_tab_size(4);
    model.set_indent_style(Some(style));
    model.select_all();
    if indent {
        model.indent_rows();
    } else {
        model.outdent_rows();
    }
    model.text().to_string()
}

#[test]
fn indent_and_outdent_go_to_the_next_or_previous_indent_stop() {
    let spaces = IndentStyle::Spaces(4);
    assert_eq!(
        shifted(spaces, "a\n  b\n c\n    d", true),
        "    a\n    b\n    c\n        d"
    );
    assert_eq!(
        shifted(spaces, "      a\n  b\n    c\n\t  d", false),
        "    a\nb\nc\n    d"
    );
    assert_eq!(
        shifted(IndentStyle::Tabs, "a\n  b\n\t  c", true),
        "\ta\n\tb\n\t\tc"
    );
    assert_eq!(
        shifted(IndentStyle::Tabs, "\t  a\n\t\tb", false),
        "\ta\n\tb"
    );
    assert_eq!(
        shifted(IndentStyle::Spaces(2), "  a\n\n  b", true),
        "    a\n\n    b"
    );
}
