//! A file's edits: in order, never overlapping, and checked by a reparse.

use fe_structural::{
    CHECK_LIMIT, Edit, Edits, EditsError, MatchOptions, Pattern, Rewrite, Stopped, apply_edits,
};

fn rust() -> &'static fe_syntax::Language {
    fe_syntax::language_by_name("rust").expect("rust is a default feature of fe-syntax")
}

fn compiled(pattern: &str, template: &str) -> (Pattern, Rewrite) {
    let pattern = Pattern::compile(rust(), pattern).unwrap();
    let rewrite = Rewrite::compile(&pattern, template).unwrap();
    (pattern, rewrite)
}

fn edits_with(pattern: &Pattern, rewrite: &Rewrite, source: &str, opts: &MatchOptions) -> Edits {
    let tree = fe_syntax::parse_str(rust(), source, || false).unwrap();
    pattern
        .edits(rewrite, source, &tree, opts, &mut || false)
        .unwrap()
}

fn edits(pattern: &Pattern, rewrite: &Rewrite, source: &str) -> Edits {
    edits_with(pattern, rewrite, source, &MatchOptions::default())
}

/// Rewrites one match at a time: each round reparses the text and rewrites
/// the first match that starts at or after the end of the last rewrite.
fn sequential(pattern: &Pattern, rewrite: &Rewrite, source: &str) -> String {
    let mut text = source.to_owned();
    let mut from = 0;
    loop {
        let all = edits(pattern, rewrite, &text);
        let Some(next) = all.edits.into_iter().find(|e| e.range.start >= from) else {
            return text;
        };
        from = next.range.start + next.text.len();
        text.replace_range(next.range, &next.text);
    }
}

#[test]
fn edits_are_in_file_order_non_overlapping_and_apply_to_the_same_text_as_a_sequential_rewrite() {
    let (pattern, rewrite) = compiled("foo($A, $B)", "bar($B, $A)");
    let source = "fn f() {\n    foo(foo(1, 2), foo(3, foo(4, 5)));\n    let x = foo(a, b) + foo(c, d);\n    foo(\n        e,\n        f,\n    );\n}\n";
    for opts in [MatchOptions::default(), MatchOptions { nested: true }] {
        let found = edits_with(&pattern, &rewrite, source, &opts);
        let ranges: Vec<_> = found.edits.iter().map(|e| e.range.clone()).collect();
        // Outermost first, the ones inside dropped, even with `nested`.
        assert_eq!(ranges.len(), 3, "{opts:?}: {ranges:?}");
        assert!(
            ranges.windows(2).all(|w| w[0].end <= w[1].start),
            "{ranges:?}"
        );
        assert_eq!(
            &source[ranges[0].clone()],
            "foo(foo(1, 2), foo(3, foo(4, 5)))"
        );
        let text = apply_edits(source, &found.edits);
        assert_eq!(text, sequential(&pattern, &rewrite, source));
        assert_eq!(
            text,
            "fn f() {\n    bar(foo(3, foo(4, 5)), foo(1, 2));\n    let x = bar(b, a) + bar(d, c);\n    foo(\n        e,\n        f,\n    );\n}\n"
        );
    }
    // Applying them back to front, one by one, gives the same text.
    let found = edits(&pattern, &rewrite, source);
    let mut text = source.to_owned();
    for edit in found.edits.iter().rev() {
        text.replace_range(edit.range.clone(), &edit.text);
    }
    assert_eq!(text, apply_edits(source, &found.edits));
}

