//! A metavariable used more than once must cover the same text each time.

use fe_structural::{Match, Pattern};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("rust is a default feature of fe-syntax")
}

fn texts<'s>(source: &'s str, matches: &[Match]) -> Vec<&'s str> {
    matches.iter().map(|m| &source[m.range.clone()]).collect()
}

#[test]
fn repeated_metavariable_requires_equal_source_text() {
    let pattern = Pattern::compile(rust(), "$A == $A").unwrap();
    let source = "fn f() { x == x; x == y; a.b == a.b; a.b == a.c; }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["x == x", "a.b == a.b"]);
    // One range per name: the first occurrence.
    assert_eq!(&source[found[0].captures.single("A").unwrap()], "x");
    assert_eq!(found[0].captures.get("A").unwrap().len(), 1);
}

#[test]
fn repeated_name_compares_across_the_pattern_tree() {
    let pattern = Pattern::compile(rust(), "foo($A, bar($A))").unwrap();
    let source = "fn f() { foo(x, bar(x)); foo(x, bar(y)); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(x, bar(x))"]);
}

#[test]
fn anonymous_metavariables_need_not_be_equal() {
    let pattern = Pattern::compile(rust(), "$_ == $_").unwrap();
    let source = "fn f() { x == y; }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["x == y"]);
    assert!(found[0].captures.is_empty());
}

#[test]
fn different_names_may_bind_equal_text() {
    let pattern = Pattern::compile(rust(), "$A == $B").unwrap();
    let source = "fn f() { x == x; }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["x == x"]);
    let names: Vec<_> = found[0].captures.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["A", "B"]);
}
