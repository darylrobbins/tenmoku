//! Regression checks on the Kotlin and SQL query overrides.

#[cfg(any(feature = "kotlin", feature = "sql"))]
use super::query_overrides::{has, highlighted};

#[test]
#[cfg(feature = "kotlin")]
fn kotlin_calls_keep_function_highlights() {
    let text = "fun f() {\n    TODO()\n    FOO()\n    Foo.BAR()\n    val x = MAX_SIZE\n    val y = Foo.LIMIT\n}\n";
    let spans = highlighted("kotlin", text);
    for (text, highlight) in [
        ("TODO", "function.builtin"),
        ("FOO", "function"),
        ("BAR", "function"),
        ("MAX_SIZE", "constant"),
        ("LIMIT", "constant"),
    ] {
        assert!(has(&spans, text, highlight), "{text}: {spans:?}");
    }
    assert!(!has(&spans, "TODO", "constant"), "{spans:?}");
    assert!(!has(&spans, "FOO", "constant"), "{spans:?}");
    assert!(!has(&spans, "BAR", "constant"), "{spans:?}");
}

#[test]
#[cfg(feature = "sql")]
fn sql_numeric_forms() {
    let text = "SELECT 1e3, 1.5e-2, 0xFF, 0b101, 0o17, 1_000, 1_0.2_5, 7, 'x', '1e3';";
    let spans = highlighted("sql", text);
    for (text, highlight) in [
        ("1e3", "number"),
        ("1.5e-2", "number"),
        ("0xFF", "number"),
        ("0b101", "number"),
        ("0o17", "number"),
        ("1_000", "number"),
        ("1_0.2_5", "number"),
        ("7", "number"),
        ("'x'", "string"),
        ("'1e3'", "string"),
    ] {
        assert!(has(&spans, text, highlight), "{text}: {spans:?}");
    }
}

#[test]
#[cfg(feature = "sql")]
fn sql_signed_numbers_with_extras() {
    let text = "SELECT - 1e3, + 1.5e-2, - /* c */ 0xFF, -\n 7, - 'x', -1;";
    let spans = highlighted("sql", text);
    for (text, highlight) in [
        (" 1e3", "number"),
        (" 1.5e-2", "number"),
        (" 0xFF", "number"),
        ("\n 7", "number"),
        ("1", "number"),
        ("'x'", "string"),
    ] {
        assert!(has(&spans, text, highlight), "{text}: {spans:?}");
    }
    let strings = spans.iter().filter(|(_, h)| *h == "string").count();
    assert_eq!(strings, 1, "{spans:?}");
    assert!(has(&spans, "/* c */", "comment"), "{spans:?}");
}
