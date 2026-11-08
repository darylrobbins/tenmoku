#![cfg(feature = "html")]
//! Vue's `injections.scm` selects the injected language from a `lang=` or
//! `type=` value, bare or quoted. Its patterns that name no vue-only node run
//! here on the native HTML grammar, which has the same element and attribute
//! nodes, since the wasm golden check is ignored and can't run in the gate.

use std::{fs, path::Path};

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// Splits query source into its top-level patterns, skipping comments and
/// strings.
fn patterns(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    let mut chars = source.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            ';' => {
                for (_, d) in chars.by_ref() {
                    if d == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                while let Some((_, d)) = chars.next() {
                    match d {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '(' | '[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    out.push(source[start.take().unwrap()..=i].to_string());
                }
            }
            _ => {}
        }
    }
    out
}

fn query() -> Query {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/grammars/vue/injections.scm");
    let source = fs::read_to_string(path).unwrap();
    let kept: Vec<String> = patterns(&source)
        .into_iter()
        .filter(|p| !p.contains("interpolation") && !p.contains("directive_attribute"))
        .collect();
    assert!(!kept.is_empty());
    let language = tree_sitter_html::LANGUAGE.into();
    Query::new(&language, &kept.join("\n\n")).expect("vue's selector patterns compile")
}

/// The languages injected into the document's `<script>`/`<style>` block.
fn languages(query: &Query, text: &str) -> Vec<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_html::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(text, None).unwrap();
    let content = query.capture_index_for_name("injection.content").unwrap();
    let mut cursor = QueryCursor::new();
    let mut found = Vec::new();
    let mut matches = cursor.matches(query, tree.root_node(), text.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures().iter().filter(|c| c.index == content) {
            if capture.node.kind() != "raw_text" {
                continue;
            }
            for property in query.property_settings(m.pattern_index) {
                if &*property.key == "injection.language" {
                    found.push(property.value.as_deref().unwrap().to_string());
                }
            }
        }
    }
    found
}

fn check(cases: &[(&str, &str)]) {
    let query = query();
    for (document, language) in cases {
        assert_eq!(languages(&query, document), [*language], "{document}");
    }
}

#[test]
fn vue_unquoted_lang_and_type_values_pick_the_injected_language() {
    check(&[
        ("<script lang=ts>const a: number = 1</script>", "typescript"),
        ("<script lang=tsx>const a = <b/></script>", "tsx"),
        ("<script lang=js>let a = 1</script>", "javascript"),
        ("<script lang=jsx>let a = <b/></script>", "javascript"),
        ("<style lang=css>.a { color: red; }</style>", "css"),
        ("<script type=module>let a = 1</script>", "javascript"),
        ("<script type=importmap>{}</script>", "json"),
        ("<style type=text/css>.a { color: red; }</style>", "css"),
    ]);
}

#[test]
fn vue_quoted_lang_and_type_values_still_pick_the_injected_language() {
    check(&[
        (
            "<script lang=\"ts\">const a: number = 1</script>",
            "typescript",
        ),
        ("<script lang=\"tsx\">const a = <b/></script>", "tsx"),
        ("<script lang=\"js\">let a = 1</script>", "javascript"),
        ("<script lang=\"jsx\">let a = <b/></script>", "javascript"),
        ("<style lang=\"css\">.a { color: red; }</style>", "css"),
        ("<script type=\"module\">let a = 1</script>", "javascript"),
        ("<script type=\"importmap\">{}</script>", "json"),
        ("<style type=\"text/css\">.a { color: red; }</style>", "css"),
        ("<script>let a = 1</script>", "javascript"),
        ("<style>.a { color: red; }</style>", "css"),
    ]);
}
