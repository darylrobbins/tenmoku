//! Multi wildcards (`$$$`) over runs of siblings, and `$_`.

use fe_structural::{Match, MatchOptions, Pattern, Stopped};

fn language(name: &str) -> &'static fe_syntax::Language {
    fe_syntax::language_by_name(name).expect("a default language of fe-syntax")
}

fn rust() -> &'static fe_syntax::Language {
    language("rust")
}

fn pattern(source: &str) -> Pattern {
    Pattern::compile(rust(), source).unwrap()
}

fn texts<'s>(source: &'s str, matches: &[Match]) -> Vec<&'s str> {
    matches.iter().map(|m| &source[m.range.clone()]).collect()
}

fn capture<'s>(source: &'s str, found: &Match, name: &str) -> &'s str {
    &source[found.captures.single(name).expect("one range")]
}

#[test]
fn multi_wildcard_matches_zero_one_and_many_arguments() {
    let pattern = pattern("foo($$$ARGS)");
    let source = "fn f() { foo(); foo(a); foo(a, b, c); bar(a); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo()", "foo(a)", "foo(a, b, c)"]);
    let args: Vec<_> = found.iter().map(|m| capture(source, m, "ARGS")).collect();
    assert_eq!(args, ["", "a", "a, b, c"]);
    // No arguments: empty, just inside the `(`.
    let empty = found[0].captures.single("ARGS").unwrap();
    assert_eq!(empty, 13..13);
}

#[test]
fn multi_wildcard_in_the_middle_backtracks() {
    let pattern = pattern("foo($A, $$$MID, $Z)");
    let source = "fn f() { foo(1,2,3,4); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(1,2,3,4)"]);
    assert_eq!(capture(source, &found[0], "A"), "1");
    assert_eq!(capture(source, &found[0], "MID"), "2,3");
    assert_eq!(capture(source, &found[0], "Z"), "4");
}

#[test]
fn multi_wildcard_captures_cover_from_first_to_last_node_including_separators() {
    let pattern = pattern("foo($A, $$$MID, $Z)");
    let source = "fn f() { foo(1, 2, 3, 4); }";
    let found = pattern.find_in_text(source).unwrap();
    let mid = found[0].captures.get("MID").unwrap();
    assert_eq!(mid.len(), 1, "one range for the run: {mid:?}");
    assert_eq!(&source[mid[0].clone()], "2, 3");
}

#[test]
fn multi_wildcard_run_covers_comments_between_its_nodes() {
    let pattern = pattern("foo($$$ARGS)");
    let source = "fn f() { foo(a, /* c */ b); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(capture(source, &found[0], "ARGS"), "a, /* c */ b");
}

#[test]
fn multi_wildcard_needs_the_fixed_siblings_around_it() {
    // The pattern's `,` is a sibling the file must have too.
    let pattern = pattern("foo($$$A, last)");
    let source = "fn f() { foo(last); foo(a, b, last); foo(a, last, b); foo(); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(a, b, last)"]);
    assert_eq!(capture(source, &found[0], "A"), "a, b");
}

#[test]
fn two_multi_wildcards_in_a_row_split_with_the_first_lazy() {
    // A macro's token tree takes them side by side.
    let pattern = pattern("foo!($$$A $$$B)");
    let source = "fn f() { foo!(x, y); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(capture(source, &found[0], "A"), "");
    assert_eq!(capture(source, &found[0], "B"), "x, y");
}

#[test]
fn unnamed_multi_wildcard_matches_and_captures_nothing() {
    let pattern = pattern("foo($$$, $LAST)");
    let source = "fn f() { foo(a, b, c); foo(); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(a, b, c)"]);
    let names: Vec<_> = found[0].captures.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["LAST"]);
    assert_eq!(capture(source, &found[0], "LAST"), "c");
}

#[test]
fn anonymous_single_wildcard_matches_one_node_and_binds_nothing() {
    let pattern = pattern("foo($_, $_)");
    let source = "fn f() { foo(a); foo(a, b); foo(a, b, c); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(a, b)"]);
    assert!(found[0].captures.is_empty());
}

#[test]
fn repeated_multi_wildcard_requires_equal_text() {
    let pattern = pattern("foo($$$A) + bar($$$A)");
    let source = "fn f() { foo(x, y) + bar(x, y); foo(x) + bar(y); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(x, y) + bar(x, y)"]);
}

