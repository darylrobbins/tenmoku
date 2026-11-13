use super::highlight_name;

const NONE: [&str; 0] = [];

#[test]
fn excluded_code_token_types_map_to_comment() {
    for token_type in ["excludedCode", "excluded code"] {
        assert_eq!(highlight_name(token_type, &NONE), Some("comment"));
        assert_eq!(
            highlight_name(token_type, &["static", "declaration"]),
            Some("comment")
        );
    }
}

#[test]
fn unknown_types_still_leave_the_grammar_colour() {
    assert_eq!(highlight_name("madeUpType", &NONE), None);
    assert_eq!(highlight_name("madeUpType", &["static"]), None);
}
