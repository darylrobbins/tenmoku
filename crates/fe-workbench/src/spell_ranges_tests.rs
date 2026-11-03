//! Which text of a buffer is spell checked: comments and strings in code, prose
//! in plain text and Markdown, found from the parse the buffer already has.

use std::{
    ops::Range,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_config::settings::SpellScope;
use fe_syntax::{Highlight, HighlightSpan, Syntax, language_by_name, loaded_language};

use crate::{
    spell::tokenize,
    spell_ranges::{MAX_SPELLABLE_BYTES, raw_html, spellable_ranges, words_in_spans},
};

const ALL: [SpellScope; 3] = [SpellScope::Comments, SpellScope::Strings, SpellScope::Text];

/// Compiles the languages that drawing a buffer would have compiled by now.
fn compile_injected() {
    for name in ["markdown_inline", "html", "rustdoc_inline"] {
        language_by_name(name).unwrap();
    }
}

/// `buffer` parsed in full as `language`, with what it injects compiled.
fn parsed(language: &str, buffer: &Buffer) -> Syntax {
    compile_injected();
    let language = language_by_name(language).unwrap();
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax
}

/// What [`spellable_ranges`] finds once highlighting has had what it needs:
/// other languages compiled, and the time a machine under load may not give it.
/// That shows as no ranges, and a time-out needn't ask for another try.
fn found(syntax: &Syntax, buffer: &Buffer, scopes: &[SpellScope], rows: Range<u32>) -> Vec<String> {
    let text = buffer.text();
    let mut tries = 0;
    loop {
        tries += 1;
        let ranges = spellable_ranges(Some(syntax), text, scopes, rows.clone());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() && (!ranges.is_empty() || tries >= 50) {
            return ranges
                .into_iter()
                .map(|range| text.slice_to_string(range))
                .collect();
        }
        for name in missing {
            language_by_name(name).unwrap();
        }
    }
}

/// [`found`] for all of `source` in `language`.
fn found_all(language: &str, source: &str, scopes: &[SpellScope]) -> Vec<String> {
    let buffer = Buffer::new(source);
    let syntax = parsed(language, &buffer);
    found(&syntax, &buffer, scopes, 0..u32::MAX)
}

/// The one range of something that should have just that.
fn one(range: Range<usize>) -> Vec<Range<usize>> {
    std::iter::once(range).collect()
}

/// The words a speller would get asked about in `found`.
fn words(found: &[String]) -> Vec<String> {
    found
        .iter()
        .flat_map(|text| tokenize(text).into_iter().map(|word| word.text.to_owned()))
        .collect()
}

const RUST: &str = "\
// line remark
/// doc remark
//! inner remark
/* block remark
 * second remark */
fn main() {
    let identifier = \"string words\";
}
";

#[test]
fn rust_comments_and_strings_only() {
    let comments = [
        "line remark",
        "doc remark",
        "inner remark",
        "block remark",
        "second remark",
    ];
    assert_eq!(found_all("rust", RUST, &ALL), {
        let mut all = comments.to_vec();
        all.push("string words");
        all
    });
    // Each scope goes on its own.
    assert_eq!(
        found_all("rust", RUST, &[SpellScope::Comments]),
        comments.to_vec()
    );
    assert_eq!(
        found_all("rust", RUST, &[SpellScope::Strings]),
        ["string words"]
    );
    assert!(found_all("rust", RUST, &[SpellScope::Text]).is_empty());
    assert!(found_all("rust", RUST, &[]).is_empty());
}

#[test]
fn rust_doc_comment_code_and_links_are_left_out() {
    let source = "/// call `frobnicate` and see [Widget] today\nfn a() {}\n";
    let found = found_all("rust", source, &ALL);
    assert_eq!(found, ["call", "and see", "today"]);
}

#[test]
fn quotes_and_their_prefixes_are_left_out() {
    let rust = "fn a() { let b = (r#\"raw words\"#, b\"byte words\", r###\"hash words\"###); }\n";
    assert_eq!(
        found_all("rust", rust, &[SpellScope::Strings]),
        ["raw words", "byte words", "hash words"]
    );
    let python = "a = f'format words'\nb = '''long words'''\nc = \"I'm here\"\n";
    assert_eq!(
        found_all("python", python, &[SpellScope::Strings]),
        ["format words", "long words", "I'm here"]
    );
    let go = "package a\nvar b = `raw words`\n";
    assert_eq!(found_all("go", go, &[SpellScope::Strings]), ["raw words"]);
}

/// Strings from the tree of each kind of language: what's a delimiter, an
/// escape or an interpolation is that language's to say.
#[test]
fn string_literals_keep_only_their_text() {
    let strings =
        |language: &str, source: &str| found_all(language, source, &[SpellScope::Strings]);
    let words = ["hello words"];
    for (language, source) in [
        ("c", "char *a = \"hello words\";\n"),
        ("c", "wchar_t *a = L\"hello words\";\n"),
        ("cpp", "auto a = R\"(hello words)\";\n"),
        ("cpp", "auto a = R\"tag(hello words)tag\";\n"),
        ("cpp", "auto a = u8R\"tag(hello words)tag\";\n"),
        ("lua", "a = \"hello words\"\n"),
        ("lua", "a = 'hello words'\n"),
        ("lua", "a = [[hello words]]\n"),
        ("lua", "a = [==[hello words]==]\n"),
        ("rust", "fn a() { let b = \"hello words\"; }\n"),
        ("rust", "fn a() { let b = r##\"hello words\"##; }\n"),
        ("python", "a = \"hello words\"\n"),
        ("python", "a = rb'hello words'\n"),
        ("python", "a = \"\"\"hello words\"\"\"\n"),
        ("go", "package a\nvar b = \"hello words\"\n"),
        ("go", "package a\nvar b = `hello words`\n"),
        ("javascript", "const a = 'hello words';\n"),
        ("javascript", "const a = `hello words`;\n"),
        ("typescript", "const a: string = \"hello words\";\n"),
        ("java", "class A { String b = \"hello words\"; }\n"),
        ("csharp", "class A { string b = @\"hello words\"; }\n"),
        ("kotlin", "val a = \"hello words\"\n"),
        ("swift", "let a = \"hello words\"\n"),
        ("ruby", "a = \"hello words\"\n"),
        ("php", "<?php $a = \"hello words\";\n"),
        ("bash", "a=\"hello words\"\n"),
        ("toml", "a = \"hello words\"\n"),
        ("yaml", "a: \"hello words\"\n"),
        ("yaml", "a: hello words\n"),
    ] {
        assert_eq!(strings(language, source), words, "{language}: {source}");
    }
}

/// A string in an interpolation is code, not the text of either string.
#[test]
fn strings_in_interpolations_have_no_text() {
    let strings =
        |language: &str, source: &str| found_all(language, source, &[SpellScope::Strings]);
    let words = ["hello", "today"];
    for (language, source) in [
        (
            "javascript",
            "const s = `hello ${lookup(\"world\")} today`;\n",
        ),
        ("javascript", "const s = `hello ${`world`} today`;\n"),
        ("python", "a = f\"hello {lookup('world')} today\"\n"),
        ("ruby", "a = \"hello #{lookup(\"world\")} today\"\n"),
        ("kotlin", "val a = \"hello ${lookup(\"world\")} today\"\n"),
    ] {
        assert_eq!(strings(language, source), words, "{language}: {source}");
    }
    let multiline = "const s = `hello ${lookup(\"world\")}\ntoday ${`more`} done`;\n";
    assert_eq!(strings("javascript", multiline), ["hello", "today", "done"]);
    // By itself it's found no matter which row of it is asked for.
    let buffer = Buffer::new(multiline);
    let syntax = parsed("javascript", &buffer);
    let at_row = found(&syntax, &buffer, &[SpellScope::Strings], 1..2);
    assert_eq!(at_row, ["hello", "today", "done"]);
}

/// How much of a call's time a scan of `spans` may take. A linear scan of these
/// takes well under a second, and a quadratic one takes minutes.
const SCALING_LIMIT: Duration = Duration::from_secs(20);

/// `spans`, as many as `source` should have for `language`, read for their
/// words: without highlighting, whose time budget would just leave it incomplete.
fn words_scan(language: &str, source: &str, spans: Vec<Range<usize>>) -> usize {
    let buffer = Buffer::new(source);
    let syntax = parsed(language, &buffer);
    let spans: Vec<_> = spans
        .into_iter()
        .map(|range| HighlightSpan {
            range,
            highlight: Highlight::from_capture("string").unwrap(),
        })
        .collect();
    let started = Instant::now();
    let found = words_in_spans(
        &syntax,
        buffer.text(),
        &ALL,
        &(0..source.len()),
        &[],
        &spans,
    );
    assert!(started.elapsed() < SCALING_LIMIT, "{:?}", started.elapsed());
    found.len()
}

#[test]
fn unmatched_braces_scale() {
    let open = "fn a() { let b = r\"";
    let source = format!("{open}word {}\"; }}\n", "{".repeat(1_500_000));
    let span = open.len() - 2..source.len() - 4;
    assert_eq!(words_scan("rust", &source, vec![span]), 1);
}

#[test]
fn many_escapes_scale() {
    let open = "fn a() { let b = \"";
    let source = format!("{open}{}\"; }}\n", "word\\n".repeat(150_000));
    // A span for each stretch between its escapes, the last with its quote.
    let mut spans = Vec::new();
    let mut at = open.len() - 1;
    for _ in 0..150_000 {
        spans.push(at..at + 5);
        at += 6;
    }
    assert_eq!(words_scan("rust", &source, spans), 150_000);
}

#[test]
fn many_strings_scale() {
    let source = format!("[{}\"word\"]\n", "\"word\",".repeat(150_000));
    let spans = (0..150_000).map(|ix| 1 + 7 * ix..1 + 7 * ix + 6).collect();
    assert_eq!(words_scan("json", &source, spans), 150_000);
}

#[test]
fn comment_marks_of_other_languages_are_left_out() {
    assert_eq!(
        found_all("python", "# hash words\n", &[SpellScope::Comments]),
        ["hash words"]
    );
    assert_eq!(
        found_all("html", "<!-- html words -->\n", &[SpellScope::Comments]),
        ["html words"]
    );
    assert_eq!(
        found_all("lua", "-- dash words\n", &[SpellScope::Comments]),
        ["dash words"]
    );
    // Marks only count up against the comment's end: this is a path.
    assert_eq!(
        found_all("rust", "// /tmp is here\n", &[SpellScope::Comments]),
        ["/tmp is here"]
    );
}

#[test]
fn string_escapes_and_format_holes_are_excluded() {
    let source = "fn a() { let b = \"hello\\nworld {name}\"; }\n";
    let found = found_all("rust", source, &[SpellScope::Strings]);
    assert_eq!(found, ["hello", "world"]);
    assert_eq!(words(&found), ["hello", "world"]);

    // Where the grammar finds the holes itself.
    let python = "a = f\"hello {name} world\\n\"\n";
    assert_eq!(
        found_all("python", python, &[SpellScope::Strings]),
        ["hello", "world"]
    );
    let javascript = "const a = `hello ${name} world`;\n";
    assert_eq!(
        found_all("javascript", javascript, &[SpellScope::Strings]),
        ["hello", "world"]
    );

    // A grammar that leaves escapes in the string, of any length.
    let c = "char *a = \"hello\\nworld {name}\";\n";
    assert_eq!(
        found_all("c", c, &[SpellScope::Strings]),
        ["hello", "world"]
    );
    let c = "char *a = \"one\\uabcdtwo\\x41three\\101four\\U0001F600five\";\n";
    assert_eq!(
        found_all("c", c, &[SpellScope::Strings]),
        ["one", "two", "three", "four", "five"]
    );
    // However many digits the language gives its escape, they're not words.
    let c = "wchar_t *a = L\"one\\x00beeword\";\n";
    assert_eq!(found_all("c", c, &[SpellScope::Strings]), ["one", "word"],);
    let cpp = "auto a = \"one\\x00beeword\";\n";
    assert_eq!(
        found_all("cpp", cpp, &[SpellScope::Strings]),
        ["one", "word"]
    );
    let rust = "fn a() { let b = \"one\\x41bad\"; }\n";
    assert_eq!(
        found_all("rust", rust, &[SpellScope::Strings]),
        ["one", "bad"]
    );
    // A template has escapes, for all its backticks, and a Go raw string none.
    let javascript = "const a = `hello\\nworld`;\n";
    assert_eq!(
        found_all("javascript", javascript, &[SpellScope::Strings]),
        ["hello", "world"]
    );
    let go = "package a\nvar b = `hello\\nworld`\n";
    assert_eq!(
        found_all("go", go, &[SpellScope::Strings]),
        ["hello\\nworld"]
    );
    // A raw string has no escapes.
    let python = "a = r\"one\\ntwo\"\n";
    assert_eq!(
        found_all("python", python, &[SpellScope::Strings]),
        ["one\\ntwo"]
    );

    // A word after an escape isn't a string's prefix.
    let short = "fn a() { let b = \"hello\\nteh\"; }\n";
    assert_eq!(
        found_all("rust", short, &[SpellScope::Strings]),
        ["hello", "teh"]
    );

    // A lone brace is no hole.
    let brace = "fn a() { let b = \"open { here\"; }\n";
    assert_eq!(
        found_all("rust", brace, &[SpellScope::Strings]),
        ["open { here"]
    );
}

#[test]
fn what_is_in_a_string_but_no_words_is_excluded() {
    assert_eq!(
        found_all("json", "{\"key\": \"value\"}", &ALL),
        ["value"],
        "an object's keys are names"
    );
    assert_eq!(
        found_all("javascript", "const a = /regex/;\n", &ALL),
        Vec::<String>::new()
    );
    assert_eq!(
        found_all("ruby", "a = :symbol\n", &ALL),
        Vec::<String>::new()
    );
}

const MARKDOWN: &str = "\
---
title: frontmatterword
---

# Heading words

A paragraph with *emphasised* and **strong** text, `inlinecode`, a
[link text](https://dest.example/destword \"Title\") and <https://auto.example/autoword>
then <span class=\"attrword\">inside words</span> &nbsp; [full][labelword] and a![alt words](imageword.png)
ending https://bare.example/bareword here.

- list item
- [x] task item

> quoted line
> second quoted

```rust
// fencecomment
let fencecode = 1;
```

    indentedcode

<div class=\"blockattr\">
blockhtml
</div>

[labelword]: https://definition.example/defword \"deftitle\"

| headcell |
|---|
| bodycell `cellcode` |

Setext words
============
";

#[test]
fn markdown_prose_only() {
    let found = found_all("markdown", MARKDOWN, &[SpellScope::Text]);
    let joined = found.join("\n");
    for prose in [
        "Heading words",
        "A paragraph with",
        "emphasised",
        "strong",
        "link text",
        "inside words",
        "full",
        "alt words",
        "list item",
        "task item",
        "quoted line",
        "second quoted",
        "Setext words",
    ] {
        assert!(joined.contains(prose), "{prose} isn't in {found:#?}");
    }
    for code in [
        "frontmatterword",
        "title",
        "inlinecode",
        "destword",
        "Title",
        "autoword",
        "span",
        "class",
        "attrword",
        "nbsp",
        "labelword",
        "imageword",
        "fencecomment",
        "fencecode",
        "indentedcode",
        "blockattr",
        "blockhtml",
        "div",
        "defword",
        "deftitle",
    ] {
        assert!(!joined.contains(code), "{code} is in {found:#?}");
    }
    // Markers go too, even a quote's on its second line.
    assert!(!joined.contains('#'), "{found:#?}");
    assert!(!joined.contains("> "), "{found:#?}");
    assert!(!joined.contains('*'), "{found:#?}");
    assert!(!joined.contains('`'), "{found:#?}");
    assert!(found.iter().all(|text| text.trim() == text), "{found:#?}");

    // A bare URL reads as prose, and the tokenizer takes it out as a URL.
    assert!(joined.contains("https://bare.example"), "{found:#?}");
    assert!(
        !words(&found).contains(&"bareword".to_owned()),
        "{found:#?}"
    );

    // It's the text scope, and none of the others.
    assert!(
        found_all(
            "markdown",
            MARKDOWN,
            &[SpellScope::Comments, SpellScope::Strings]
        )
        .is_empty()
    );
}

#[test]
fn markdown_toml_front_matter_and_html_comments_are_excluded() {
    let source = "+++\ntitle = \"frontword\"\n+++\n\nBody words\n\n<!-- blockremark -->\n\nAfter <!-- inlineremark --> words\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["Body words", "After", "words"]
    );
}

#[test]
fn markdown_raw_html_is_excluded_whole() {
    let source = "One <?target piword?> two <![CDATA[cdataword]]> three <span hidden>four</span> \
                  five <!-- commentword --> six <!DOCTYPE doctypeword> seven <br/> eight\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        [
            "One", "two", "three", "four", "five", "six", "seven", "eight"
        ]
    );
    // What isn't a tag stays prose, and so does what's escaped or never closed.
    let source = "If a < b then more\n\nAn \\<escaped> word\n\nOpen <unclosed word\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        [
            "If a < b then more",
            "An",
            "escaped> word",
            "Open <unclosed word"
        ]
    );
}

