//! A pattern's `where` section: constraints on metavariables and `inside`.

use fe_structural::{Match, Pattern, PatternError, PatternErrorKind};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("rust is a default feature of fe-syntax")
}

fn pattern(source: &str) -> Pattern {
    Pattern::compile(rust(), source).unwrap()
}

fn error(source: &str) -> PatternError {
    Pattern::compile(rust(), source).unwrap_err()
}

fn texts<'s>(source: &'s str, matches: &[Match]) -> Vec<&'s str> {
    matches.iter().map(|m| &source[m.range.clone()]).collect()
}

const UNWRAPS: &str = "fn f() { a.unwrap(); b().unwrap(); c.d.unwrap(); }";

#[test]
fn where_kind_constraint_filters_by_node_kind() {
    let found = pattern("$X.unwrap()\nwhere\n$X: kind=identifier")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["a.unwrap()"]);
}

#[test]
fn where_regex_constraint_filters_by_text() {
    let found = pattern("$X.unwrap()\nwhere\n$X: regex=^[bc]")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["b().unwrap()", "c.d.unwrap()"]);
}

#[test]
fn where_text_constraint_needs_the_exact_text() {
    let found = pattern("$X.unwrap()\nwhere\n$X: text=c.d")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["c.d.unwrap()"]);
}

#[test]
fn where_not_constraint_inverts() {
    let found = pattern("$X.unwrap()\nwhere\n$X: not kind=identifier")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["b().unwrap()", "c.d.unwrap()"]);
    let found = pattern("$X.unwrap()\nwhere\n$X: not regex=d$")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["a.unwrap()", "b().unwrap()"]);
}

#[test]
fn constraints_on_one_name_all_hold() {
    let found = pattern("$X.unwrap()\n  where  \n\n$X: not kind=identifier\n$X: not text=b()\n")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["c.d.unwrap()"]);
}

#[test]
fn a_constraint_steers_a_multi_wildcard_run() {
    // Lazily, REST would be empty; its constraint makes it take `b, c`.
    let pattern = pattern("foo($$$REST, $LAST)\nwhere\n$$$REST: regex=c$");
    let source = "fn f() { foo(a, b, c, d); foo(x, y); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(a, b, c, d)"]);
    let rest = found[0].captures.single("REST").unwrap();
    assert_eq!(&source[rest], "a, b, c");
}

#[test]
fn a_kind_constraint_on_a_multi_wildcard_checks_each_named_node() {
    let pattern = pattern("foo($$$ARGS)\nwhere\n$ARGS: kind=integer_literal");
    let source = "fn f() { foo(1, 2); foo(1, x); foo(); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(1, 2)", "foo()"]);
}

const TESTS: &str =
    "fn test() { a.unwrap(); }\nfn main() { b.unwrap(); }\nconst C: u8 = c.unwrap();\n";

#[test]
fn where_inside_requires_an_ancestor_of_the_kind() {
    let found = pattern("$X.unwrap()\nwhere\ninside: kind=function_item")
        .find_in_text(TESTS)
        .unwrap();
    assert_eq!(texts(TESTS, &found), ["a.unwrap()", "b.unwrap()"]);
    let found = pattern("$X.unwrap()\nwhere\ninside: kind=function_item name=test")
        .find_in_text(TESTS)
        .unwrap();
    assert_eq!(texts(TESTS, &found), ["a.unwrap()"]);
}

#[test]
fn where_not_inside_excludes_it() {
    let found = pattern("$X.unwrap()\nwhere\nnot inside: kind=function_item name=test")
        .find_in_text(TESTS)
        .unwrap();
    assert_eq!(texts(TESTS, &found), ["b.unwrap()", "c.unwrap()"]);
    let found = pattern("$X.unwrap()\nwhere\nnot inside: kind=function_item")
        .find_in_text(TESTS)
        .unwrap();
    assert_eq!(texts(TESTS, &found), ["c.unwrap()"]);
}

#[test]
fn inside_looks_at_ancestors_not_the_match_itself() {
    let found = pattern("fn $F() { $$$ }\nwhere\nnot inside: kind=function_item")
        .find_in_text(TESTS)
        .unwrap();
    assert_eq!(found.len(), 2);
}

