use super::highlight_name;

const NONE: [&str; 0] = [];

#[test]
fn token_types_map_to_highlight_names() {
    for (token_type, name) in [
        ("namespace", "module"),
        ("class", "type"),
        ("struct", "type"),
        ("enum", "type"),
        ("interface", "type"),
        ("type", "type"),
        ("typeParameter", "type"),
        ("parameter", "variable.parameter"),
        ("variable", "variable"),
        ("property", "property"),
        ("event", "property"),
        ("enumMember", "constant"),
        ("function", "function"),
        ("method", "function.method"),
        ("macro", "function.macro"),
        ("decorator", "attribute"),
        ("regexp", "string.special"),
        ("keyword", "keyword"),
        ("comment", "comment"),
        ("string", "string"),
        ("number", "number"),
        ("operator", "operator"),
    ] {
        assert_eq!(
            highlight_name(token_type, &NONE),
            Some(name),
            "{token_type}"
        );
    }

    assert_eq!(
        highlight_name("type", &["defaultLibrary"]),
        Some("type.builtin")
    );
    assert_eq!(
        highlight_name("class", &["declaration", "defaultLibrary"]),
        Some("type.builtin")
    );
    assert_eq!(
        highlight_name("function", &["defaultLibrary"]),
        Some("function.builtin")
    );
    assert_eq!(
        highlight_name("comment", &["documentation"]),
        Some("comment.documentation")
    );
    assert_eq!(highlight_name("variable", &["readonly"]), Some("constant"));
    assert_eq!(highlight_name("variable", &["static"]), Some("constant"));
    assert_eq!(
        highlight_name("parameter", &["readonly"]),
        Some("variable.parameter")
    );
    assert_eq!(highlight_name("string", &["documentation"]), Some("string"));

    assert_eq!(highlight_name("lifetime", &NONE), None);
    assert_eq!(highlight_name("selfKeyword", &["defaultLibrary"]), None);
}