#[test]
fn markdown_raw_html_edges() {
    // A `>` in a quoted attribute doesn't end a tag the grammar misses.
    let source = "One <span hidden title=\"a > attrword\">two</span> three\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["One", "two", "three"]
    );
    // Two backslashes are an escaped backslash, and then a tag.
    let source = "One \\\\<span hidden>two</span> three\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["One", "two", "three"]
    );
    // A `<` in a code span starts nothing.
    let source = "Use `<tag` here > today\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["Use", "here > today"]
    );
    // A quote that never closes doesn't hide the tag's `>`.
    let source = "One <a b=\"c> two\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["One", "two"]
    );
    // Nor does it stop a later tag's quoted `>` being read as quoted.
    let source = "One <a b='c> two <span hidden title=\"a > attrword\">three</span>\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["One", "two", "three"]
    );
    // With no `>` after it, it ends at the next `<`, or else its line, and the
    // paragraph's next line is prose.
    let source = "One <a b='badword\nnext words\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["One", "next words"]
    );
    assert_eq!(raw_html("<a b='cd ef <g h", &[]), one(0..12));
    assert_eq!(raw_html("<a b='cd ef\ngh>", &[]), one(0..11));
    // A `<` after closed quotes, or with none, ends the look for a `>`.
    assert_eq!(raw_html("<a b=\"c\" <d>", &[]), [0..9, 9..12]);
    assert_eq!(raw_html("<a b <d>", &[]), one(5..8));
    // A tag can take several lines, and quotes that close but no `>` only one.
    assert_eq!(raw_html("<a\nb=\"c\">", &[]), one(0..9));
    assert_eq!(raw_html("<a b=\"c\" d\ne f", &[]), one(0..10));
}

