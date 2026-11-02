//! The tree shape `crates/fe-syntax/queries/csv.scm` counts columns from.

use fe_grammar_csv::{LANGUAGE_CSV, LANGUAGE_TSV};
use tree_sitter::{Language, Parser, Tree};

fn parse(language: &Language, text: &str) -> Tree {
    let mut parser = Parser::new();
    parser.set_language(language).unwrap();
    parser.parse(text, None).unwrap()
}

/// Each row's fields, as the text of their content ("" for an empty field).
fn rows(language: &Language, text: &str) -> Vec<Vec<String>> {
    let tree = parse(language, text);
    let root = tree.root_node();
    assert!(!root.has_error(), "{text:?}: {}", root.to_sexp());
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .map(|row| {
            assert_eq!(row.kind(), "row");
            let mut cursor = row.walk();
            row.named_children(&mut cursor)
                .map(|field| {
                    assert_eq!(field.kind(), "field");
                    field.named_child(0).map_or(String::new(), |content| {
                        assert_eq!(content.kind(), "content");
                        text[content.byte_range()].to_owned()
                    })
                })
                .collect()
        })
        .collect()
}

fn languages() -> [(Language, char); 2] {
    [(LANGUAGE_CSV.into(), ','), (LANGUAGE_TSV.into(), '\t')]
}

#[test]
fn every_column_is_a_named_child_of_its_row() {
    for (language, separator) in languages() {
        for (text, expected) in [
            ("a,,c", vec![vec!["a", "", "c"]]),
            (",b", vec![vec!["", "b"]]),
            ("a,", vec![vec!["a"]]),
            (",", vec![vec![""]]),
            ("a,b\n\nc\n", vec![vec!["a", "b"], vec!["c"]]),
            ("a,b\r\nc\rd", vec![vec!["a", "b"], vec!["c"], vec!["d"]]),
            (
                "\"a,\nb\",\"c\"\"d\"\n",
                vec![vec!["\"a,\nb\"", "\"c\"\"d\""]],
            ),
            ("", vec![]),
            ("\n", vec![]),
        ] {
            // Commas stand for the separator in `text` and `expected`.
            let swap = |text: &str| text.replace(',', &separator.to_string());
            assert_eq!(
                rows(&language, &swap(text)),
                expected
                    .iter()
                    .map(|row| row.iter().map(|field| swap(field)).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
                "{text:?}"
            );
        }
    }
}

/// Parses `text` and every extension of it by up to `left` more characters.
fn visit(text: &mut String, left: usize, languages: &[(Language, char); 2]) {
    for (language, _) in languages {
        let tree = parse(language, text);
        let root = tree.root_node();
        assert!(!root.has_error(), "{text:?}: {}", root.to_sexp());
    }
    if left > 0 {
        for c in ['a', ',', '\t', '"', '\n', ' '] {
            text.push(c);
            visit(text, left - 1, languages);
            text.pop();
        }
    }
}

#[test]
fn no_text_is_a_parse_error() {
    visit(&mut String::new(), 5, &languages());
}
