//! Matching Python patterns against Python files.

use fe_structural::{Match, Pattern};

fn python() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("python").expect("python is a default feature of fe-syntax")
}

fn texts<'s>(source: &'s str, matches: &[Match]) -> Vec<&'s str> {
    matches.iter().map(|m| &source[m.range.clone()]).collect()
}

#[test]
fn python_and_js_patterns_match_with_their_own_grammars() {
    let pattern = Pattern::compile(python(), "print($A)").unwrap();
    assert_eq!(pattern.root_kind(), "call");
    let source = "print(x)\nprint(x, y)\n# print(z)\ndef f():\n    print(a.b)\n";
    let found = pattern.find_in_text(source).unwrap();
    // `print(x, y)` has two arguments: a mismatch until `$$$` (SS4).
    assert_eq!(texts(source, &found), ["print(x)", "print(a.b)"]);
    let captured: Vec<_> = found
        .iter()
        .map(|m| &source[m.captures.single("A").unwrap()])
        .collect();
    assert_eq!(captured, ["x", "a.b"]);
}

#[test]
fn python_comments_inside_a_call_are_skipped() {
    let pattern = Pattern::compile(python(), "f(1, 2)").unwrap();
    let source = "f(1,  # one\n  2)\n";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["f(1,  # one\n  2)"]);
}