/// The scan alone, as highlighting this many tags would run out of time and
/// leave no ranges to look at.
#[test]
fn raw_html_scales() {
    let unclosed_quotes = format!("One <a b=\"c> two {}", "<a b=\"c ".repeat(200_000));
    let unclosed_tags = format!("One {}", "<a ".repeat(500_000));
    let backslashes = format!("{}<a>", "\\".repeat(1_000_000));
    // Each tag's `>` is quoted, and a `<` follows its closing quote.
    let failing_quotes = "<a b=\">x\"".repeat(200_000);
    let started = Instant::now();
    assert_eq!(raw_html(&failing_quotes, &[]).len(), 200_000);
    assert_eq!(
        raw_html(&unclosed_quotes, &[]).first().map(|cut| cut.start),
        Some(4)
    );
    assert!(raw_html(&unclosed_tags, &[]).is_empty());
    assert_eq!(raw_html(&backslashes, &[]).len(), 1);
    assert!(started.elapsed() < SCALING_LIMIT, "{:?}", started.elapsed());
}

#[test]
fn markdown_past_the_injection_limit_has_no_ranges() {
    // Only as much as is asked for has to be highlighted, so a screenful has
    // its prose, but not all of them at once, where code spans would read as it.
    let source = "Some `code` here\n\n".repeat(5000);
    let buffer = Buffer::new(&source);
    let syntax = parsed("markdown", &buffer);
    assert!(found(&syntax, &buffer, &ALL, 0..u32::MAX).is_empty());
    assert_eq!(found(&syntax, &buffer, &ALL, 0..1), ["Some", "here"]);
}

