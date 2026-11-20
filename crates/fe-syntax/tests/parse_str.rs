//! `parse_str`: a string parsed in a language on the calling thread.

use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use fe_syntax::{
    GrammarDeadlines, GrammarSpec, parse_str, register_language, set_grammar_deadlines,
};

/// Deadlines are process-wide, so the tests that set them take turns.
static DEADLINES: Mutex<()> = Mutex::new(());

fn deadlines(parse: Duration) -> MutexGuard<'static, ()> {
    let guard = DEADLINES.lock().unwrap_or_else(|e| e.into_inner());
    set_grammar_deadlines(GrammarDeadlines {
        load: Duration::from_secs(5),
        parse,
    });
    guard
}

fn json() -> &'static fe_syntax::Language {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let spec = GrammarSpec {
        language: "parse-str-json".into(),
        wasm: Path::new(&manifest).join("tests/fixtures/tree-sitter-json.wasm"),
        extensions: vec!["parse-str-json".into()],
        ..GrammarSpec::default()
    };
    register_language(&spec).unwrap()
}

#[test]
#[cfg(all(feature = "rust", feature = "python"))]
fn parse_str_rust_and_python_give_trees_with_the_expected_root_kinds() {
    let rust = fe_syntax::language_by_name("rust").unwrap();
    let tree = parse_str(rust, "fn main() {}\n", || false).unwrap();
    assert_eq!(tree.root_node().kind(), "source_file");
    let python = fe_syntax::language_by_name("python").unwrap();
    let tree = parse_str(python, "x = 1\n", || false).unwrap();
    assert_eq!(tree.root_node().kind(), "module");
}

#[test]
#[cfg(feature = "rust")]
fn parse_str_stops_when_the_callback_says_so_and_returns_none() {
    let rust = fe_syntax::language_by_name("rust").unwrap();
    let text = "fn f() { let x = 1; }\n".repeat(2000);
    let mut asked = 0;
    let tree = parse_str(rust, &text, || {
        asked += 1;
        true
    });
    assert!(tree.is_none());
    assert!(asked > 0);
}

#[test]
fn parse_str_parses_a_registered_wasm_grammar() {
    let _turn = deadlines(Duration::from_secs(30));
    let tree = parse_str(json(), "{\"a\": [1, 2]}", || false).unwrap();
    assert_eq!(tree.root_node().kind(), "document");
    assert!(!tree.root_node().has_error());
}