#[test]
fn constraint_on_an_unbound_metavariable_is_a_compile_error_with_a_position() {
    let source = "$X.unwrap()\nwhere\n$X: kind=identifier\n  $Y: kind=identifier";
    let found = error(source);
    assert_eq!(found.kind, PatternErrorKind::UnboundMetavar);
    assert_eq!(found.offset, source.find("$Y").unwrap());
    // `$_` binds nothing, so it can't be constrained either.
    let source = "$_.unwrap()\nwhere\n$_: kind=identifier";
    let found = error(source);
    assert_eq!(found.kind, PatternErrorKind::UnboundMetavar);
    assert_eq!(found.offset, source.find("$_:").unwrap());
}

#[test]
fn unknown_keys_and_lines_are_compile_errors_with_a_position() {
    for (source, at) in [
        ("$X.unwrap()\nwhere\n$X: colour=red", "colour"),
        ("$X.unwrap()\nwhere\n$X: not colour=red", "colour"),
        ("$X.unwrap()\nwhere\n$X kind=identifier", "$X kind"),
        ("$X.unwrap()\nwhere\nnot $X: kind=identifier", "not $X"),
        ("$X.unwrap()\nwhere\ninside: field=body", "field"),
        ("$X.unwrap()\nwhere\ninside: name=test", "name"),
        ("$X.unwrap()\nwhere\noutside: kind=mod_item", "outside"),
    ] {
        let found = error(source);
        assert_eq!(
            found.kind,
            PatternErrorKind::UnknownConstraint,
            "{source:?}"
        );
        assert_eq!(found.offset, source.find(at).unwrap(), "{source:?}");
    }
    // An empty value is blamed where it should be: the end.
    let source = "$X.unwrap()\nwhere\n$X: kind=";
    let found = error(source);
    assert_eq!(found.kind, PatternErrorKind::UnknownConstraint);
    assert_eq!(found.offset, source.len());
}

#[test]
fn a_bad_regex_is_a_compile_error_at_its_value() {
    let source = "$X.unwrap()\nwhere\n$X: regex=a(b";
    let found = error(source);
    assert_eq!(found.kind, PatternErrorKind::InvalidRegex);
    assert_eq!(found.offset, source.find("a(b").unwrap());
}

#[test]
fn where_counts_only_alone_on_its_line() {
    // Rust's `where` clause is code, not the section.
    let pattern = pattern("fn $F<T>() where T: Copy {}");
    let source = "fn g<T>() where T: Copy {}";
    assert_eq!(pattern.find_in_text(source).unwrap().len(), 1);
    // A pattern error before the section keeps its offset in the whole.
    let found = error("foo(\nwhere\n$X: kind=identifier");
    assert_eq!(found.kind, PatternErrorKind::Syntax);
}

#[test]
fn source_keeps_the_where_section() {
    let text = "$X.unwrap()\nwhere\n$X: kind=identifier";
    assert_eq!(pattern(text).source(), text);
}

#[test]
fn text_and_regex_values_keep_trailing_spaces() {
    // Review F3: the value runs to the end of the line, spaces included.
    let found = pattern("$X.unwrap()\nwhere\n$X: text=a ")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert!(found.is_empty(), "{:?}", texts(UNWRAPS, &found));
    let found = pattern("$X.unwrap()\nwhere\n$X: regex=^a $\r\n")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert!(found.is_empty(), "{:?}", texts(UNWRAPS, &found));
    let source = r#"fn f() { g("a ").unwrap(); g("a").unwrap(); }"#;
    let found = pattern("g($S).unwrap()\nwhere\n$S: regex=a \"$")
        .find_in_text(source)
        .unwrap();
    assert_eq!(texts(source, &found), [r#"g("a ").unwrap()"#]);
    // A kind is one word: spaces after it don't count.
    let found = pattern("$X.unwrap()\nwhere\n$X: kind=identifier  ")
        .find_in_text(UNWRAPS)
        .unwrap();
    assert_eq!(texts(UNWRAPS, &found), ["a.unwrap()"]);
}
