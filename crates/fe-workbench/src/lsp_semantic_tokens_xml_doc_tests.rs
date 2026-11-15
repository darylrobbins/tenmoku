use super::highlight_name;

const NONE: [&str; 0] = [];

const CASES: [(&str, &str, &str); 10] = [
    ("xmlDocCommentName", "xml doc comment - name", "tag"),
    (
        "xmlDocCommentAttributeName",
        "xml doc comment - attribute name",
        "attribute",
    ),
    (
        "xmlDocCommentAttributeValue",
        "xml doc comment - attribute value",
        "string",
    ),
    (
        "xmlDocCommentAttributeQuotes",
        "xml doc comment - attribute quotes",
        "string",
    ),
    (
        "xmlDocCommentDelimiter",
        "xml doc comment - delimiter",
        "comment.documentation",
    ),
    (
        "xmlDocCommentText",
        "xml doc comment - text",
        "comment.documentation",
    ),
    (
        "xmlDocCommentComment",
        "xml doc comment - comment",
        "comment",
    ),
    (
        "xmlDocCommentCDataSection",
        "xml doc comment - cdata section",
        "markup.raw",
    ),
    (
        "xmlDocCommentEntityReference",
        "xml doc comment - entity reference",
        "escape",
    ),
    (
        "xmlDocCommentProcessingInstruction",
        "xml doc comment - processing instruction",
        "keyword",
    ),
];

#[test]
fn each_xml_doc_type_maps_in_both_spellings() {
    for (custom, raw, want) in CASES {
        assert_eq!(highlight_name(custom, &NONE), Some(want), "{custom}");
        assert_eq!(highlight_name(raw, &NONE), Some(want), "{raw}");
    }
}

#[test]
fn modifiers_do_not_change_xml_doc_mapping() {
    for (custom, raw, want) in CASES {
        for modifiers in [&["static", "defaultLibrary"][..], &["documentation"][..]] {
            assert_eq!(highlight_name(custom, modifiers), Some(want), "{custom}");
            assert_eq!(highlight_name(raw, modifiers), Some(want), "{raw}");
        }
    }
}