#[test]
fn markdown_email_autolinks_are_excluded() {
    let source = "Mail <someone@example.com> or <{misspeled}@example.com> today\n";
    assert_eq!(
        found_all("markdown", source, &[SpellScope::Text]),
        ["Mail", "or", "today"]
    );
}

#[test]
fn markdown_asks_for_its_grammars_when_they_are_not_compiled() {
    // A test run by nextest has its own process, so nothing has compiled this.
    if loaded_language("markdown_inline").is_some() {
        return;
    }
    let buffer = Buffer::new("Some words\n");
    let language = language_by_name("markdown").unwrap();
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..9).is_empty());
    assert!(syntax.take_missing_languages().contains(&"markdown_inline"));
    language_by_name("markdown_inline").unwrap();
    assert_eq!(found(&syntax, &buffer, &ALL, 0..9), ["Some words"]);
}

#[test]
fn an_incomplete_result_is_no_ranges() {
    // A test run by nextest has its own process, so nothing has compiled this.
    // Its doc comment would be read with its code span in it, were it not for
    // the language it lacks.
    if loaded_language("rustdoc_inline").is_some() {
        return;
    }
    let buffer = Buffer::new("/// call `frobnicate` today\nfn a() {}\n// plain remark\n");
    let language = language_by_name("rust").unwrap();
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(!syntax.checked_highlights(0..buffer.text().len()).complete);
    assert!(spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..9).is_empty());
    assert!(
        syntax.take_missing_languages().contains(&"rustdoc_inline"),
        "nothing asked for it"
    );
    language_by_name("rustdoc_inline").unwrap();
    assert_eq!(
        found(&syntax, &buffer, &ALL, 0..9),
        ["call", "today", "plain remark"]
    );
}

