//! `injection.doc-prefix`: a run of doc comment lines, one host node each, is
//! one layer, with each line's prefix left out.

use std::{collections::HashSet, ops::Range, time::Instant};

use fe_buffer::Buffer;

use super::{Collected, DocRun, InjectionQuery};
use crate::*;

fn parsed(name: &str, text: &str) -> (Buffer, Syntax) {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(name).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run_until(|_| || false));
    (buffer, syntax)
}

/// The text of each xml layer kept after highlighting `range` of `text`.
fn layers(text: &str, range: Range<usize>) -> Vec<Vec<&str>> {
    language_by_name("xml");
    let (_buffer, syntax) = parsed("csharp", text);
    syntax.highlights_compiling(range);
    syntax
        .layers()
        .into_iter()
        .filter(|(language, _)| *language == "xml")
        .map(|(_, ranges)| ranges.into_iter().map(|range| &text[range]).collect())
        .collect()
}

#[test]
fn a_run_of_doc_lines_is_one_layer_without_its_prefixes() {
    let text =
        "class C\n{\n    /// <summary>\n    /// Text\n    /// </summary>\n    void F() { }\n}\n";
    assert_eq!(
        layers(text, 0..text.len()),
        [[" <summary>", " Text", " </summary>"]]
    );
}

#[test]
fn a_blank_row_or_another_node_ends_a_run() {
    let text = "class C\n{\n    /// <a/>\n\n    /// <b/>\n    // plain\n    /// <c/>\n    void F() { }\n    /// <d/>\n    void G() { }\n}\n";
    assert_eq!(
        layers(text, 0..text.len()),
        [[" <a/>"], [" <b/>"], [" <c/>"], [" <d/>"]]
    );
}

#[test]
fn lines_with_the_prefix_and_more_of_its_last_byte_are_not_doc_lines() {
    let text = "/// <a>\n//// <b/>\n/// </a>\nclass C { }\n";
    assert_eq!(layers(text, 0..text.len()), [[" <a>"], [" </a>"]]);
    // Nor a plain comment, nor an empty `///` line's nothing.
    let text = "// <a/>\n/* <b/> */\n///\n/// <c/>\nclass C { }\n";
    assert_eq!(layers(text, 0..text.len()), [[" <c/>"]]);
}

/// Highlighting only a run's last line still parses the whole run, so its
/// closing tag pairs with the opening one off screen.
#[test]
fn highlighting_part_of_a_run_takes_all_of_it() {
    let text = "/// <summary>\n/// Text\n/// </summary>\nclass C { }\n";
    let last = text.find("/// </summary>").unwrap();
    assert_eq!(
        layers(text, last..last + 5),
        [[" <summary>", " Text", " </summary>"]]
    );
}

/// An empty prefix is no prefix: the pattern injects as one without it.
#[test]
fn an_empty_doc_prefix_is_ignored() {
    let grammar: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let source = "((comment) @injection.content (#set! injection.language \"xml\") (#set! injection.doc-prefix \"\"))\n\
                  ((comment) @injection.content (#set! injection.language \"xml\") (#set! injection.doc-prefix \"///\"))";
    let query = InjectionQuery::new(&grammar, source).unwrap();
    assert_eq!(query.doc_prefixes, [None, Some("///".into())]);
}

/// Walking a parent's children for a run checks the deadline at each.
#[test]
fn collecting_a_run_stops_at_the_deadline() {
    let text = format!("{}class C {{ }}\n", "/// <a/>\n".repeat(200));
    let (buffer, syntax) = parsed("csharp", &text);
    let tree = syntax.tree().unwrap();
    let last = tree
        .root_node()
        .descendant_for_byte_range(text.len() - 20, text.len() - 19)
        .filter(|node| node.kind() == "comment")
        .expect("the last doc line");
    let run = |deadline| {
        let run = DocRun {
            text: buffer.text(),
            prefix: b"///",
            pattern_ix: 0,
            include_children: false,
            deadline,
        };
        let (mut combined, mut ranges) = (HashSet::new(), Vec::new());
        let collected = run.add(last, &mut combined, &mut ranges);
        (collected, combined.len(), ranges.len())
    };
    assert_eq!(run(None), (Collected::All, 200, 200));
    assert_eq!(run(Some(Instant::now())).0, Collected::OutOfTime);
}
