//! Printf verbs in Go format strings colour apart from the string.

#![cfg(all(feature = "go", feature = "printf"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(language_by_name("go").unwrap(), Buffer::new(text).current());
    syntax.finish(syntax.parse_job().run());
    loop {
        let spans = syntax.highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            return spans
                .into_iter()
                .map(|span| (text[span.range].to_string(), span.highlight.name()))
                .collect();
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

fn placeholders(call: &str) -> Vec<String> {
    let found = spans(&format!("package p\nfunc f() {{\n\t{call}\n}}\n"));
    found
        .into_iter()
        .filter(|(_, h)| *h == "string.placeholder")
        .map(|(t, _)| t)
        .collect()
}

#[test]
fn verb_in_errorf_is_a_placeholder() {
    let found = spans("package p\nfunc f() {\n\tfmt.Errorf(\"create data dir: %w\", err)\n}\n");
    assert!(
        found.contains(&("%w".to_string(), "string.placeholder")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(t, h)| t.contains("create data dir: ") && *h == "string"),
        "{found:?}"
    );
}

#[test]
fn every_verb_form_is_one_placeholder() {
    for verb in [
        "%v", "%d", "%s", "%q", "%x", "%5.2f", "%-8s", "%+v", "%[1]d", "%*d",
    ] {
        assert_eq!(
            placeholders(&format!("fmt.Printf(\"a {verb} b\", x)")),
            vec![verb.to_string()],
            "{verb}"
        );
    }
    let found = spans("package p\nfunc f() {\n\tfmt.Printf(\"100%% sure\")\n}\n");
    assert!(found.contains(&("%%".to_string(), "escape")), "{found:?}");
    assert!(
        !found.iter().any(|(_, h)| *h == "string.placeholder"),
        "{found:?}"
    );
}

#[test]
fn printf_family_and_raw_strings_are_covered() {
    for call in [
        "fmt.Printf(\"%d\", n)",
        "fmt.Sprintf(\"%d\", n)",
        "log.Printf(\"%d\", n)",
        "t.Errorf(\"%d\", n)",
        "fmt.Fprintf(w, \"%d\", n)",
        "fmt.Printf(`%d`, n)",
    ] {
        assert_eq!(placeholders(call), vec!["%d".to_string()], "{call}");
    }
}

#[test]
fn non_format_calls_stay_plain() {
    for call in [
        "fmt.Println(\"100%d\")",
        "x.Do(\"%d\")",
        "fmt.Printf(format, a)",
        "fmt.Printf(format, \"%d\")",
        "fmt.Sscanf(\"100%d\", format, &n)",
        "fmt.Fprintf(w, format, \"%d\")",
    ] {
        assert!(placeholders(call).is_empty(), "{call}");
    }
}

#[test]
fn go_verbs_followed_by_text() {
    for (call, verb) in [
        ("fmt.Errorf(\"%w happened\", e)", "%w"),
        ("fmt.Errorf(\"%w: x\", e)", "%w"),
        ("fmt.Printf(\"%t ok\", b)", "%t"),
    ] {
        assert_eq!(placeholders(call), vec![verb.to_string()], "{call}");
        let found = spans(&format!("package p\nfunc f() {{\n\t{call}\n}}\n"));
        assert!(
            found
                .iter()
                .any(|(t, h)| *h == "string" && !t.contains('%') && t.len() > 1),
            "{call}: {found:?}"
        );
    }
    let found = spans("package p\nfunc f() {\n\tfmt.Sscanf(in, \"%d\", &n)\n}\n");
    assert!(
        found.contains(&("%d".to_string(), "string.placeholder")),
        "{found:?}"
    );
}

#[test]
fn space_flag_is_part_of_the_verb() {
    assert_eq!(
        placeholders("fmt.Printf(\"% d\", n)"),
        vec!["% d".to_string()]
    );
}

#[test]
fn c_size_modifiers_are_not_go_verbs() {
    assert_eq!(
        placeholders("fmt.Printf(\"%ld %zu\", a, b)"),
        vec!["%l".to_string(), "%z".to_string()]
    );
}

#[test]
fn appendf_format_is_the_second_argument() {
    assert_eq!(
        placeholders("fmt.Appendf(b, \"%d\", n)"),
        vec!["%d".to_string()]
    );
    assert_eq!(
        placeholders("fmt.Appendf(b, `%d`, n)"),
        vec!["%d".to_string()]
    );
    assert!(placeholders("fmt.Appendf(\"%d\", n)").is_empty());
    assert!(placeholders("fmt.Appendf(b, format, \"%d\")").is_empty());
}

#[test]
fn a_value_argument_is_not_a_format() {
    assert_eq!(
        placeholders("fmt.Printf(\"%s\", \"%d\")"),
        vec!["%s".to_string()]
    );
}