#[test]
fn markdown_tables_are_left_out() {
    // A cell's content has no inline highlights to tell code from words.
    let source = "| headword |\n|---|\n| `cellcode` bodyword |\n";
    assert!(found_all("markdown", source, &[SpellScope::Text]).is_empty());
}

#[test]
fn plain_text_is_all_spellable() {
    let buffer = Buffer::new("Some plain words.\n\nA seccond paragraph.\n");
    let text = buffer.text();
    assert_eq!(
        spellable_ranges(None, text, &ALL, 0..u32::MAX),
        one(0..text.len())
    );
    assert_eq!(
        spellable_ranges(None, text, &[SpellScope::Text], 0..text.row_count()),
        one(0..text.len())
    );
    // Comments and strings say nothing about plain text.
    assert!(
        spellable_ranges(
            None,
            text,
            &[SpellScope::Comments, SpellScope::Strings],
            0..9
        )
        .is_empty()
    );
    assert!(spellable_ranges(None, Buffer::new("").text(), &ALL, 0..9).is_empty());

    // A diff is highlighted, and has no comments or strings.
    assert!(found_all("diff", "--- a words\n+++ b words\n+added words\n", &ALL).is_empty());
}

#[test]
fn injected_languages_follow_their_layer() {
    // v1 skips a fence whole, comment and all, and whatever its language.
    let fence =
        "Before words\n\n```rust\n// fencecomment\nlet a = \"fencestring\";\n```\n\nAfter words\n";
    assert_eq!(
        found_all("markdown", fence, &ALL),
        ["Before words", "After words"]
    );
    let unknown = "```nosuchlanguage\n// fencecomment\n```\n";
    assert!(found_all("markdown", unknown, &ALL).is_empty());

    // In code, an injected language's comments and strings count like any.
    let html = "<p>pagetext</p>\n<script>\n// script remark\nlet a = 'script string';\n</script>\n";
    language_by_name("javascript").unwrap();
    assert_eq!(
        found_all("html", html, &ALL),
        ["script remark", "script string"]
    );
}

