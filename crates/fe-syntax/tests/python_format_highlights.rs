//! Python's `%` and `str.format` placeholders colour apart from the string.

#![cfg(all(feature = "python", feature = "printf"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("python").unwrap(),
        Buffer::new(text).current(),
    );
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

fn of(text: &str, highlight: &str) -> Vec<String> {
    spans(text)
        .into_iter()
        .filter(|(_, h)| *h == highlight)
        .map(|(t, _)| t)
        .collect()
}

fn placeholders(text: &str) -> Vec<String> {
    of(text, "string.placeholder")
}

#[test]
fn percent_operator_verbs_are_placeholders() {
    let text = "x = \"%s: %5.2f %(k)d %%\" % args\n";
    assert_eq!(placeholders(text), ["%s", "%5.2f", "%(k)d"]);
    assert_eq!(of(text, "escape"), ["%%"]);
    assert_eq!(placeholders("x = b\"%s\" % a\n"), ["%s"]);
}

#[test]
fn format_method_fields_are_placeholders() {
    let text = "x = \"{} {0} {name!r:>{width}} {{ }}\".format(a)\n";
    assert_eq!(placeholders(text), ["{}", "{0}", "{name!r:>{width}}"]);
    assert_eq!(of(text, "escape"), ["{{", "}}"]);
}

#[test]
fn logging_calls_colour_percent_verbs() {
    assert_eq!(
        placeholders("log.info(\"%s took %d\", a, b)\n"),
        ["%s", "%d"]
    );
    assert_eq!(placeholders("log.log(lvl, \"%s\", a)\n"), ["%s"]);
    assert!(placeholders("foo.bar(\"%d\")\n").is_empty());
    assert!(placeholders("log.log(\"%d\")\n").is_empty());
}

#[test]
fn fstrings_and_non_literals_are_untouched() {
    for text in [
        "x = f\"{x}\" % y\n",
        "x = fmt % y\n",
        "x = name.format(1)\n",
        "x = f\"{x}\".format(1)\n",
        "log.info(f\"{x} %d\")\n",
    ] {
        assert!(of(text, "escape").is_empty(), "{text}");
        let found = placeholders(text);
        // Only an f-string's own braces, as python.scm paints them.
        assert!(
            found.iter().all(|t| t == "{" || t == "}"),
            "{text}: {found:?}"
        );
    }
    let plain = spans("x = f\"{x}\"\n");
    let mixed = spans("x = f\"{x}\" % y\n");
    let before_operator: Vec<_> = mixed.iter().take_while(|s| s.0 != "%").collect();
    assert_eq!(plain.iter().collect::<Vec<_>>(), before_operator);
}
