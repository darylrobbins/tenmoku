//! Keys written in code (object, struct, table and designated-initializer
//! keys) are `string.special.key`, like data-file keys; member access, field
//! declarations, shorthand and quoted keys keep their highlights.

#![cfg(all(
    feature = "javascript",
    feature = "typescript",
    feature = "rust",
    feature = "go",
    feature = "c",
    feature = "lua",
    feature = "python"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

const KEY: &str = "string.special.key";

/// `(start, text, highlight)` for every span of `text` in the language `name`.
fn spans(name: &str, text: &str) -> Vec<(usize, String, &'static str)> {
    let mut syntax = Syntax::new(language_by_name(name).unwrap(), Buffer::new(text).current());
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| {
            let start = span.range.start;
            (start, text[span.range].to_string(), span.highlight.name())
        })
        .collect()
}

/// The highlight of the `nth` (from 0) occurrence of `word` in `text`, which
/// must be a whole span. `None` when the text there has no span of its own.
fn at(
    found: &[(usize, String, &'static str)],
    text: &str,
    word: &str,
    nth: usize,
) -> Option<&'static str> {
    let start = text
        .match_indices(word)
        .map(|(ix, _)| ix)
        .nth(nth)
        .unwrap_or_else(|| panic!("no {word} #{nth}"));
    found
        .iter()
        .find(|(s, t, _)| *s == start && t == word)
        .map(|(_, _, h)| *h)
}

/// Asserts that the `nth` `word` in `text` of `lang` is `highlight`.
#[track_caller]
fn assert_is(lang: &str, text: &str, word: &str, nth: usize, highlight: &str) {
    let found = spans(lang, text);
    assert_eq!(
        at(&found, text, word, nth),
        Some(highlight),
        "{lang} {word} #{nth}: {found:?}"
    );
}

/// Asserts that the `nth` `word` in `text` of `lang` isn't a key.
#[track_caller]
fn assert_not_key(lang: &str, text: &str, word: &str, nth: usize) {
    let found = spans(lang, text);
    let start = text.match_indices(word).nth(nth).unwrap().0;
    let end = start + word.len();
    assert!(
        !found
            .iter()
            .any(|(s, t, h)| *h == KEY && *s < end && s + t.len() > start),
        "{lang} {word} #{nth}: {found:?}"
    );
}

#[test]
fn users_tsx_snippet_keys_are_keys() {
    let text = r#"function fromBook(b: Book): FormState {
  return {
    isbn: b.isbn13 || b.isbn10,
    title: b.title,
    authors: b.authors.join(", "),
    publishedYear: b.publishedYear?.toString() ?? "",
  };
}
"#;
    let found = spans("tsx", text);
    let is = |word: &str, nth: usize, highlight: &str| {
        assert_eq!(
            at(&found, text, word, nth),
            Some(highlight),
            "{word} #{nth}: {found:?}"
        );
    };
    // `isbn` #0 is the key; #1 and #2 start isbn13 and isbn10.
    is("isbn", 0, KEY);
    is("isbn13", 0, "property");
    is("isbn10", 0, "property");
    is("title", 0, KEY);
    is("title", 1, "property");
    is("authors", 0, KEY);
    is("authors", 1, "property");
    is("publishedYear", 0, KEY);
    is("publishedYear", 1, "property");
    is("join", 0, "function.method");
    is("toString", 0, "function.method");
}

#[test]
fn javascript_and_typescript_keys() {
    for lang in ["javascript", "typescript", "tsx"] {
        let text = "const o = { title: 1, isbn, \"quoted\": 2, 1: 3, [k]: 4 };\n\
                    const { author: a } = b;\n\
                    const m = { f: () => 1, g() {} };\n\
                    b.used;\n";
        assert_is(lang, text, "title", 0, KEY);
        assert_is(lang, text, "author", 0, KEY);
        assert_not_key(lang, text, "isbn", 0);
        assert_is(lang, text, "\"quoted\"", 0, "string");
        assert_not_key(lang, text, "1", 1);
        assert_not_key(lang, text, "k", 0);
        assert_is(lang, text, "f", 0, "function.method");
        assert_is(lang, text, "g", 0, "function.method");
        assert_is(lang, text, "used", 0, "property");
    }
    for lang in ["typescript", "tsx"] {
        let text = "interface Book { title: string }\n\
                    type T = { isbn: number };\n\
                    class A { author = 1 }\n";
        assert_is(lang, text, "title", 0, "property");
        assert_is(lang, text, "isbn", 0, "property");
        assert_is(lang, text, "author", 0, "property");
    }
    let text = "class A { author = 1 }\n";
    assert_is("javascript", text, "author", 0, "property");
    for lang in ["javascript", "tsx"] {
        let text = "const e = <div className=\"x\" />;\n";
        assert_is(lang, text, "className", 0, "attribute");
    }
}