#[test]
fn viewport_window_and_large_files() {
    let source = "\
// first remark
fn a() {}
/* block one
   block two
   block three */
fn b() {}
const C: &str = \"long
string\";
// last remark
";
    let buffer = Buffer::new(source);
    let syntax = parsed("rust", &buffer);
    let at = |rows: Range<u32>| found(&syntax, &buffer, &ALL, rows);

    assert_eq!(at(0..1), ["first remark"]);
    // Nothing that doesn't touch the rows.
    assert!(at(1..2).is_empty());
    assert!(at(5..6).is_empty());
    assert!(at(50..60).is_empty());
    assert!(at(2..2).is_empty());
    // The middle of a block comment gets the rest of it.
    let block = ["block one", "block two", "block three"];
    assert_eq!(at(3..4), block);
    assert_eq!(at(4..5), block);
    assert_eq!(at(2..3), block);
    // So does the middle of a string, and it loses its quotes all the same.
    assert_eq!(at(7..8), ["long\nstring"]);
    assert_eq!(at(6..7), ["long\nstring"]);
    assert_eq!(at(8..9), ["last remark"]);
    assert_eq!(at(1..3), block);

    // A run of line comments isn't one token: each row gets its own.
    let run = Buffer::new("// one remark\n// two remark\n// three remark\n");
    let run_syntax = parsed("rust", &run);
    assert_eq!(found(&run_syntax, &run, &ALL, 1..2), ["two remark"]);

    // A string split by escapes or holes is still one token, at either edge.
    let split = "// before\nconst A: &str = \"one\\n\ntwo\\t\nthree\";\n// after\n";
    let buffer_split = Buffer::new(split);
    let syntax_split = parsed("rust", &buffer_split);
    let whole = ["one", "two", "three"];
    for row in 1..4 {
        let found = found(
            &syntax_split,
            &buffer_split,
            &[SpellScope::Strings],
            row..row + 1,
        );
        assert_eq!(found, whole, "row {row}");
    }
    let template = "const a = `one\n${b}\ntwo\n${c}\nthree`;\n";
    let buffer_template = Buffer::new(template);
    let syntax_template = parsed("javascript", &buffer_template);
    let whole = ["one", "two", "three"];
    for row in 0..5 {
        let found = found(
            &syntax_template,
            &buffer_template,
            &[SpellScope::Strings],
            row..row + 1,
        );
        assert_eq!(found, whole, "row {row}");
    }

    // Only what's in a scope goes whole: with just comments on, a string on the
    // row asked for doesn't bring in the comments in its other rows' holes.
    let holes = "const a = `one ${/* other remark */ b}\ntwo\nthree ${/* next remark */ c}`;\n";
    let buffer_holes = Buffer::new(holes);
    let syntax_holes = parsed("javascript", &buffer_holes);
    let comments = [SpellScope::Comments];
    assert!(found(&syntax_holes, &buffer_holes, &comments, 1..2).is_empty());
    assert_eq!(
        found(&syntax_holes, &buffer_holes, &comments, 0..1),
        ["other remark"]
    );
    // With both on, the string goes whole, and those comments still don't touch.
    assert_eq!(
        found(&syntax_holes, &buffer_holes, &ALL, 1..2),
        ["one", "two\nthree"]
    );

    // A token far longer than the window, reached from its middle.
    let long = format!("/* {}*/\nfn a() {{}}\n", "lengthy remark\n".repeat(400));
    let buffer = Buffer::new(&long);
    let syntax = parsed("rust", &buffer);
    let found_long = found(&syntax, &buffer, &[SpellScope::Comments], 200..201);
    assert_eq!(found_long.len(), 400);
    assert!(found(&syntax, &buffer, &[SpellScope::Comments], 401..402).is_empty());

    // Markdown's whole tokens are its paragraphs.
    let markdown = "First para\nline two\n\nSecond para\n> not a quote\n\nThird para\n";
    let buffer = Buffer::new(markdown);
    let syntax = parsed("markdown", &buffer);
    assert_eq!(
        found(&syntax, &buffer, &ALL, 1..2),
        ["First para\nline two"]
    );
    assert!(found(&syntax, &buffer, &ALL, 2..3).is_empty());

    // Plain text has no tokens: just the rows, which never cut a word.
    let buffer = Buffer::new("one\ntwo\nthree\n");
    assert_eq!(spellable_ranges(None, buffer.text(), &ALL, 1..2), one(4..8));
    assert_eq!(
        spellable_ranges(None, buffer.text(), &ALL, 2..9),
        one(8..14)
    );
}

