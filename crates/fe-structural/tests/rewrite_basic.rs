//! Rewrite templates: captures substituted into literal text.

use fe_structural::{
    Edits, EditsError, MatchOptions, Pattern, Rewrite, RewriteError, RewriteErrorKind, apply_edits,
};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("rust is a default feature of fe-syntax")
}

fn pattern(source: &str) -> Pattern {
    Pattern::compile(rust(), source).unwrap()
}

/// The edits rewriting `source` with `pattern` and `template`, and the
/// text they give.
fn rewrite(pattern_source: &str, template: &str, source: &str) -> (Edits, String) {
    let pattern = pattern(pattern_source);
    let rewrite = Rewrite::compile(&pattern, template).unwrap();
    let tree = fe_syntax::parse_str(rust(), source, || false).unwrap();
    let edits = pattern
        .edits(
            &rewrite,
            source,
            &tree,
            &MatchOptions::default(),
            &mut || false,
        )
        .unwrap();
    let text = apply_edits(source, &edits.edits);
    (edits, text)
}

fn compile_error(pattern_source: &str, template: &str) -> RewriteError {
    Rewrite::compile(&pattern(pattern_source), template).unwrap_err()
}

#[test]
fn swap_arguments_rewrites_foo_a_b_to_bar_b_a() {
    let source =
        "fn main() {\n    foo(1, 2);\n    let x = foo(a.b, c());\n    baz(foo(\"s\", 'c'));\n}\n";
    let (edits, text) = rewrite("foo($A, $B)", "bar($B, $A)", source);
    assert!(edits.checked);
    let pairs: Vec<_> = edits
        .edits
        .iter()
        .map(|e| (&source[e.range.clone()], e.text.as_str(), e.flagged))
        .collect();
    assert_eq!(
        pairs,
        [
            ("foo(1, 2)", "bar(2, 1)", false),
            ("foo(a.b, c())", "bar(c(), a.b)", false),
            ("foo(\"s\", 'c')", "bar('c', \"s\")", false),
        ]
    );
    assert_eq!(
        text,
        "fn main() {\n    bar(2, 1);\n    let x = bar(c(), a.b);\n    baz(bar('c', \"s\"));\n}\n"
    );
}

#[test]
fn unwrap_becomes_question_mark_with_the_receiver_kept() {
    let source = "fn f() -> Option<u8> {\n    let a = x.unwrap();\n    let b = g(1).unwrap().y.unwrap();\n    Some(a)\n}\n";
    let (edits, text) = rewrite("$X.unwrap()", "$X?", source);
    // The outer `….y.unwrap()` is taken; the `g(1).unwrap()` in its
    // receiver is source text, not rewritten.
    assert_eq!(edits.edits.len(), 2);
    assert!(edits.edits.iter().all(|e| !e.flagged));
    assert_eq!(
        text,
        "fn f() -> Option<u8> {\n    let a = x?;\n    let b = g(1).unwrap().y?;\n    Some(a)\n}\n"
    );
}

#[test]
fn multi_capture_substitutes_source_between_first_and_last_with_separators() {
    let source =
        "fn f() {\n    foo(a,  b ,c);\n    foo(x);\n    foo();\n    foo(/* only */ y);\n}\n";
    let (edits, text) = rewrite("foo($$$ARGS)", "bar(0, $$$ARGS)", source);
    assert_eq!(edits.edits.len(), 4);
    let texts: Vec<_> = edits.edits.iter().map(|e| e.text.as_str()).collect();
    // Separators as the file spells them; an empty run is nothing; a
    // comment before the first node is outside the run.
    assert_eq!(
        texts,
        ["bar(0, a,  b ,c)", "bar(0, x)", "bar(0, )", "bar(0, y)"]
    );
    assert!(text.contains("bar(0, a,  b ,c);"));
}

#[test]
fn dollar_dollar_in_a_template_is_a_literal_dollar() {
    let source = "fn f() { foo(a); }";
    let (edits, text) = rewrite("foo($A)", "m!($$ $A $$$$ $x $Abc $ $$$)", source);
    assert_eq!(edits.edits[0].text, "m!($ a $$ $x $Abc $ $$)");
    assert_eq!(text, "fn f() { m!($ a $$ $x $Abc $ $$); }");
    // `$$` then a name is a dollar and the name, not a capture.
    let (_, text) = rewrite("foo($A)", "$$A", source);
    assert_eq!(text, "fn f() { $A; }");
}

