//! Compiling Rust patterns.

use fe_structural::{Pattern, PatternErrorKind, PatternNode};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("rust is a default feature of fe-syntax")
}

fn names(pattern: &Pattern) -> Vec<&str> {
    pattern
        .metavariables()
        .iter()
        .map(|m| m.name.as_str())
        .collect()
}

fn wildcards(node: &PatternNode, out: &mut Vec<(String, String)>) {
    if let Some(wildcard) = &node.wildcard {
        out.push((wildcard.name.clone(), node.kind.to_string()));
    }
    for child in &node.children {
        wildcards(child, out);
    }
}

#[test]
fn rust_call_pattern_compiles_with_two_single_metavariables() {
    let pattern = Pattern::compile(rust(), "foo($A, $B)").unwrap();
    assert_eq!(pattern.root_kind(), "call_expression");
    assert_eq!(names(&pattern), ["A", "B"]);
    assert!(
        pattern
            .metavariables()
            .iter()
            .all(|m| !m.multi && !m.anonymous)
    );
    let mut found = Vec::new();
    wildcards(pattern.root(), &mut found);
    assert_eq!(
        found,
        [
            ("A".to_owned(), "identifier".to_owned()),
            ("B".to_owned(), "identifier".to_owned())
        ]
    );
    assert_eq!(pattern.literals(), ["foo", "(", ",", ")"]);
}

#[test]
fn rust_method_chain_pattern_root_is_the_call_not_the_statement_wrapper() {
    let pattern = Pattern::compile(rust(), "$X.unwrap()").unwrap();
    assert_eq!(pattern.root_kind(), "call_expression");
    assert_eq!(names(&pattern), ["X"]);
    assert!(pattern.literals().contains(&"unwrap"));
    // Whitespace around the pattern doesn't move the root.
    let padded = Pattern::compile(rust(), "  \n$X.unwrap()\n").unwrap();
    assert_eq!(padded.root(), pattern.root());
}

#[test]
fn rust_item_pattern_root_is_the_item() {
    let pattern = Pattern::compile(rust(), "fn $NAME() {}").unwrap();
    assert_eq!(pattern.root_kind(), "function_item");
}

#[test]
fn rust_multi_and_anonymous_metavariables_compile() {
    let pattern = Pattern::compile(rust(), "foo($$$ARGS, $_, $_)").unwrap();
    assert_eq!(pattern.root_kind(), "call_expression");
    let vars: Vec<_> = pattern
        .metavariables()
        .iter()
        .map(|m| (m.name.as_str(), m.multi, m.anonymous))
        .collect();
    assert_eq!(
        vars,
        [
            ("ARGS", true, false),
            ("_any0", false, true),
            ("_any1", false, true)
        ]
    );
}

#[test]
fn pattern_with_a_syntax_error_reports_the_error_position_and_does_not_panic() {
    // The errors follow a metavariable, so their offsets map back through
    // the placeholder, which is longer than `$X`.
    let error = Pattern::compile(rust(), "let $X = ;").unwrap_err();
    assert_eq!(error.kind, PatternErrorKind::Syntax);
    assert_eq!(error.offset, "let $X ".len(), "{error}");
    let error = Pattern::compile(rust(), "foo($A, b c)").unwrap_err();
    assert_eq!(error.kind, PatternErrorKind::Syntax);
    assert_eq!(error.offset, "foo($A, b ".len(), "{error}");

    let error = Pattern::compile(rust(), "fn (").unwrap_err();
    assert_eq!(error.kind, PatternErrorKind::Syntax);
    assert!(error.offset <= 4, "{error}");

    let error = Pattern::compile(rust(), "   ").unwrap_err();
    assert_eq!(error.kind, PatternErrorKind::Empty);

    // A metavariable glued to other text is no node of its own.
    let error = Pattern::compile(rust(), "foo(x$A)").unwrap_err();
    assert!(
        matches!(
            error.kind,
            PatternErrorKind::MetavarNotANode | PatternErrorKind::Syntax
        ),
        "{error}"
    );

    let error = Pattern::compile(rust(), "foo($A, $$$A)").unwrap_err();
    assert_eq!(error.kind, PatternErrorKind::MixedMetavar);
    assert_eq!(error.offset, 8);
}

#[test]
fn placeholder_text_never_collides_with_pattern_text() {
    let pattern = Pattern::compile(rust(), "foo(__tsMV_A__, $A)").unwrap();
    assert_eq!(names(&pattern), ["A"]);
    // The identifier the user wrote stays a literal; only `$A` is a wildcard.
    assert!(pattern.literals().contains(&"__tsMV_A__"));
    let mut found = Vec::new();
    wildcards(pattern.root(), &mut found);
    assert_eq!(found, [("A".to_owned(), "identifier".to_owned())]);
}

#[test]
fn a_doubled_dollar_is_a_literal_dollar() {
    let pattern = Pattern::compile(rust(), "m!($$x)").unwrap();
    assert!(pattern.metavariables().is_empty());
    assert!(pattern.literals().iter().any(|l| l.contains('$')));
}
