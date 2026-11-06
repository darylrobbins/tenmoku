use std::{
    cell::Cell,
    ops::Range,
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Origin};
use proptest::{prelude::prop, prop_assert_eq, proptest};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use super::{Collected, Layers};
use crate::{
    highlight::{clip, overlay},
    syntax::{DEADLINE_AFTER, DEADLINE_CHECKS, highlight_query},
    *,
};

fn parsed(name: &str, buffer: &Buffer) -> Syntax {
    let mut syntax = Syntax::new(language_by_name(name).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run_until(|_| || false));
    syntax
}

/// `(text, highlight)` for every span of `text` highlighted whole.
fn spans(name: &str, text: &str) -> Vec<(String, &'static str)> {
    let syntax = parsed(name, &Buffer::new(text));
    syntax
        .highlights_compiling(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

/// Asserts that `found` includes each of `expected`.
fn assert_includes(found: &[(String, &str)], expected: &[(&str, &str)]) {
    let missing: Vec<_> = expected
        .iter()
        .filter(|&&(text, highlight)| !found.contains(&(text.to_string(), highlight)))
        .collect();
    assert!(missing.is_empty(), "missing {missing:?} in {found:?}");
}

#[test]
fn markdown_inline_and_fenced_code_are_highlighted() {
    let text = "# A `b`\n\n\
                > Some `code` and [a link](https://x.dev).\n\
                > More *emphasis*.\n\n\
                ```rust\nfn main() {}\n```\n";
    let found = spans("markdown", text);
    assert_includes(
        &found,
        &[
            // Inline grammar, inside a heading and a block quote.
            ("A ", "markup.heading"),
            ("b", "markup.raw"),
            ("code", "markup.raw"),
            ("`", "punctuation.delimiter"),
            ("a link", "markup.link"),
            ("https://x.dev", "markup.link.url"),
            // The block grammar's `> ` between the quote's lines stays.
            ("> ", "punctuation.special"),
            // Rust in the fence, over the fence's own highlight.
            ("fn", "keyword"),
            ("main", "function"),
            ("rust\n", "markup.raw"),
            (" ", "markup.raw"),
        ],
    );
    // Emphasis has no highlight of ours, but its delimiters do.
    assert!(!found.iter().any(|(text, _)| text.contains("emphasis")));

    // Fence languages resolve by alias and extension, ignoring case.
    for (fence, keyword) in [
        ("JS", "const"),
        ("py", "def"),
        #[cfg(feature = "cpp")]
        ("c++", "class"),
        ("shell", "if"),
        ("rust,ignore", "fn"),
    ] {
        let source = match keyword {
            "const" => "const a = 1;",
            "def" => "def f(): pass",
            "class" => "class C {};",
            "if" => "if true; then :; fi",
            _ => "fn f() {}",
        };
        let text = format!("```{fence}\n{source}\n```\n");
        assert_includes(&spans("markdown", &text), &[(keyword, "keyword")]);
    }
}

#[test]
fn html_script_and_style_are_highlighted() {
    let text = "<style>p { color: red; }</style>\n\
                <script>const x = `<b>${1}</b>`;</script>\n";
    assert_includes(
        &spans("html", text),
        &[
            ("style", "tag"),
            ("color", "property"),
            ("script", "tag"),
            ("const", "keyword"),
            ("1", "number"),
        ],
    );
}

/// A tagged template's pieces are parsed as one layer, around its `${…}`s,
/// which stay JavaScript.
#[test]
fn tagged_templates_combine_their_pieces() {
    let text = "html`<p class=${c}>hi</p>`;\ncss`a { color: red; }`;\n";
    let found = spans("javascript", text);
    assert_includes(
        &found,
        &[
            ("p", "tag"),
            ("class", "attribute"),
            ("c", "variable"),
            ("${", "punctuation.special"),
            ("hi", "string"),
            ("color", "property"),
        ],
    );
    let syntax = parsed("javascript", &Buffer::new(text));
    syntax.highlights_compiling(0..text.len());
    let layers = syntax.layers();
    assert_eq!(layers[0], ("html", vec![5..14, 18..25]));
    assert_eq!(layers[1].0, "css");
}

/// PHP heredocs and nowdocs are parsed in the language their label names. A
/// heredoc's pieces form one layer around its interpolations, which stay PHP.
#[test]
#[cfg(all(feature = "php", feature = "sql"))]
fn php_heredocs_and_nowdocs_inject_their_text() {
    let text = "<?php\n\
                $q = <<<SQL\n  SELECT a FROM t WHERE id = {$id} AND b = 1\n  SQL;\n\
                $r = <<<'sql'\n  SELECT c\n  FROM u\n  sql;\n";
    let found = spans("php", text);
    assert_includes(
        &found,
        &[
            ("SELECT", "keyword"),
            ("FROM", "keyword"),
            ("WHERE", "keyword"),
            ("t", "type"),
            ("u", "type"),
            ("1", "number"),
            ("id", "variable"),
        ],
    );
    let selects = found
        .iter()
        .filter(|span| *span == &("SELECT".into(), "keyword"));
    assert_eq!(selects.count(), 2, "{found:?}");

    let syntax = parsed("php", &Buffer::new(text));
    syntax.highlights_compiling(0..text.len());
    let at = |s: &str| text.find(s).unwrap();
    let layers = syntax.layers();
    assert_eq!(layers.len(), 2, "{layers:?}");
    assert_eq!(layers[0].0, "sql");
    assert_eq!(
        layers[0].1[..2],
        [
            at("\n  SELECT a")..at("{$id}"),
            at("} AND") + 1..at("\n  SQL")
        ]
    );
    assert_eq!(layers[1].0, "sql");
    assert_eq!(layers[1].1[0].start, at("\n  SELECT c"));
}

/// The line breaks between a heredoc's lines, which no node covers, are
/// injected with its text: an SQL `--` comment ends at the end of its line,
/// as it does in a nowdoc.
#[test]
#[cfg(all(feature = "php", feature = "sql"))]
fn php_heredoc_line_breaks_end_sql_comments() {
    let text = "<?php\n\
                $q = <<<SQL\n  SELECT a -- by {$id}\n  FROM t\n\n  WHERE b = 1\n  SQL;\n\
                $r = <<<'SQL'\n  SELECT c -- all\n  FROM u\n  SQL;\n";
    let found = spans("php", text);
    assert_includes(
        &found,
        &[
            ("FROM", "keyword"),
            ("t", "type"),
            ("WHERE", "keyword"),
            ("1", "number"),
            ("u", "type"),
        ],
    );
    let froms = found
        .iter()
        .filter(|span| *span == &("FROM".into(), "keyword"));
    assert_eq!(froms.count(), 2, "{found:?}");
}

/// A heredoc in a language where line breaks matter, YAML, parses its lines
/// as lines, around an interpolation.
#[test]
#[cfg(feature = "php")]
fn php_heredocs_keep_lines_for_yaml() {
    let text = "<?php\n$y = <<<YAML\n  a: 1\n  b: {$x}\n  c: true\n  YAML;\n";
    let found = spans("php", text);
    assert_includes(
        &found,
        &[
            ("a", "property"),
            ("1", "number"),
            ("b", "property"),
            ("x", "variable"),
            ("c", "property"),
            ("true", "constant.builtin"),
        ],
    );
}

#[test]
fn only_injections_overlapping_the_range_are_parsed() {
    let block = |n| format!("Para {n} with `code`.\n\n```rust\nfn f{n}() {{}}\n```\n\n");
    let text: String = (0..6).map(block).collect();
    let mut buffer = Buffer::new(&text);
    let mut syntax = parsed("markdown", &buffer);
    let third = text.find("Para 2").unwrap();
    let fourth = text.find("Para 3").unwrap();

    syntax.highlights_compiling(third..fourth);
    let layers = syntax.layers();
    let languages: Vec<_> = layers.iter().map(|(language, _)| *language).collect();
    assert_eq!(languages, ["markdown_inline", "rust"]);
    assert!(
        layers
            .iter()
            .all(|(_, ranges)| third <= ranges[0].start && ranges[0].end <= fourth),
        "{layers:?}"
    );

    // Another range adds its own layers, and keeps those already parsed.
    syntax.highlights_compiling(fourth..text.len());
    assert_eq!(syntax.layers().len(), 2 + 3 * 2);

    // An edit keeps them, moved along.
    let moved: Vec<_> = syntax
        .layers()
        .into_iter()
        .map(|(language, ranges)| {
            (
                language,
                ranges.iter().map(|r| r.start + 1..r.end + 1).collect(),
            )
        })
        .collect();
    buffer.edit([(0..0, "x")], Origin::User);
    syntax.edit(buffer.current());
    assert_eq!(syntax.layers(), moved);
}

/// `count` blocks, each a paragraph with inline code and a Rust fence.
fn blocks(count: usize) -> String {
    (0..count)
        .map(|n| format!("Para {n} with `code`.\n\n```rust\nfn f{n}() {{}}\n```\n\n"))
        .collect()
}

/// Asserts that `syntax` highlights `buffer` and keeps the same layers as a
/// fresh parse does.
fn assert_fresh(name: &str, syntax: &Syntax, buffer: &Buffer, context: &str) {
    let fresh = parsed(name, buffer);
    let len = buffer.len();
    assert_eq!(
        syntax.highlights_compiling(0..len),
        fresh.highlights_compiling(0..len),
        "{context}: {:?}",
        buffer.text().to_string()
    );
    assert_eq!(
        syntax.layer_trees(),
        fresh.layer_trees(),
        "{context}: {:?}",
        buffer.text().to_string()
    );
}

#[test]
fn an_edit_reparses_only_the_layers_it_touches() {
    let text = blocks(4);
    let mut buffer = Buffer::new(&text);
    let mut syntax = parsed("markdown", &buffer);
    syntax.highlights_compiling(0..text.len());
    assert_eq!(syntax.take_layer_parses().len(), 4 * 2);

    // Typing in the second fence. Until the host is reparsed, the layer is
    // used as it is, like the host tree.
    let fence = text.find("fn f1").unwrap();
    buffer.edit([(fence + 4..fence + 4, "x")], Origin::User);
    syntax.edit(buffer.current());
    syntax.highlights_compiling(0..buffer.len());
    assert_eq!(syntax.take_layer_parses(), []);
    syntax.finish(syntax.parse_job().run());
    syntax.highlights_compiling(0..buffer.len());
    let fence = fence..fence + "fn f1x() {}\n".len();
    assert_eq!(syntax.take_layer_parses(), [("rust", vec![fence])]);
    assert_fresh("markdown", &syntax, &buffer, "typed in a fence");
    assert_eq!(syntax.take_layer_parses(), []);

    // Typing at the start moves every layer, but only reparses the first.
    let before = syntax.layers();
    buffer.edit([(0..0, "A ")], Origin::User);
    syntax.edit(buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax.highlights_compiling(0..buffer.len());
    let parses = syntax.take_layer_parses();
    let layers = syntax.layers();
    assert_eq!(parses, layers[..1]);
    assert_eq!(parses[0].0, "markdown_inline");
    let moved = |ranges: &[Range<usize>]| ranges.iter().map(|r| r.start + 2..r.end + 2).collect();
    assert_eq!(
        layers[1..],
        before[1..]
            .iter()
            .map(|(language, ranges)| (*language, moved(ranges)))
            .collect::<Vec<_>>()
    );
    assert_fresh("markdown", &syntax, &buffer, "typed at the start");
}

/// Deleting a fence or a `<script>` drops its layer once the host is
/// reparsed and the place is highlighted again.
#[test]
fn a_layer_whose_injection_is_gone_is_dropped() {
    let cases = [
        ("markdown", blocks(3), "```rust\nfn f1() {}\n```\n"),
        (
            "html",
            "<p>a</p>\n<script>let a = 1;</script>\n<style>p {}</style>\n".to_string(),
            "<script>let a = 1;</script>",
        ),
    ];
    for (name, text, gone) in cases {
        let mut buffer = Buffer::new(&text);
        let mut syntax = parsed(name, &buffer);
        syntax.highlights_compiling(0..text.len());
        let count = syntax.layers().len();
        let start = text.find(gone).unwrap();
        buffer.edit([(start..start + gone.len(), "")], Origin::User);
        syntax.edit(buffer.current());
        syntax.finish(syntax.parse_job().run());
        syntax.highlights_compiling(0..buffer.len());
        assert_eq!(syntax.layers().len(), count - 1, "{name}");
        assert_fresh(name, &syntax, &buffer, name);
    }
}

/// Deleting the whole text leaves every layer's ranges empty; highlighting
/// the empty buffer once the host is reparsed drops them all.
#[test]
fn deleting_everything_drops_every_layer() {
    let text = blocks(3);
    let mut buffer = Buffer::new(&text);
    let mut syntax = parsed("markdown", &buffer);
    syntax.highlights_compiling(0..text.len());
    assert!(!syntax.layers().is_empty());
    buffer.edit([(0..text.len(), "")], Origin::User);
    syntax.edit(buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax.highlights_compiling(0..0);
    assert_eq!(syntax.layers(), []);
}

/// Typing inside a template's `${…}` leaves the bytes of the HTML layer
/// around it as they were, so it isn't reparsed.
#[test]
fn an_edit_between_a_layers_ranges_reparses_nothing() {
    let text = "html`<p class=${c}>hi</p>`;\n";
    let mut buffer = Buffer::new(text);
    let mut syntax = parsed("javascript", &buffer);
    syntax.highlights_compiling(0..text.len());
    assert_eq!(syntax.take_layer_parses(), [("html", vec![5..14, 18..25])]);
    let c = text.find("c}").unwrap();
    for at in [c, c + 1] {
        buffer.edit([(at..at, "x")], Origin::User);
        syntax.edit(buffer.current());
        syntax.finish(syntax.parse_job().run());
        syntax.highlights_compiling(0..buffer.len());
        assert_eq!(syntax.take_layer_parses(), [], "typed at {at}");
        assert_fresh("javascript", &syntax, &buffer, "typed in ${…}");
        syntax.take_layer_parses();
    }
}

/// Showing a cached range again makes its layers recent, so the next range
/// far away drops the oldest range's layers, not its.
#[test]
fn a_cached_range_shown_again_keeps_its_layers() {
    let text = blocks(200);
    let syntax = parsed("markdown", &Buffer::new(&text));
    // A block every 20, far apart: each range keeps only its own layers.
    let block = |n: usize| {
        let start = |n| text.find(&format!("Para {n} ")).unwrap();
        start(n)..start(n + 1)
    };
    let kept = |n: usize| {
        let range = block(n);
        syntax
            .layers()
            .iter()
            .any(|(_, ranges)| range.start <= ranges[0].start && ranges[0].end <= range.end)
    };
    let ranges = super::RECENT_RANGES;
    for n in 0..ranges {
        syntax.highlights_compiling(block(20 * n));
    }
    // Cached, as nothing has changed.
    syntax.highlights_compiling(block(0));
    syntax.highlights_compiling(block(20 * ranges));
    assert!(kept(0) && !kept(20), "{:?}", syntax.layers());
    assert!((2..=ranges).all(|n| kept(20 * n)));
}

#[test]
fn layers_far_from_the_viewport_are_dropped() {
    let text = blocks(200);
    let syntax = parsed("markdown", &Buffer::new(&text));
    // About ten blocks.
    let screen = text.find("Para 10").unwrap();
    let mut start = 0;
    while start < text.len() {
        let end = (start + screen).min(text.len());
        syntax.highlights_compiling(start..end);
        // Within a screen of the last ranges highlighted.
        let near = start.saturating_sub(super::RECENT_RANGES * screen)..end + screen;
        let layers = syntax.layers();
        assert!(
            layers
                .iter()
                .all(|(_, ranges)| near.start <= ranges[0].end && ranges[0].start <= near.end),
            "{near:?}: {layers:?}"
        );
        start = end;
    }
    let count = syntax.layers().len();
    assert!(
        0 < count && count <= 2 * 10 * (super::RECENT_RANGES + 1),
        "{count}"
    );
}

/// The model is `incremental_reparse_matches_a_full_parse` in the crate's
/// tests: random batches of edits, with parses finishing late and highlights
/// in between, must leave the layers and highlights a fresh parse gives.
#[test]
fn random_edits_leave_layers_as_a_fresh_parse_would() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "markdown",
            "# A `b`\n\n> Quote `q` and <b>x</b>.\n> More.\n\n\
             ```rust\nfn main() { let x = 1; }\n```\n\nPara *em* `c`.\n\n\
             ```js\nconst a = `<p>${b}</p>`;\n```\n",
            &[
                "```",
                "```rust\n",
                "`",
                "\n",
                "\n\n",
                "> ",
                "fn ",
                "{",
                "}",
                "<i>",
                "x",
                " ",
                "",
            ],
        ),
        (
            "html",
            "<p>a</p>\n<script>const x = 1;\nlet y = html`<b>${x}</b>`;</script>\n\
             <style>p { color: red; }</style>\n",
            &[
                "<script>",
                "</script>",
                "<style>",
                "</style>",
                "{",
                "}",
                "`",
                "${",
                ";",
                "\n",
                "x",
                "",
                "<",
            ],
        ),
        (
            "javascript",
            "const a = html`<p class=${c}>hi</p>`;\ncss`a { color: red; }`;\nf(`plain`);\n",
            &["html`", "css`", "`", "${", "}", "<p>", "\n", "x", ";", ""],
        ),
    ];
    let mut rng = StdRng::seed_from_u64(17);
    for &(name, text, pieces) in cases {
        let mut buffer = Buffer::new(text);
        let mut syntax = parsed(name, &buffer);
        let mut in_flight = None;
        for step in 0..200 {
            for _ in 0..rng.random_range(1..3) {
                let edits: Vec<_> = (0..rng.random_range(1..3))
                    .map(|_| {
                        let start = rng.random_range(0..=buffer.len());
                        let end = (start + rng.random_range(0..6)).min(buffer.len());
                        (start..end, pieces[rng.random_range(0..pieces.len())])
                    })
                    .collect();
                buffer.edit(edits, Origin::User);
            }
            syntax.edit(buffer.current());
            match rng.random_range(0..4) {
                0 => in_flight = Some(syntax.parse_job()),
                1 => {
                    if let Some(job) = in_flight.take() {
                        syntax.finish(job.run());
                    }
                }
                2 => syntax.finish(syntax.parse_job().run()),
                _ => {}
            }
            // A frame, parsed or not, keeping only the layers near it.
            let start = rng.random_range(0..=buffer.len());
            let end = (start + rng.random_range(0..40)).min(buffer.len());
            syntax.highlights_compiling(start..end);
            if step % 4 == 0 {
                syntax.finish(syntax.parse_job().run());
                let fresh = parsed(name, &buffer);
                assert_eq!(
                    syntax.highlights_compiling(start..end),
                    fresh.highlights_compiling(start..end),
                    "{name} after step {step}: {:?}",
                    buffer.text().to_string()
                );
                assert_fresh(name, &syntax, &buffer, &format!("{name} after step {step}"));
            }
        }
    }
}

#[test]
fn unknown_injected_languages_stay_plain() {
    let text = "```mermaid\ngraph TD; A-->B\n```\n";
    assert_eq!(
        spans("markdown", text),
        [
            ("```", "punctuation.delimiter"),
            ("mermaid\ngraph TD; A-->B\n", "markup.raw"),
            ("```", "punctuation.delimiter"),
            ("\n", "markup.raw"),
        ]
        .map(|(text, highlight)| (text.to_string(), highlight))
    );

    // Regex and JSDoc have no grammar here; the host's highlights stay.
    let text = "/** @param {number} x */\nconst r = /ab+c/g;\nstyled.div`a`;\n";
    assert_includes(
        &spans("javascript", text),
        &[
            ("/** @param {number} x */", "comment"),
            ("ab+c", "string.special"),
            ("`a`", "string"),
        ],
    );
}

/// An injected language that isn't compiled yet stays plain and is reported
/// missing, even if it's compiled before it's asked for, as by another view.
/// The next call highlights it.
#[test]
fn missing_languages_are_reported() {
    let text = "```java\nclass A {}\n```\n";
    let syntax = parsed("markdown", &Buffer::new(text));
    let keyword =
        |spans: Vec<HighlightSpan>| spans.iter().any(|span| span.highlight.name() == "keyword");
    // Each test has its own process under nextest, so Java hasn't been
    // compiled yet; under `cargo test` another test may have compiled it.
    let first = keyword(syntax.highlights(0..text.len()));
    language_by_name("java");
    let missing: &[&str] = if first { &[] } else { &["java"] };
    assert_eq!(syntax.take_missing_languages(), missing);
    assert_eq!(syntax.take_missing_languages(), [] as [&str; 0]);
    assert!(keyword(syntax.highlights(0..text.len())));
}

/// Finding and parsing injections stop at the deadline, leave the
/// injections plain and report the result incomplete. The next call, with
/// time, highlights them.
#[test]
fn injections_out_of_time_stay_plain() {
    let pieces = "<b>${x}</b>".repeat(2000);
    let cases = [
        ("markdown", "rust", "```rust\nfn f() {}\n```\n".to_string()),
        ("javascript", "html", format!("html`{pieces}`;\n")),
    ];
    for (host, injected, text) in cases {
        language_by_name(injected);
        let buffer = Buffer::new(&text);
        let syntax = parsed(host, &buffer);
        let (language, tree) = (syntax.language(), syntax.tree().unwrap());
        let host_spans = highlight_query(language, buffer.text(), tree, 0..text.len(), true, None);
        let mut layers = Layers::default();
        let mut paint = |deadline| {
            let host = super::Host {
                language,
                text: buffer.text(),
                tree,
                parsed: true,
            };
            super::paint(
                host,
                0..text.len(),
                deadline,
                &mut layers,
                host_spans.clone(),
            )
        };
        assert_eq!(
            paint(Some(Instant::now())),
            (host_spans.clone(), false, false)
        );
        let (spans, complete, _) = paint(None);
        assert!(complete && spans != host_spans, "{injected}");
        assert_eq!(layers.keys().len(), 1);
    }
}

/// Collecting a content node's ranges stops at the deadline: a block quote
/// with a continuation on every line has as many ranges as lines.
#[test]
fn content_ranges_stop_at_the_deadline() {
    let text = "> line\n".repeat(1000);
    let syntax = parsed("markdown", &Buffer::new(&text));
    let root = syntax.tree().unwrap().root_node();
    let inline = root
        .named_descendant_for_byte_range(2, 3)
        .filter(|node| node.kind() == "inline")
        .expect("the quote's text");
    let mut ranges = Vec::new();
    let collected = super::content_ranges(inline, false, None, &mut ranges);
    assert_eq!((collected, ranges.len()), (Collected::All, 1000));
    ranges.clear();
    let collected = super::content_ranges(inline, false, Some(Instant::now()), &mut ranges);
    assert_eq!(collected, Collected::OutOfTime);
    assert!(ranges.len() <= 1);
}

/// Runs `f` with every deadline passing after `after` checks, and counts the
/// checks.
fn with_deadline_after<T>(after: usize, f: &mut dyn FnMut() -> T) -> (T, usize) {
    DEADLINE_CHECKS.set(0);
    DEADLINE_AFTER.set(after);
    let result = f();
    DEADLINE_AFTER.set(usize::MAX);
    (result, DEADLINE_CHECKS.get())
}

/// A deadline too far off to pass by the clock: it passes when the test
/// says, with [`with_deadline_after`].
fn far_deadline() -> Option<Instant> {
    Instant::now().checked_add(Duration::from_secs(3600))
}

/// Collecting a combined template string's pieces checks the deadline at
/// each of them, and finding its injection stops within a few checks of the
/// deadline passing, wherever that happens.
#[test]
fn combined_templates_stop_at_the_deadline() {
    let pieces = "<b>${x}</b>".repeat(2000);
    let text = format!("html`{pieces}`;\n");
    language_by_name("html");
    let buffer = Buffer::new(&text);
    let syntax = parsed("javascript", &buffer);
    let tree = syntax.tree().unwrap();
    let deadline = far_deadline();

    let fragment = tree
        .root_node()
        .named_descendant_for_byte_range(5, 6)
        .filter(|node| node.kind() == "string_fragment")
        .expect("the template's first piece");
    let pattern = super::Pattern {
        language: None,
        combined: true,
        include_children: true,
        include_gaps: false,
    };
    let collected = Cell::new(0);
    let mut collect = || {
        let mut ranges = Vec::new();
        let in_time = super::match_ranges(
            &[fragment, fragment],
            0,
            &pattern,
            &mut Default::default(),
            deadline,
            &mut ranges,
        );
        collected.set(ranges.len());
        in_time
    };
    // The first node, 2001 fragments, 2000 substitutions, two backticks,
    // and the second node, whose parent is already combined.
    let all = 1 + 4003 + 1;
    assert_eq!(
        with_deadline_after(usize::MAX, &mut collect),
        (Collected::All, all)
    );
    assert_eq!(collected.get(), 2001);
    for after in [0, 1, 2000, 4003, all - 1] {
        let stopped = with_deadline_after(after, &mut collect);
        assert_eq!(stopped, (Collected::OutOfTime, after + 1));
        assert!(collected.get() <= after.div_ceil(2), "{after}");
    }

    let mut layers = Layers::default();
    let mut find = || {
        let mut painter = super::Painter {
            skipped: false,
            text: buffer.text(),
            range: 0..text.len(),
            deadline,
            host_parsed: true,
            layers: &mut layers,
            parser: None,
            complete: true,
        };
        let found = painter.find(syntax.language(), tree, None);
        painter.complete && found.len() == 1
    };
    let (complete, checks) = with_deadline_after(usize::MAX, &mut find);
    assert!(complete && checks > 4003, "{checks} checks");
    for after in (0..checks).step_by(checks / 100) {
        let (complete, taken) = with_deadline_after(after, &mut find);
        assert!(!complete, "deadline after {after} checks");
        assert!(
            taken <= after + 3,
            "{taken} checks with the deadline after {after}"
        );
    }
}

fn spans_of(specs: &[(usize, usize, usize)]) -> Vec<HighlightSpan> {
    let mut start = 0;
    let mut spans: Vec<HighlightSpan> = Vec::new();
    for &(gap, len, highlight) in specs {
        start += gap;
        let range = start..start + len + 1;
        let highlight = Highlight::all().nth(highlight).unwrap();
        start = range.end;
        match spans.last_mut() {
            Some(last) if last.range.end == range.start && last.highlight == highlight => {
                last.range.end = range.end;
            }
            _ => spans.push(HighlightSpan { range, highlight }),
        }
    }
    spans
}

fn paint(spans: &[HighlightSpan], len: usize) -> Vec<Option<Highlight>> {
    let mut bytes = vec![None; len];
    for span in spans {
        bytes[span.range.clone()].fill(Some(span.highlight));
    }
    bytes
}

proptest! {
    /// Clipping then overlaying gives what painting the clipped top over the
    /// base byte by byte does.
    #[test]
    fn clipped_overlay_paints_each_byte(
        base in prop::collection::vec((0usize..4, 0usize..6, 0usize..3), 0..8),
        top in prop::collection::vec((0usize..4, 0usize..6, 0usize..3), 0..8),
        ranges in prop::collection::vec((0usize..6, 0usize..8), 0..5),
    ) {
        const LEN: usize = 128;
        let (base, top) = (spans_of(&base), spans_of(&top));
        let mut start = 0;
        let ranges: Vec<Range<usize>> = ranges
            .iter()
            .map(|&(gap, len)| {
                start += gap;
                let range = start..start + len + 1;
                start = range.end;
                range
            })
            .collect();
        let inside = |byte: usize| ranges.iter().any(|range| range.contains(&byte));
        let mut expected = paint(&base, LEN);
        for (byte, painted) in paint(&top, LEN).into_iter().enumerate() {
            if painted.is_some() && inside(byte) {
                expected[byte] = painted;
            }
        }
        let merged = overlay(base, clip(top, &ranges));
        for pair in merged.windows(2) {
            prop_assert_eq!(pair[0].range.end <= pair[1].range.start, true);
            prop_assert_eq!(
                pair[0].range.end < pair[1].range.start || pair[0].highlight != pair[1].highlight,
                true
            );
        }
        prop_assert_eq!(paint(&merged, LEN), expected);
    }
}

/// Paints the injections of `syntax`'s tree over all of `buffer`, with
/// `layers`, and says whether the result is complete.
fn paint_all(
    buffer: &Buffer,
    syntax: &Syntax,
    deadline: Option<Instant>,
    layers: &mut Layers,
) -> bool {
    let host = super::Host {
        language: syntax.language(),
        text: buffer.text(),
        tree: syntax.tree().unwrap(),
        parsed: true,
    };
    super::paint(host, 0..buffer.len(), deadline, layers, Vec::new()).1
}

/// Painting injections, from finding them to parsing, reusing and
/// highlighting their layers, stops within a few checks of the deadline
/// passing, wherever that happens: whether a layer of thousands of ranges is
/// parsed afresh, kept from an earlier call, or reparsed after an edit.
#[test]
fn painting_stops_at_the_deadline() {
    let pieces = "<b>${x}</b>".repeat(2000);
    let text = format!("html`{pieces}`;\n");
    language_by_name("html");
    let mut buffer = Buffer::new(&text);
    let mut syntax = parsed("javascript", &buffer);
    let deadline = far_deadline();
    // Paints from a copy of `layers` with the deadline passing at each of
    // about 100 steps, and returns the parses of a run in time.
    let assert_stops = |buffer: &Buffer, syntax: &Syntax, layers: &Layers, case: &str| {
        let run = |after| {
            let mut layers = layers.clone();
            layers.parses.clear();
            let paint = &mut || paint_all(buffer, syntax, deadline, &mut layers);
            let (complete, checks) = with_deadline_after(after, paint);
            (complete, checks, layers.parses)
        };
        let (complete, checks, parses) = run(usize::MAX);
        assert!(complete, "{case}");
        for after in (0..checks).step_by(checks.div_ceil(100)) {
            let (complete, taken, _) = run(after);
            assert!(!complete, "{case}: deadline after {after} checks");
            assert!(
                taken <= after + 3,
                "{case}: {taken} checks with the deadline after {after}"
            );
        }
        parses
    };
    let layer = |ranges| vec![("html", ranges)];

    let parses = assert_stops(&buffer, &syntax, &Layers::default(), "parsed afresh");
    assert_eq!(parses.len(), 1);
    let ranges = parses[0].1.clone();
    assert_eq!(ranges.len(), 2001);

    syntax.highlights_compiling(0..text.len());
    assert_eq!(syntax.layers(), layer(ranges.clone()));
    let kept = syntax.kept_layers();
    assert_eq!(assert_stops(&buffer, &syntax, &kept, "kept"), []);

    // Typing in the last piece.
    let at = text.len() - "</b>`;\n".len();
    buffer.edit([(at..at, "i")], Origin::User);
    syntax.edit(buffer.current());
    syntax.finish(syntax.parse_job().run_until(|_| || false));
    let edited = syntax.kept_layers();
    let mut ranges = ranges;
    ranges.last_mut().unwrap().end += 1;
    let parses = assert_stops(&buffer, &syntax, &edited, "edited");
    assert_eq!(parses, layer(ranges));
}

/// However early or late the deadline passes, painting `text` as `host`
/// isn't complete unless it ran to the end: in particular when it passes
/// inside the query of the last injected layer, which no injection query or
/// deeper layer checks after.
fn assert_incomplete_unless_run_to_the_end(host: &str, text: &str) {
    let buffer = Buffer::new(text);
    let syntax = parsed(host, &buffer);
    // Compile the injected languages' queries.
    syntax.highlights_compiling(0..text.len());
    let deadline = far_deadline();
    let paint = |after| {
        let paint = &mut || paint_all(&buffer, &syntax, deadline, &mut Layers::default());
        with_deadline_after(after, paint)
    };
    let (complete, checks) = paint(usize::MAX);
    assert!(complete);
    assert!(checks > 0);
    for after in 0..checks {
        let (complete, _) = paint(after);
        assert!(
            !complete,
            "{host}: complete with the deadline after {after} of {checks}"
        );
    }
}

/// TOML has no injection query, so nothing checks the deadline after its
/// layer's highlight query stops.
#[test]
fn a_timeout_in_the_last_injected_layer_is_incomplete() {
    let text = "# T\n\n```toml\n[[a]]\nname = \"x\"\nsize = 3\n```\n";
    assert_incomplete_unless_run_to_the_end("markdown", text);
}

/// A layer at [`MAX_DEPTH`] is highlighted, but its own injections aren't
/// looked for, so nothing checks the deadline after its query either.
#[test]
fn a_timeout_in_a_layer_at_the_depth_limit_is_incomplete() {
    let mut text = "[[a]]\nname = \"x\"\n".to_string();
    for _ in 0..super::MAX_DEPTH {
        text = format!("````markdown\n{text}````\n");
    }
    text = format!("```toml\n{text}```\n");
    let text = text.replacen("```toml", "```markdown", 1);
    assert_incomplete_unless_run_to_the_end("markdown", &text);
}

/// An injection with too many ranges stays plain, and one with just few
/// enough is parsed. Either way the result is complete: it would be the same
/// every time.
#[test]
fn an_injection_with_too_many_ranges_stays_plain() {
    language_by_name("html");
    for (pieces, layers) in [
        (super::MAX_LAYER_RANGES - 1, 1),
        (super::MAX_LAYER_RANGES, 0),
    ] {
        // `pieces` substitutions between `pieces + 1` fragments.
        let text = format!("html`{}a`;\n", "a${x}".repeat(pieces));
        let buffer = Buffer::new(&text);
        let syntax = parsed("javascript", &buffer);
        let mut kept = Layers::default();
        assert!(paint_all(&buffer, &syntax, None, &mut kept), "{pieces}");
        assert_eq!(kept.keys().len(), layers, "{pieces}");
    }
}

/// Past [`super::MAX_INJECTIONS`] in a tree, injections stay plain, and past
/// [`super::MAX_KEPT_LAYERS`] layers are parsed and used but not kept.
#[test]
fn injections_and_layers_kept_are_bounded() {
    let text = "`c`\n\n".repeat(super::MAX_INJECTIONS + 1);
    language_by_name("markdown_inline");
    let buffer = Buffer::new(&text);
    let syntax = parsed("markdown", &buffer);
    let mut layers = Layers::default();
    assert!(paint_all(&buffer, &syntax, None, &mut layers));
    // Each paragraph's inline markup is a layer.
    assert_eq!(layers.parses.len(), super::MAX_INJECTIONS);
    assert_eq!(layers.keys().len(), super::MAX_KEPT_LAYERS);
}

/// A large raw Go format string on a cached layer is scanned under the
/// deadline: it reports incomplete once the deadline passes, and a range
/// away from the string reads none of it.
#[cfg(all(feature = "go", feature = "printf"))]
#[test]
fn go_format_scan_stops_at_the_deadline() {
    language_by_name("printf");
    let text = format!(
        "package p\nfunc f() {{\n\tfmt.Printf(`{}`, x)\n}}\n",
        "%d ".repeat(200_000)
    );
    let text = text.replace("\\\n", "\n").replace("\\\t", "\t");
    let buffer = Buffer::new(&text);
    let syntax = parsed("go", &buffer);
    let deadline = far_deadline();
    let mut layers = Layers::default();
    assert!(paint_all(&buffer, &syntax, deadline, &mut layers));
    let paint = &mut || paint_all(&buffer, &syntax, deadline, &mut layers.clone());
    let (complete, checks) = with_deadline_after(usize::MAX, paint);
    assert!(complete);
    for after in [0, checks / 2] {
        let (complete, _) = with_deadline_after(after, paint);
        assert!(!complete, "deadline after {after} checks");
    }
}
