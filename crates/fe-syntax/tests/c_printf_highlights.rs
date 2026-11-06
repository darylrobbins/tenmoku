//! Printf verbs in C and C++ format strings colour apart from the string.

#![cfg(all(feature = "c", feature = "printf"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(lang: &str, text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(language_by_name(lang).unwrap(), Buffer::new(text).current());
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

fn found(lang: &str, call: &str, highlight: &str) -> Vec<String> {
    spans(lang, &format!("void f(void) {{\n    {call};\n}}\n"))
        .into_iter()
        .filter(|(_, h)| *h == highlight)
        .map(|(t, _)| t)
        .collect()
}

fn placeholders(call: &str) -> Vec<String> {
    found("c", call, "string.placeholder")
}

#[test]
fn verb_in_printf_is_a_placeholder() {
    let all = spans("c", "void f(void) {\n    printf(\"%s: %d\\n\", a, b);\n}\n");
    assert!(
        all.contains(&("%s".to_string(), "string.placeholder")),
        "{all:?}"
    );
    assert!(
        all.contains(&("%d".to_string(), "string.placeholder")),
        "{all:?}"
    );
    assert!(
        all.iter()
            .any(|(t, h)| t.starts_with(": ") && *h == "string"),
        "{all:?}"
    );
}

#[test]
fn every_c_conversion_form_is_one_placeholder() {
    for d in [
        "%5.2f", "%-8s", "%ld", "%llu", "%zu", "%#x", "%*d", "%1$d", "%hhd",
    ] {
        assert_eq!(
            placeholders(&format!("printf(\"a {d} b\", x)")),
            vec![d.to_string()],
            "{d}"
        );
    }
    assert_eq!(
        found("c", "printf(\"a %% b\")", "escape"),
        vec!["%%".to_string()]
    );
    assert!(placeholders("printf(\"a %% b\")").is_empty());
}

#[test]
fn invalid_directives_paint_nothing() {
    for d in ["% ", "%5", "%y", "%"] {
        assert!(
            placeholders(&format!("printf(\"a{d}\")")).is_empty(),
            "{d:?}"
        );
    }
}

#[test]
fn format_position_per_function() {
    for call in [
        "fprintf(f, \"%d\", n)",
        "snprintf(buf, n, \"%d\", x)",
        "syslog(LOG_ERR, \"%d\", s)",
        "dprintf(fd, \"%d\", s)",
        "vsnprintf(buf, n, \"%d\", ap)",
        "vprintf(\"%d\", ap)",
    ] {
        assert_eq!(placeholders(call), vec!["%d".to_string()], "{call}");
    }
    for call in [
        "printf(fmt, a)",
        "puts(\"%d\")",
        "foo(\"%d\")",
        "printf(a, \"%d\")",
        "fprintf(\"%d\", f)",
        "snprintf(buf, \"%d\", n)",
    ] {
        assert!(placeholders(call).is_empty(), "{call}");
    }
}

#[test]
fn concatenated_pieces_each_inject() {
    assert_eq!(
        placeholders("printf(\"%s \" \"%d\", a, b)"),
        vec!["%s".to_string(), "%d".to_string()]
    );
}

#[cfg(feature = "cpp")]
mod cpp {
    use super::*;

    fn cpp(call: &str) -> Vec<String> {
        found("cpp", call, "string.placeholder")
    }

    #[test]
    fn cpp_std_qualified_and_raw_strings() {
        assert_eq!(cpp("std::printf(\"%d\", n)"), vec!["%d".to_string()]);
        assert_eq!(cpp("printf(\"%d\", n)"), vec!["%d".to_string()]);
        assert_eq!(cpp("printf(R\"(%d)\", n)"), vec!["%d".to_string()]);
        assert!(cpp("foo::printf(\"%d\", n)").is_empty());
        assert!(cpp("printf(fmt, \"%d\")").is_empty());
    }
}
