//! Makefiles get their own highlights: targets, prerequisites, variables,
//! directives, and bash in recipe lines.

#![cfg(all(feature = "make", feature = "bash"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

const FIXTURE: &str = "\
.PHONY: dev web build test
BINARY := bin/app
include config.mk
ifeq ($(CI),1)
export FOO
endif
define greet
echo hi
endef

dev:
\t@echo \"Go API on :8080\"
\t@( SEED=1 ADMIN_PASSWORD=$${ADMIN_PASSWORD:-dev} go run ./cmd/server & \\
\t  sleep 1 && \\
\t  echo started )

web:
\tnpm --prefix web ci || npm --prefix web install

build: web ## build it
\t-$(BINARY) $@ $< $^ $(shell pwd) $(wildcard *.go)

test: test-go test-web
\t+go test ./...
\tgo run ./cmd &

ifdef X
A := 1
else
A := 2
endif
";

fn spans(language: &str, text: &str) -> Vec<(String, &'static str)> {
    spans_at(language, text)
        .into_iter()
        .map(|(range, name)| (text[range].to_string(), name))
        .collect()
}

/// `(byte range, highlight)` for every span of `text`.
fn spans_at(language: &str, text: &str) -> Vec<(std::ops::Range<usize>, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name(language).unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    loop {
        let spans = syntax.highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            return spans
                .into_iter()
                .map(|span| (span.range, span.highlight.name()))
                .collect();
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

fn has(found: &[(String, &str)], text: &str, highlight: &str) {
    assert!(
        found.iter().any(|(t, h)| t == text && *h == highlight),
        "missing ({text:?}, {highlight:?}) in {found:?}"
    );
}

#[test]
fn target_and_prerequisites_are_coloured() {
    let found = spans("make", FIXTURE);
    has(&found, "build", "function");
    for name in ["web", "test-go", "test-web"] {
        assert!(
            found
                .iter()
                .any(|(t, h)| t == name && *h != "function" && *h != "comment"),
            "{name} not distinctly coloured in {found:?}"
        );
    }
    has(&found, ".PHONY", "keyword");
}

#[test]
fn variables_and_references_are_coloured() {
    let found = spans("make", FIXTURE);
    has(&found, "BINARY", "variable");
    has(&found, "$(", "punctuation.special");
    has(&found, ")", "punctuation.special");
    for a in ["$@", "$<", "$^"] {
        assert!(
            found
                .iter()
                .any(|(t, h)| a.contains(t.as_str()) && *h == "variable.builtin"),
            "{a} in {found:?}"
        );
    }
    has(&found, "shell", "function");
    has(&found, "wildcard", "function");
    has(&found, ":=", "operator");
    has(&found, "## build it", "comment");
}

#[test]
fn directives_and_recipe_prefixes_are_coloured() {
    let found = spans("make", FIXTURE);
    for k in [
        "include", "ifeq", "else", "endif", "define", "endef", "export",
    ] {
        has(&found, k, "keyword");
    }
    for o in ["@", "-", "+"] {
        has(&found, o, "operator");
    }
}

#[test]
fn recipe_bodies_highlight_as_bash() {
    let found = spans("make", FIXTURE);
    assert!(
        found
            .iter()
            .any(|(t, h)| t.contains("Go API on :8080") && *h == "string"),
        "{found:?}"
    );
    has(&found, "||", "operator");
    has(&found, "&", "operator");
    // The continuation line is still bash.
    has(&found, "&&", "operator");
}

#[test]
fn double_dollar_variable_is_bash() {
    let text = "dev:\n\t@FOO=$${ADMIN_PASSWORD:-dev} echo $(BINARY)\n";
    let found = spans_at("make", text);
    // The highlights covering any byte of the nth occurrence of `needle`.
    let at = |needle: &str, nth: usize| -> Vec<&str> {
        let start = text.match_indices(needle).nth(nth).unwrap().0;
        let range = start..start + needle.len();
        found
            .iter()
            .filter(|(r, _)| r.start < range.end && range.start < r.end)
            .map(|(_, h)| *h)
            .collect()
    };
    // The name inside the braces is a variable colour, not plain.
    assert_eq!(at("ADMIN_PASSWORD", 0), ["variable"], "{found:?}");
    // The `$$` is an escape, not a make reference.
    assert_eq!(at("$$", 0), ["escape"], "{found:?}");
    // The make reference on the same line keeps its make colours.
    assert_eq!(at("$(", 0), ["punctuation.special"], "{found:?}");
    assert_eq!(at("BINARY", 0), ["variable"], "{found:?}");
    assert_eq!(at(")", 0), ["punctuation.special"], "{found:?}");
}