#[test]
fn large_buffers_have_no_ranges() {
    // At the limit, and over it.
    let plain = "a".repeat(MAX_SPELLABLE_BYTES);
    let buffer = Buffer::new(&plain);
    assert_eq!(
        spellable_ranges(None, buffer.text(), &ALL, 0..1),
        one(0..MAX_SPELLABLE_BYTES)
    );
    let over = "a".repeat(MAX_SPELLABLE_BYTES + 1);
    assert!(spellable_ranges(None, Buffer::new(&over).text(), &ALL, 0..1).is_empty());

    // Parsed in full, but over the limit.
    let filler = "// remark\n".repeat(MAX_SPELLABLE_BYTES / 10 + 1);
    let buffer = Buffer::new(&filler);
    let rust = language_by_name("rust").unwrap();
    assert!(filler.len() < rust.parse_limit());
    let syntax = parsed("rust", &buffer);
    assert!(syntax.tree().is_some());
    assert!(spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..1).is_empty());

    // Large-file mode: only a window of it is parsed.
    let filler = "// remark\n".repeat(rust.parse_limit() / 10 + 1);
    let buffer = Buffer::new(&filler);
    let mut syntax = Syntax::new(rust, buffer.current());
    syntax.finish(syntax.window_job(0..10).unwrap().run());
    assert!(syntax.tree().is_none());
    assert!(syntax.window().is_some());
    assert!(!syntax.highlights(0..10).is_empty());
    assert!(spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..1).is_empty());
}