#[test]
fn repeated_name_inside_one_list_backtracks_to_equal_runs() {
    let pattern = pattern("foo($$$A, sep, $$$A)");
    let source = "fn f() { foo(x, y, sep, x, y); foo(x, sep, y); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(x, y, sep, x, y)"]);
    assert_eq!(capture(source, &found[0], "A"), "x, y");
}

#[test]
fn statement_list_multi_wildcard_matches_a_block_body() {
    let pattern = pattern("if $C { $$$BODY }");
    let source = "fn f() { if ready { start(); go(); } if done {} if x { y } }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(
        texts(source, &found),
        ["if ready { start(); go(); }", "if done {}", "if x { y }"]
    );
    assert_eq!(capture(source, &found[0], "C"), "ready");
    assert_eq!(capture(source, &found[0], "BODY"), "start(); go();");
    assert_eq!(capture(source, &found[1], "BODY"), "");
    assert_eq!(capture(source, &found[2], "BODY"), "y");
}

#[test]
fn python_block_multi_wildcard_takes_the_statements() {
    let pattern = Pattern::compile(language("python"), "def $F():\n    $$$BODY").unwrap();
    let source = "def a():\n    x = 1\n    return x\n\ndef b(y):\n    pass\n";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(capture(source, &found[0], "F"), "a");
    assert_eq!(capture(source, &found[0], "BODY"), "x = 1\n    return x");
}

#[test]
fn multi_wildcard_alone_matches_one_node() {
    let pattern = pattern("$$$ALL");
    let source = "fn f() {}";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["fn f() {}"]);
    assert_eq!(capture(source, &found[0], "ALL"), "fn f() {}");
}

fn call_with(args: usize) -> String {
    let list: Vec<String> = (0..args).map(|i| format!("a{i}")).collect();
    format!("fn f() {{ foo({}); }}", list.join(", "))
}

#[test]
fn pathological_multi_wildcards_stay_polynomial() {
    let source = call_with(200);
    let tree = fe_syntax::parse_str(rust(), &source, || false).unwrap();
    let opts = MatchOptions::default();
    // Allowed a million steps (asked every 1024), each search must end on
    // its own: plain backtracking over 401 siblings takes far more.
    let budget = 1_000;
    for (text, matches) in [
        ("foo($$$A, $$$B, $$$C)", 1),
        // Fails, after trying every split: the last argument isn't `zz`.
        ("foo($$$A, $$$B, $$$C, zz)", 0),
        ("foo($$$A, a7, $$$B, a9, $$$C, zz)", 0),
    ] {
        let mut asks = 0usize;
        let found = pattern(text)
            .find_all(&source, &tree, &opts, &mut || {
                asks += 1;
                asks > budget
            })
            .unwrap_or_else(|stopped| panic!("{text}: {stopped} after {asks} asks"));
        assert_eq!(found.len(), matches, "{text}");
    }
}

#[test]
fn backtracking_asks_the_stop_callback() {
    let source = call_with(200);
    let tree = fe_syntax::parse_str(rust(), &source, || false).unwrap();
    let pattern = pattern("foo($$$A, a7, $$$B, a9, $$$C, zz)");
    // The file has few nodes; the asks after the first come from matching.
    let mut asks = 0usize;
    let result = pattern.find_all(&source, &tree, &MatchOptions::default(), &mut || {
        asks += 1;
        asks == 3
    });
    assert_eq!(result, Err(Stopped::Cancelled));
    assert_eq!(asks, 3);
}

#[test]
fn a_long_run_of_comments_in_a_candidate_asks_the_stop_callback() {
    // T709 F2: extras inside one candidate count towards the asks.
    let mut source = String::from("fn f() { foo(1, ");
    for _ in 0..10_000 {
        source.push_str("/**/ ");
    }
    source.push_str("2); }");
    let tree = fe_syntax::parse_str(rust(), &source, || false).unwrap();
    let pattern = pattern("foo(1, 2)");
    let mut asks = 0usize;
    let result = pattern.find_all(&source, &tree, &MatchOptions::default(), &mut || {
        asks += 1;
        asks == 2
    });
    assert_eq!(result, Err(Stopped::Cancelled));
    let found = pattern.find_in_text(&source).unwrap();
    assert_eq!(found.len(), 1);
}

