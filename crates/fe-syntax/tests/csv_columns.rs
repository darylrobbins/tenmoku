//! CSV and TSV: each column takes a colour by its index, however many fields
//! before it are empty or quoted.

#![cfg(feature = "csv")]

use std::path::Path;

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name, language_name_for_path};

/// The highlight of columns 0 to 15, in order.
const COLUMNS: [&str; 16] = [
    "keyword",
    "string",
    "number",
    "function",
    "type",
    "constant",
    "property",
    "variable.parameter",
    "keyword",
    "string",
    "number",
    "function",
    "type",
    "constant",
    "property",
    "variable.parameter",
];

/// `text`'s highlighted runs in `language`, as `(text, highlight)`.
fn highlighted<'a>(language: &str, text: &'a str) -> Vec<(&'a str, &'static str)> {
    let language = language_by_name(language).unwrap();
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    assert!(syntax.tree().is_some(), "{text:?} has a tree");
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (&text[span.range], span.highlight.name()))
        .collect()
}

/// Both dialects: `text` is written with commas, and for TSV its commas become
/// tabs and its tabs commas, so each separator is plain text to the other.
fn in_both_dialects(text: &str, expected: &[(&str, usize)]) {
    let swap = |text: &str| -> String {
        text.chars()
            .map(|c| match c {
                ',' => '\t',
                '\t' => ',',
                c => c,
            })
            .collect()
    };
    for (language, text, swapped) in [("csv", text.to_owned(), false), ("tsv", swap(text), true)] {
        let expected: Vec<(String, &str)> = expected
            .iter()
            .map(|&(field, column)| {
                let field = if swapped {
                    swap(field)
                } else {
                    field.to_owned()
                };
                (field, COLUMNS[column])
            })
            .collect();
        let found: Vec<(String, &str)> = highlighted(language, &text)
            .into_iter()
            .map(|(field, highlight)| (field.to_owned(), highlight))
            .collect();
        assert_eq!(found, expected, "{language}: {text:?}");
    }
}

#[test]
fn file_extensions_pick_the_dialect() {
    for (file, language) in [("x.csv", "csv"), ("x.tsv", "tsv"), ("x.tab", "tsv")] {
        assert_eq!(language_name_for_path(Path::new(file)), Some(language));
    }
}

#[test]
fn empty_and_quoted_fields_keep_the_columns_after_them() {
    in_both_dialects("a,,\"c,d\",e\n", &[("a", 0), ("\"c,d\"", 2), ("e", 3)]);
    in_both_dialects(",b\n", &[("b", 1)]);
    in_both_dialects(",,\n", &[]);
    in_both_dialects("a,\nb\n", &[("a", 0), ("b", 0)]);
}

#[test]
fn a_quoted_field_with_an_embedded_newline_keeps_the_column_count() {
    in_both_dialects(
        "a,\"b\nc\",d\ne,f\n",
        &[("a", 0), ("\"b\nc\"", 1), ("d", 2), ("e", 0), ("f", 1)],
    );
    in_both_dialects(
        "\"a\"\"b\",\"\",c\n",
        &[("\"a\"\"b\"", 0), ("\"\"", 1), ("c", 2)],
    );
}

#[test]
fn columns_past_the_sixteenth_are_plain() {
    let fields: Vec<String> = (0..20).map(|column| format!("f{column}")).collect();
    let expected: Vec<(&str, usize)> = fields[..16]
        .iter()
        .enumerate()
        .map(|(column, field)| (field.as_str(), column))
        .collect();
    in_both_dialects(&fields.join(","), &expected);
}

#[test]
fn fields_hold_spaces_and_single_characters_and_the_other_separator() {
    in_both_dialects(
        "a b, c ,d\te,1\n",
        &[("a b", 0), (" c ", 1), ("d\te", 2), ("1", 3)],
    );
}

#[test]
fn blank_rows_and_empty_files_have_no_highlights() {
    in_both_dialects("a,b\n\n\nc,d", &[("a", 0), ("b", 1), ("c", 0), ("d", 1)]);
    in_both_dialects("", &[]);
    in_both_dialects("\n\n", &[]);
}

#[test]
fn stray_quotes_keep_the_columns_after_them() {
    in_both_dialects(
        "5\" disk,x\na,b\n",
        &[("5\" disk", 0), ("x", 1), ("a", 0), ("b", 1)],
    );
    in_both_dialects(
        "\"a\"b,c\nd\"e\"f,g\n",
        &[("\"a\"b", 0), ("c", 1), ("d\"e\"f", 0), ("g", 1)],
    );
    // Never closed: it ends where an unquoted field would.
    in_both_dialects(
        "a,\"b,c\nd,e\n",
        &[("a", 0), ("\"b", 1), ("c", 2), ("d", 0), ("e", 1)],
    );
}

#[test]
fn a_window_starting_mid_row_keeps_the_columns() {
    let text = "a,b,c,d\n";
    let language = language_by_name("csv").unwrap();
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    let found: Vec<_> = syntax
        .highlights(text.find('d').unwrap()..text.len())
        .into_iter()
        .map(|span| (&text[span.range], span.highlight.name()))
        .collect();
    assert_eq!(found, [("d", COLUMNS[3])]);
}