#[test]
fn rust_struct_keys() {
    let text = "fn f(b: Book) {\n\
                    let n = Book { title: b.title, isbn };\n\
                    let Book { title: t, .. } = b;\n\
                }\n\
                struct Book { title: String }\n";
    assert_is("rust", text, "title", 0, KEY);
    assert_is("rust", text, "title", 1, "property");
    assert_is("rust", text, "title", 2, KEY);
    assert_is("rust", text, "title", 3, "property");
    assert_not_key("rust", text, "isbn", 0);
}

#[test]
fn go_composite_literal_keys() {
    let text = "package p\n\
                type Book struct { Title string }\n\
                var a = Book{Title: b.Title}\n\
                var s = []Book{{Title: \"x\"}}\n\
                var m = map[string]int{mk: 1}\n\
                var r = [...]string{A: \"a\"}\n";
    assert_is("go", text, "Title", 0, "property");
    assert_is("go", text, "Title", 1, KEY);
    assert_is("go", text, "Title", 2, "property");
    assert_is("go", text, "Title", 3, KEY);
    assert_not_key("go", text, "mk", 0);
    assert_is("go", text, "mk", 0, "variable");
    assert_not_key("go", text, "A", 0);
    assert_is("go", text, "A", 0, "variable");
}

#[test]
fn c_designated_initializer_keys() {
    let text = "struct p { int x; int y; };\n\
                void f(struct p b, struct p *c) {\n\
                    struct p q = { .x = 1, .y = b.y };\n\
                    int z = c->y;\n\
                }\n";
    assert_is("c", text, "x", 0, "property");
    assert_is("c", text, "y", 0, "property");
    assert_is("c", text, "x", 1, KEY);
    assert_is("c", text, "y", 1, KEY);
    assert_is("c", text, "y", 2, "property");
    assert_is("c", text, "y", 3, "property");
}

#[test]
fn lua_table_keys() {
    let text = "local t = { title = b.title, f = function() end, [\"k\"] = 1, [idx] = 2 }\n";
    assert_is("lua", text, "title", 0, KEY);
    assert_is("lua", text, "title", 1, "property");
    assert_is("lua", text, "f", 0, "function");
    assert_is("lua", text, "\"k\"", 0, "string");
    // A bracketed name is an expression: it keeps the crate's `field`.
    assert_is("lua", text, "idx", 0, "property");
}

#[test]
fn python_has_no_identifier_keys() {
    let text = "d = {\"a\": 1, k: 2}\nf(a=1)\nb.title\n";
    let found = spans("python", text);
    assert!(!found.iter().any(|(_, _, h)| *h == KEY), "{found:?}");
    assert_is("python", text, "title", 0, "property");
}

#[test]
fn is_key_needs_exactly_a_keys_range() {
    let text = "fn f(b: Book) -> Book { Book { title: b.title } }\n";
    let mut syntax = Syntax::new(
        language_by_name("rust").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    let key = text.find("title").unwrap();
    let access = text.rfind("title").unwrap();
    assert!(syntax.is_key(key..key + 5));
    for range in [
        key..key + 3,       // prefix
        key + 3..key + 5,   // suffix
        key + 1..key + 4,   // interior
        key..key + 6,       // oversized, into the colon
        key - 1..key + 5,   // oversized, from the space
        key..key,           // empty
        access..access + 5, // member access
    ] {
        assert!(
            !syntax.is_key(range.clone()),
            "{range:?}: {:?}",
            &text[range.clone()]
        );
    }
}

#[test]
fn is_key_is_false_before_a_parse() {
    let text = "fn f() -> Book { Book { title: 1 } }\n";
    let syntax = Syntax::new(
        language_by_name("rust").unwrap(),
        Buffer::new(text).current(),
    );
    let key = text.find("title").unwrap();
    assert!(!syntax.is_key(key..key + 5));
}
