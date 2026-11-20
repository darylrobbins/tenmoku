//! Compiling patterns in languages other than Rust, wasm included.

use std::{path::Path, time::Duration};

use fe_structural::Pattern;
use fe_syntax::{GrammarDeadlines, GrammarSpec, register_language, set_grammar_deadlines};

fn language(name: &str) -> &'static fe_syntax::Language {
    fe_syntax::language_by_name(name).expect("a default feature of fe-syntax")
}

fn names(pattern: &Pattern) -> Vec<&str> {
    pattern
        .metavariables()
        .iter()
        .map(|m| m.name.as_str())
        .collect()
}

#[test]
fn python_and_go_patterns_compile_where_dollar_is_not_an_identifier_char() {
    let pattern = Pattern::compile(language("python"), "print($A)").unwrap();
    assert_eq!(pattern.root_kind(), "call");
    assert_eq!(names(&pattern), ["A"]);
    assert!(pattern.literals().contains(&"print"));

    let pattern = Pattern::compile(language("go"), "fmt.Println($A)").unwrap();
    assert_eq!(pattern.root_kind(), "call_expression");
    assert_eq!(names(&pattern), ["A"]);
    assert!(pattern.literals().contains(&"Println"));
}

#[test]
fn json_pattern_falls_back_to_a_string_placeholder() {
    // A wasm parse under the gate's load needs room; this binary's only wasm
    // test sets it.
    set_grammar_deadlines(GrammarDeadlines {
        load: Duration::from_secs(30),
        parse: Duration::from_secs(30),
    });
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let spec = GrammarSpec {
        language: "structural-json".into(),
        wasm: Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm"),
        extensions: vec!["structural-json".into()],
        ..GrammarSpec::default()
    };
    let json = register_language(&spec).unwrap();
    let pattern = Pattern::compile(json, r#"{"name": $V}"#).unwrap();
    assert_eq!(pattern.root_kind(), "object");
    assert_eq!(names(&pattern), ["V"]);
    let mut stack = vec![pattern.root()];
    let mut kinds = Vec::new();
    while let Some(node) = stack.pop() {
        if node.wildcard.is_some() {
            kinds.push(node.kind.to_string());
        }
        stack.extend(&node.children);
    }
    // The string, not its content.
    assert_eq!(kinds, ["string".to_owned()]);
}
