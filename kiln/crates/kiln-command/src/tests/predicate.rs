// SPDX-License-Identifier: LicenseRef-Proprietary

use super::*;

fn parse(source: &str) -> ContextPredicate {
    ContextPredicate::parse(source).unwrap_or_else(|error| panic!("{source:?}: {error}"))
}

fn id(name: &str) -> Box<ContextPredicate> {
    Box::new(ContextPredicate::identifier(name))
}

#[test]
fn parses_and_evaluates() {
    let stack = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    assert!(parse("editor").eval(&stack));
    assert!(parse("workbench && editor").eval(&stack));
    assert!(!parse("terminal").eval(&stack));

    let editable = parse("editor && !read_only");
    assert!(editable.eval(&stack));
    let read_only = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor").flag("read_only"));
    assert!(!editable.eval(&read_only));

    // The innermost frame that sets a key wins; an unset key equals nothing.
    let normal = parse("vim.mode == normal");
    let vim = ContextStack::new()
        .with(ContextFrame::new("workbench").set("vim.mode", "insert"))
        .with(ContextFrame::new("editor").set("vim.mode", "normal"));
    assert!(normal.eval(&vim));
    assert!(!parse("vim.mode != normal").eval(&vim));
    assert!(!normal.eval(&stack));
    assert!(parse("vim.mode != normal").eval(&stack));
    assert!(
        parse(r#"lang == "c++""#)
            .eval(&ContextStack::new().with(ContextFrame::new("editor").set("lang", "c++")))
    );

    // `!` binds tighter than `&&`, which binds tighter than `||`.
    use ContextPredicate::{And, Not, Or};
    assert_eq!(
        parse("a || b && c"),
        Or(id("a"), Box::new(And(id("b"), id("c"))))
    );
    assert_eq!(parse("!a && b"), And(Box::new(Not(id("a"))), id("b")));
    assert_eq!(
        parse("(a || b) && c"),
        And(Box::new(Or(id("a"), id("b"))), id("c"))
    );

    for source in [
        "a || b && c",
        "(a || b) && c",
        "a && (b && c)",
        "a || (b || c)",
        "!(a && b) || !c",
        "!!a",
        "!(vim.mode == normal) && x != \"two words\"",
        r#"key == "quote\" and \\""#,
        "  editor&&!read_only  ",
    ] {
        let predicate = parse(source);
        let printed = predicate.to_string();
        assert_eq!(
            parse(&printed),
            predicate,
            "{source:?} printed as {printed:?}"
        );
    }
    assert_eq!(parse("(a)&&(b||c)").to_string(), "a && (b || c)");

    let error = ContextPredicate::parse("editor &&").unwrap_err();
    assert_eq!(error.offset, 9, "{error}");
    assert_eq!(ContextPredicate::parse("a == ").unwrap_err().offset, 5);
    assert_eq!(ContextPredicate::parse("(a").unwrap_err().offset, 2);
    assert_eq!(ContextPredicate::parse("a b").unwrap_err().offset, 2);
    assert_eq!(ContextPredicate::parse("a & b").unwrap_err().offset, 2);
    assert_eq!(
        ContextPredicate::parse("k == \"open").unwrap_err().offset,
        5
    );
}