#[test]
fn a_string_edited_open_is_not_read_until_parsed() {
    // The old string's capture stretches over what's now an identifier.
    let source = "fn a() { let s = \"hello\"; }\n";
    let mut buffer = Buffer::new(source);
    let mut syntax = parsed("rust", &buffer);
    let at = source.rfind('"').unwrap();
    let typed = "\"; let identifier = \"";
    buffer.edit([(at..at, typed)], fe_buffer::Origin::User);
    syntax.edit(buffer.current());
    assert!(spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..9).is_empty());

    syntax.finish(syntax.parse_job().run());
    assert_eq!(found(&syntax, &buffer, &ALL, 0..9), ["hello"]);
}

#[test]
fn nothing_here_parses() {
    compile_injected();
    for (language, source) in [("rust", "// a remark\n"), ("markdown", "Some words\n")] {
        let mut buffer = Buffer::new(source);
        let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());

        // Not parsed yet: no ranges, and still no tree after asking.
        assert!(spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..9).is_empty());
        assert!(syntax.tree().is_none());
        assert!(syntax.needs_parse());

        // Edited since the last parse: nothing until it's parsed, and asking
        // doesn't do that.
        syntax.finish(syntax.parse_job().run());
        assert!(!syntax.needs_parse());
        let len = buffer.text().len();
        buffer.edit([(len..len, "\n")], fe_buffer::Origin::User);
        syntax.edit(buffer.current());
        assert!(syntax.needs_parse());
        let ranges = spellable_ranges(Some(&syntax), buffer.text(), &ALL, 0..9);
        assert!(syntax.needs_parse(), "{language} reparsed");
        assert!(ranges.is_empty(), "{language}: {ranges:?}");

        syntax.finish(syntax.parse_job().run());
        let ranges = found(&syntax, &buffer, &ALL, 0..9);
        assert_eq!(ranges.len(), 1, "{language}: {ranges:?}");
    }
}