#[test]
fn unknown_metavariable_in_a_template_is_an_error_with_a_position() {
    let error = compile_error("foo($A)", "bar($A, $B)");
    assert_eq!(
        error,
        RewriteError {
            kind: RewriteErrorKind::UnknownMetavar,
            offset: 8
        }
    );
    assert_eq!(
        error.to_string(),
        "metavariable the pattern does not bind at byte 8"
    );
    let error = compile_error("foo($$$A)", "bar($$$B)");
    assert_eq!(
        (error.kind, error.offset),
        (RewriteErrorKind::UnknownMetavar, 4)
    );
}

#[test]
fn single_and_multi_must_be_written_as_in_the_pattern() {
    let error = compile_error("foo($$$A)", "bar($A)");
    assert_eq!(
        (error.kind, error.offset),
        (RewriteErrorKind::MixedMetavar, 4)
    );
    let error = compile_error("foo($A)", "x $$$A");
    assert_eq!(
        (error.kind, error.offset),
        (RewriteErrorKind::MixedMetavar, 2)
    );
}

#[test]
fn anonymous_metavariables_capture_nothing_so_a_template_cannot_use_them() {
    for template in ["bar($_)", "bar($_A)", "bar($$$_B)"] {
        let error = compile_error("foo($_, $$$)", template);
        assert_eq!(
            (error.kind, error.offset),
            (RewriteErrorKind::AnonymousMetavar, 4),
            "{template}"
        );
    }
}

#[test]
fn a_repeated_capture_and_a_template_with_none_both_work() {
    let source = "fn f() { foo(a); }";
    let (_, text) = rewrite("foo($A)", "$A + $A", source);
    assert_eq!(text, "fn f() { a + a; }");
    let (_, text) = rewrite("foo($A)", "()", source);
    assert_eq!(text, "fn f() { (); }");
    let pattern = pattern("foo($A)");
    assert_eq!(Rewrite::compile(&pattern, "x$A").unwrap().template(), "x$A");
}

#[test]
fn edits_are_the_pairs_replacer_gives() {
    let source = "fn f() { foo(a); foo(b); }";
    let (edits, _) = rewrite("foo($A)", "bar($A)", source);
    assert_eq!(
        edits.edits[0].clone().into_pair(),
        (9..15, "bar(a)".to_owned())
    );
    assert_eq!(
        edits.into_pairs(),
        [(9..15, "bar(a)".to_owned()), (17..23, "bar(b)".to_owned())]
    );
}

#[test]
fn a_rewrite_used_with_another_pattern_is_an_error_not_a_deletion() {
    // T711 F4: `$A` compiled for `foo($A)` would substitute nothing for
    // `bar($B)`'s matches.
    let source = "fn f() { bar(x); foo(x); }";
    let tree = fe_syntax::parse_str(rust(), source, || false).unwrap();
    let other = pattern("bar($B)");
    let rewrite = Rewrite::compile(&pattern("foo($A)"), "$A").unwrap();
    let result = other.edits(
        &rewrite,
        source,
        &tree,
        &MatchOptions::default(),
        &mut || false,
    );
    assert_eq!(
        result,
        Err(EditsError::WrongPattern(RewriteError {
            kind: RewriteErrorKind::UnknownMetavar,
            offset: 0
        }))
    );
    // Bound, but multi where the rewrite wants single.
    let multi = pattern("bar($$$A)");
    let result = multi.edits(
        &rewrite,
        source,
        &tree,
        &MatchOptions::default(),
        &mut || false,
    );
    assert!(
        matches!(result, Err(EditsError::WrongPattern(ref e)) if e.kind == RewriteErrorKind::MixedMetavar),
        "{result:?}"
    );
    // Another pattern binding the same names fits.
    let same = pattern("baz($A) + foo($A)");
    assert!(
        same.edits(
            &rewrite,
            source,
            &tree,
            &MatchOptions::default(),
            &mut || false
        )
        .is_ok()
    );
}