#[test]
fn a_comment_that_is_not_an_extra_is_compared_like_any_node() {
    // T709 F1: gitignore's `comment` is a node of the grammar, not an
    // extra, so a pattern holding one must match it in the file.
    let gitignore = language("gitignore");
    let pattern = Pattern::compile(gitignore, "# build output\ntarget/\n").unwrap();
    let source = "# build output\ntarget/\n";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), [source]);
    let other = "# other\ntarget/\n";
    assert!(pattern.find_in_text(other).unwrap().is_empty());
}

#[test]
fn a_later_mismatch_retries_an_earlier_lists_split() {
    // Review F1: `foo`'s list first gives A nothing and B `x, y`; `bar(x)`
    // then fails, and the search goes back to try A = `x`, B = `y`.
    let pattern = pattern("foo($$$A, $$$B) + bar($$$A)");
    let source = "fn f() { foo(x, y) + bar(x); foo(x, y) + bar(z); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["foo(x, y) + bar(x)"]);
    assert_eq!(capture(source, &found[0], "A"), "x");
    assert_eq!(capture(source, &found[0], "B"), "y");
}

#[test]
fn a_later_mismatch_retries_a_nested_lists_split() {
    // `bar`'s list first gives B `2, 3`, which the outer `3` rejects.
    let pattern = pattern("foo(bar($$$A, $$$B), $$$B)");
    let source = "fn f() { foo(bar(1, 2, 3), 3); }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(capture(source, &found[0], "A"), "1, 2");
    assert_eq!(capture(source, &found[0], "B"), "3");
}

#[test]
fn more_than_one_varying_repeated_name_is_refused_at_compile() {
    // Review F2: matching several such names is NP-hard, so the pattern is
    // refused, at the second one's second use.
    let text = "foo($$$A, $$$B, $$$A, $$$B, zz)";
    let error = Pattern::compile(rust(), text).unwrap_err();
    assert_eq!(error.kind, fe_structural::PatternErrorKind::VaryingRepeats);
    assert_eq!(error.offset, text.rfind("$$$B").unwrap());
    // Repeats whose first use is fixed are fine, as is one that varies.
    for text in [
        "foo($A, $$$B) + bar($A) + baz($$$B)",
        "foo($$$A, $$$B) + bar($$$A, $X) + baz($X)",
        "foo($$$A, $X, $$$B) + bar($$$A)",
    ] {
        Pattern::compile(rust(), text).unwrap_or_else(|e| panic!("{text}: {e}"));
    }
}

/// How many times a search over a call of `args` arguments asked `stop`
/// (once per 1024 steps), never stopping it.
fn asks(text: &str, args: usize) -> usize {
    let source = call_with(args);
    let tree = fe_syntax::parse_str(rust(), &source, || false).unwrap();
    let mut asks = 0usize;
    let found = pattern(text)
        .find_all(&source, &tree, &MatchOptions::default(), &mut || {
            asks += 1;
            false
        })
        .unwrap();
    assert!(found.is_empty(), "{text}");
    asks
}

#[test]
fn a_repeated_multi_name_stays_polynomial() {
    // Review F2: one varying repeated name and a failing tail. Doubling the
    // arguments multiplies the steps by at most about 2³, where
    // backtracking over every split grows exponentially with the pattern.
    for text in [
        "foo($$$A, $$$B, $$$A, zz)",
        "foo($$$A, $$$B, $$$C, $$$A, zz)",
        "foo($$$A, $X, $$$B) + bar($$$A, zz)",
    ] {
        let (small, large) = (asks(text, 50), asks(text, 100));
        assert!(
            large <= 10 * small.max(2),
            "{text}: {small} then {large} asks"
        );
    }
}

#[test]
fn a_pattern_ended_by_the_parse_matches_without_the_suffix() {
    // T708 F2: `let $X = 1` parses only with a `;` after it; the `;` isn't
    // part of the pattern, nor a literal of it.
    let pattern = pattern("let $X = 1");
    assert!(
        !pattern.literals().contains(&";"),
        "{:?}",
        pattern.literals()
    );
    let source = "fn f() { let a = 1; let b = 2; }";
    let found = pattern.find_in_text(source).unwrap();
    assert_eq!(texts(source, &found), ["let a = 1;"]);
}
