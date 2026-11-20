//! Matching JavaScript patterns against JavaScript files.

use fe_structural::{Match, Pattern};

fn javascript() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("javascript").expect("javascript is a default feature of fe-syntax")
}

fn texts<'s>(source: &'s str, matches: &[Match]) -> Vec<&'s str> {
    matches.iter().map(|m| &source[m.range.clone()]).collect()
}

#[test]
fn python_and_js_patterns_match_with_their_own_grammars() {
    let pattern = Pattern::compile(javascript(), "console.log($A)").unwrap();
    assert_eq!(pattern.root_kind(), "call_expression");
    let source = "console.log(x);\nconsole.log(x, y);\nconsole.error(z);\n\
                  function f() { console.log(/* c */ a.b); }\n";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(
        texts(source, &found),
        ["console.log(x)", "console.log(/* c */ a.b)"]
    );
    let captured: Vec<_> = found
        .iter()
        .map(|m| &source[m.captures.single("A").unwrap()])
        .collect();
    assert_eq!(captured, ["x", "a.b"]);
}

#[test]
fn js_repeated_metavariable_compares_text() {
    let pattern = Pattern::compile(javascript(), "$A === $A").unwrap();
    let source = "a === a;\na === b;\n";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["a === a"]);
}
