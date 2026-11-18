use fe_editor::decorations::{Decoration, DecorationKind, Tint};

use super::{highlight_name, modifier_marks};

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

/// `deprecated` strikes the token through, whatever its type; `mutable`
/// underlines it in its colour; other modifiers add nothing.
#[test]
fn deprecated_tokens_are_struck_through() {
    let strike = Decoration::new(3..6, DecorationKind::Strikethrough, Tint::Muted);
    assert_eq!(
        modifier_marks(3..6, Some("function"), &["deprecated"]),
        std::slice::from_ref(&strike)
    );
    assert_eq!(
        modifier_marks(3..6, None, &["deprecated"]),
        std::slice::from_ref(&strike)
    );
    let underline = Decoration::new(
        3..6,
        DecorationKind::SyntaxUnderline("variable".into()),
        Tint::Muted,
    );
    assert_eq!(
        modifier_marks(3..6, Some("variable"), &["mutable", "deprecated"]),
        [underline, strike]
    );
    assert_eq!(
        modifier_marks(3..6, Some("function"), &["static", "async"]),
        []
    );
}
