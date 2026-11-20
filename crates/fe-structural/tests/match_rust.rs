//! Matching Rust patterns against Rust files.

use fe_structural::{Match, MatchOptions, Pattern, Stopped};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("rust is a default feature of fe-syntax")
}

fn pattern(source: &str) -> Pattern {
    Pattern::compile(rust(), source).unwrap()
}

fn texts<'s>(source: &'s str, matches: &[Match]) -> Vec<&'s str> {
    matches.iter().map(|m| &source[m.range.clone()]).collect()
}

fn capture<'s>(source: &'s str, found: &Match, name: &str) -> &'s str {
    &source[found.captures.single(name).expect("captured once")]
}

#[test]
fn rust_unwrap_pattern_finds_every_unwrap_call_in_a_file() {
    let source = r#"fn main() {
    let a = foo.unwrap();
    let b = bar().unwrap();
    // x.unwrap() in a comment
    let c = "y.unwrap()";
    let d = z.unwrap_or(1);
    baz(q.unwrap(), r . unwrap ());
}
"#;
    let tree = fe_syntax::parse_str(rust(), source, || false).unwrap();
    let found = pattern("$X.unwrap()")
        .find_all(source, &tree, &MatchOptions::default(), &mut || false)
        .unwrap();
    assert_eq!(
        texts(source, &found),
        [
            "foo.unwrap()",
            "bar().unwrap()",
            "q.unwrap()",
            "r . unwrap ()"
        ]
    );
    let receivers: Vec<_> = found.iter().map(|m| capture(source, m, "X")).collect();
    assert_eq!(receivers, ["foo", "bar()", "q", "r"]);
}

#[test]
fn match_ignores_whitespace_and_comments_between_tokens() {
    let pattern = pattern("foo(1,2)");
    let source = "fn f() { foo( 1, /* c */ 2 ); foo(1, // c\n 2); foo(1, 3); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(
        texts(source, &found),
        ["foo( 1, /* c */ 2 )", "foo(1, // c\n 2)"]
    );
}

#[test]
fn metavariable_binds_the_whole_subexpression() {
    let source = "fn f() { foo(a.b(c)); }";
    let found = pattern("foo($A)").find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(a.b(c))"]);
    assert_eq!(capture(source, &found[0], "A"), "a.b(c)");
    assert_eq!(found[0].captures.len(), 1);
}

#[test]
fn a_different_child_count_is_a_mismatch() {
    let source = "fn f() { foo(a, b); foo(); foo(c); }";
    let found = pattern("foo($A)").find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(c)"]);
}

#[test]
fn leaves_compare_by_text_and_keywords_by_kind() {
    let source = "fn f() { let x = 1; let mut y = 1; let z = 2; }";
    let found = pattern("let $V = 1;").find_in_text(source).unwrap();
    // `let mut` has a child more; `z = 2` has another literal.
    assert_eq!(texts(source, &found), ["let x = 1;"]);
}

#[test]
fn macro_pattern_compares_token_tree_leaves_by_text() {
    let source = "fn f() { foo!(x); foo!(y); bar!(x); foo!(a.b); foo!(x, y); }";
    let found = pattern("foo!($A)").find_in_text(source).unwrap();
    // A token tree is flat: `a.b` is three tokens, not one node.
    assert_eq!(texts(source, &found), ["foo!(x)", "foo!(y)"]);
}

#[test]
fn anonymous_metavariables_capture_nothing() {
    let source = "fn f() { foo(a, b); }";
    let found = pattern("foo($_, $B)").find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(a, b)"]);
    let names: Vec<_> = found[0].captures.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["B"]);
    assert_eq!(capture(source, &found[0], "B"), "b");
}

#[test]
fn nested_matches_are_reported_outermost_first_and_not_overlapping_by_default() {
    let source = "fn f() { foo(foo(foo(x))); foo(y); }";
    let found = pattern("foo($A)").find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(foo(foo(x)))", "foo(y)"]);
    assert_eq!(capture(source, &found[0], "A"), "foo(foo(x))");
    for pair in found.windows(2) {
        assert!(pair[0].range.end <= pair[1].range.start);
    }
}

#[test]
fn nested_matches_option_reports_inner_ones_too() {
    let source = "fn f() { foo(foo(foo(x))); foo(y); }";
    let found = pattern("foo($A)")
        .find_in_text_with(source, &MatchOptions { nested: true }, &mut || false)
        .unwrap();
    assert_eq!(
        texts(source, &found),
        ["foo(foo(foo(x)))", "foo(foo(x))", "foo(x)", "foo(y)"]
    );
    let captured: Vec<_> = found.iter().map(|m| capture(source, m, "A")).collect();
    assert_eq!(captured, ["foo(foo(x))", "foo(x)", "x", "y"]);
}

#[test]
fn cancel_callback_stops_a_match_over_a_large_generated_file_promptly() {
    let mut source = String::from("fn main() {\n");
    for i in 0..20_000 {
        source.push_str(&format!("    let v{i} = x{i}.unwrap();\n"));
    }
    source.push_str("}\n");
    let tree = fe_syntax::parse_str(rust(), &source, || false).unwrap();
    let pattern = pattern("$X.unwrap()");
    let opts = MatchOptions::default();

    // Not stopped, the callback is asked all through the walk.
    let mut asks = 0usize;
    let found = pattern
        .find_all(&source, &tree, &opts, &mut || {
            asks += 1;
            false
        })
        .unwrap();
    assert_eq!(found.len(), 20_000);
    assert!(asks > 100, "asked {asks} times");

    // Stopped on its third ask, it isn't asked again: the walk ends within
    // 1024 nodes of the request.
    let mut asks = 0usize;
    let result = pattern.find_all(&source, &tree, &opts, &mut || {
        asks += 1;
        asks == 3
    });
    assert_eq!(result, Err(Stopped::Cancelled));
    assert_eq!(asks, 3);

    // Stopped at once, it ends before any node.
    let result = pattern.find_all(&source, &tree, &opts, &mut || true);
    assert_eq!(result, Err(Stopped::Cancelled));
}

#[test]
fn a_multi_metavariable_is_unsupported_for_now() {
    let pattern = pattern("foo($$$ARGS)");
    assert_eq!(
        pattern.find_in_text("fn f() { foo(); }"),
        Err(Stopped::Unsupported)
    );
    let tree = fe_syntax::parse_str(rust(), "foo();", || false).unwrap();
    let result = pattern.find_all("foo();", &tree, &MatchOptions::default(), &mut || false);
    assert_eq!(result, Err(Stopped::Unsupported));
}

#[test]
fn missing_tokens_from_error_recovery_never_match() {
    // The `)` is MISSING, inserted by recovery.
    let source = "fn f() { foo(x; }";
    let found = pattern("foo($A)").find_in_text(source).unwrap();
    assert!(found.is_empty(), "{:?}", texts(source, &found));
}