#[test]
fn rewrite_that_breaks_the_syntax_is_flagged_by_the_reparse_check_and_a_good_one_is_not() {
    let source =
        "fn f() -> Option<u8> {\n    let a = x.unwrap();\n    g(y.unwrap());\n    Some(a)\n}\n";
    // `$X?` keeps the file parsing: nothing flagged.
    let (pattern, good) = compiled("$X.unwrap()", "$X?");
    let found = edits(&pattern, &good, source);
    assert!(found.checked);
    assert_eq!(found.edits.len(), 2);
    assert!(found.edits.iter().all(|e| !e.flagged), "{found:?}");
    // An unclosed call adds ERROR nodes: both flagged, and still returned.
    let (pattern, bad) = compiled("$X.unwrap()", "$X.unwrap(");
    let found = edits(&pattern, &bad, source);
    assert!(found.checked);
    assert_eq!(found.edits.len(), 2);
    assert!(found.edits.iter().all(|e| e.flagged), "{found:?}");
    let broken = apply_edits(source, &found.edits);
    let tree = fe_syntax::parse_str(rust(), &broken, || false).unwrap();
    assert!(tree.root_node().has_error());
}

#[test]
fn only_the_edits_that_break_the_syntax_are_flagged() {
    // A `let` is a statement: fine where the call was one, an error inside
    // an expression.
    let (pattern, rewrite) = compiled("foo($A)", "let y = $A");
    let source = "fn f() {\n    foo(1);\n    let x = foo(2);\n    foo(3);\n}\n";
    let found = edits(&pattern, &rewrite, source);
    assert!(found.checked);
    let flagged: Vec<_> = found.edits.iter().map(|e| e.flagged).collect();
    assert_eq!(flagged, [false, true, false]);
}

#[test]
fn errors_already_in_the_file_are_not_blamed_on_the_edits() {
    let (pattern, rewrite) = compiled("$X.unwrap()", "$X?");
    let source = "fn f() {\n    let a = x.unwrap();\n    let b = (;\n}\n";
    let found = edits(&pattern, &rewrite, source);
    assert!(found.checked);
    assert_eq!(found.edits.len(), 1);
    assert!(!found.edits[0].flagged);
}

#[test]
fn a_file_over_the_limit_skips_the_check() {
    let (pattern, rewrite) = compiled("$X.unwrap()", "$X.unwrap(");
    let mut source = String::from("fn f() {\n    x.unwrap();\n}\n");
    let filler = "// padding padding padding padding padding padding padding\n";
    while source.len() <= CHECK_LIMIT {
        source.push_str(filler);
    }
    let found = edits(&pattern, &rewrite, &source);
    assert!(!found.checked);
    assert_eq!(found.edits.len(), 1);
    assert!(!found.edits[0].flagged);
    // No match, nothing to check.
    let found = edits(&pattern, &rewrite, "fn f() {}\n");
    assert_eq!(
        found,
        Edits {
            edits: Vec::<Edit>::new(),
            checked: true
        }
    );
}

#[test]
fn stop_during_the_check_cancels() {
    let (pattern, rewrite) = compiled("$X.unwrap()", "$X.unwrap(");
    let source = "fn f() {\n    x.unwrap();\n}\n";
    let tree = fe_syntax::parse_str(rust(), source, || false).unwrap();
    // The search asks once, then the check's parses ask: stop there.
    let mut asked = 0;
    let result = pattern.edits(
        &rewrite,
        source,
        &tree,
        &MatchOptions::default(),
        &mut || {
            asked += 1;
            asked > 1
        },
    );
    assert_eq!(result, Err(EditsError::Stopped(Stopped::Cancelled)));
}

#[test]
fn carriage_returns_in_a_template_become_newlines_checked_or_not() {
    // T711 F3: parse_str refuses `\r`, and buffers hold `\n` only.
    let (pattern, rewrite) = compiled("foo($A)", "bar(\r\n    $A,\r    0,\r\n)");
    let source = "fn f() {\n    foo(x);\n}\n";
    let found = edits(&pattern, &rewrite, source);
    assert!(found.checked);
    assert_eq!(found.edits[0].text, "bar(\n    x,\n    0,\n)");
    assert!(!found.edits[0].flagged);
    let mut large = source.to_owned();
    while large.len() <= CHECK_LIMIT {
        large.push_str("// padding padding padding padding padding padding padding\n");
    }
    let found = edits(&pattern, &rewrite, &large);
    assert!(!found.checked);
    assert_eq!(found.edits[0].text, "bar(\n    x,\n    0,\n)");
}
