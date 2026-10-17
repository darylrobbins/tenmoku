use fe_markdown::{Align, BlockKind, Inline, parse};

fn text(text: &str) -> Inline {
    Inline::Text(text.into())
}

fn link(dest: &str, shown: &str) -> Inline {
    Inline::Link {
        dest: dest.into(),
        title: String::new(),
        content: vec![text(shown)],
    }
}

/// The inlines of a one-paragraph document.
fn inlines(markdown: &str) -> Vec<Inline> {
    let document = parse(markdown);
    match &document.blocks[..] {
        [block] => match &block.kind {
            BlockKind::Paragraph(content) => content.clone(),
            kind => panic!("not a paragraph: {kind:?}"),
        },
        blocks => panic!("not one block: {blocks:?}"),
    }
}

/// Each link's text and destination.
fn links(markdown: &str) -> Vec<(String, String)> {
    fn collect(inlines: &[Inline], out: &mut Vec<(String, String)>) {
        for inline in inlines {
            match inline {
                Inline::Link { dest, content, .. } => {
                    out.push((Inline::plain_text(content), dest.clone()))
                }
                Inline::Emphasis(content)
                | Inline::Strong(content)
                | Inline::Strikethrough(content) => collect(content, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    collect(&inlines(markdown), &mut out);
    out
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(shown, dest)| (shown.to_string(), dest.to_string()))
        .collect()
}

#[test]
fn gfm_constructs_parse() {
    // Tables: alignments, head and body cells.
    let document = parse("| a | *b* | c | d |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n");
    let BlockKind::Table { align, head, rows } = &document.blocks[0].kind else {
        panic!("not a table");
    };
    assert_eq!(
        align,
        &[Align::Left, Align::Center, Align::Right, Align::None]
    );
    assert_eq!(
        head,
        &[
            vec![text("a")],
            vec![Inline::Emphasis(vec![text("b")])],
            vec![text("c")],
            vec![text("d")],
        ]
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].cells,
        [
            vec![text("1")],
            vec![text("2")],
            vec![text("3")],
            vec![text("4")]
        ]
    );

    // Task items, tight and loose.
    for markdown in [
        "- [x] done\n- [ ] todo\n- plain\n",
        "- [x] done\n\n- [ ] todo\n\n- plain\n",
    ] {
        let document = parse(markdown);
        let BlockKind::List { items, .. } = &document.blocks[0].kind else {
            panic!("not a list");
        };
        let tasks: Vec<_> = items.iter().map(|item| item.task).collect();
        assert_eq!(tasks, [Some(true), Some(false), None], "{markdown:?}");
        let BlockKind::Paragraph(content) = &items[1].blocks[0].kind else {
            panic!("not a paragraph");
        };
        assert_eq!(content, &[text("todo")], "{markdown:?}");
    }
    // Only an item's first marker makes it a task; a nested item has its own.
    let document = parse("- a\n  - [x] b\n");
    let BlockKind::List { items, .. } = &document.blocks[0].kind else {
        panic!("not a list");
    };
    assert_eq!(items[0].task, None);
    let BlockKind::List { items: nested, .. } = &items[0].blocks[1].kind else {
        panic!("not a nested list");
    };
    assert_eq!(nested[0].task, Some(true));

    // Strikethrough.
    assert_eq!(
        inlines("a ~~gone~~ b"),
        [
            text("a "),
            Inline::Strikethrough(vec![text("gone")]),
            text(" b")
        ]
    );

    // Angle-bracket autolinks, which pulldown-cmark finds itself.
    assert_eq!(
        links("<https://example.com/a> and <me@example.com>"),
        pairs(&[
            ("https://example.com/a", "https://example.com/a"),
            ("me@example.com", "mailto:me@example.com"),
        ])
    );

    // Literal URLs, `www.` hosts and e-mail addresses.
    assert_eq!(
        inlines("See https://example.com/x?y=1, or www.example.org."),
        [
            text("See "),
            link("https://example.com/x?y=1", "https://example.com/x?y=1"),
            text(", or "),
            link("http://www.example.org", "www.example.org"),
            text("."),
        ]
    );
    assert_eq!(
        links("Mail foo.bar+baz@example.co.uk!"),
        pairs(&[(
            "foo.bar+baz@example.co.uk",
            "mailto:foo.bar+baz@example.co.uk"
        )])
    );
    // Trailing punctuation is left out, and so is a `)` without its `(`.
    assert_eq!(
        links("(see https://en.wikipedia.org/wiki/Rust_(language)), https://a.example/b?!:*_~"),
        pairs(&[
            (
                "https://en.wikipedia.org/wiki/Rust_(language)",
                "https://en.wikipedia.org/wiki/Rust_(language)"
            ),
            ("https://a.example/b", "https://a.example/b"),
        ])
    );
    assert_eq!(
        links("www.example.com/a)b) and (www.example.com/c)"),
        pairs(&[
            ("www.example.com/a)b", "http://www.example.com/a)b"),
            ("www.example.com/c", "http://www.example.com/c"),
        ])
    );
    // In emphasis, after `*`, `_`, `~` or `(`, but not inside a word.
    assert_eq!(
        links("*www.a.com* _https://b.com_ xwww.c.com nothttps://d.com"),
        pairs(&[
            ("www.a.com", "http://www.a.com"),
            ("https://b.com", "https://b.com"),
        ])
    );

    // Not in code spans, existing links or images' alt text.
    assert_eq!(
        inlines("`https://a.com` [www.b.com](https://c.com) ![https://d.com](e.png)"),
        [
            Inline::Code("https://a.com".into()),
            text(" "),
            Inline::Link {
                dest: "https://c.com".into(),
                title: String::new(),
                content: vec![text("www.b.com")],
            },
            text(" "),
            Inline::Image {
                dest: "e.png".into(),
                title: String::new(),
                alt: "https://d.com".into(),
            },
        ]
    );
    // A URL straight after a code span is outside it.
    assert_eq!(
        links("`a`https://b.com"),
        pairs(&[("https://b.com", "https://b.com")])
    );
    // Nor in code blocks.
    let document = parse("    https://a.com\n");
    assert!(
        matches!(&document.blocks[0].kind, BlockKind::Code { code, .. } if code == "https://a.com\n")
    );
}

#[test]
fn autolinks_in_headings_tables_and_lists() {
    let document = parse("# www.a.com\n\n| https://b.com |\n|-|\n\n- c@d.com\n");
    let BlockKind::Heading { content, slug, .. } = &document.blocks[0].kind else {
        panic!("not a heading");
    };
    assert_eq!(content, &[link("http://www.a.com", "www.a.com")]);
    assert_eq!(slug, "wwwacom");
    let BlockKind::Table { head, .. } = &document.blocks[1].kind else {
        panic!("not a table");
    };
    assert_eq!(head[0], [link("https://b.com", "https://b.com")]);
    let BlockKind::List { items, .. } = &document.blocks[2].kind else {
        panic!("not a list");
    };
    assert_eq!(
        items[0].blocks[0].kind,
        BlockKind::Paragraph(vec![link("mailto:c@d.com", "c@d.com")])
    );
}

#[test]
fn raw_html_is_kept_as_source() {
    assert_eq!(
        inlines("a <b>bold</b> c"),
        [
            text("a "),
            Inline::Html("<b>".into()),
            text("bold"),
            Inline::Html("</b>".into()),
            text(" c"),
        ]
    );
}

#[test]
fn footnotes_and_math_stay_text() {
    assert_eq!(inlines("a[^1] $x$"), [text("a[^1] $x$")]);
}

#[test]
fn literal_links_are_found_in_the_source_before_its_markup() {
    // An entity at the end is left out, by its source; one inside is decoded.
    assert_eq!(
        inlines("www.a.com/b&amp;"),
        [link("http://www.a.com/b", "www.a.com/b"), text("&")]
    );
    assert_eq!(
        links("https://a.com/?x=1&amp;y=2"),
        pairs(&[("https://a.com/?x=1&y=2", "https://a.com/?x=1&y=2")])
    );
    // Emphasis characters inside a URL are part of it.
    assert_eq!(
        inlines("https://a.com/foo*bar*baz"),
        [link(
            "https://a.com/foo*bar*baz",
            "https://a.com/foo*bar*baz"
        )]
    );
    assert_eq!(
        inlines("*https://a.com/b*"),
        [Inline::Emphasis(vec![link(
            "https://a.com/b",
            "https://a.com/b"
        )])]
    );
    // A delimiter whose pair the link took is shown as written.
    assert_eq!(
        inlines("https://a.com/*b c*"),
        [link("https://a.com/*b", "https://a.com/*b"), text(" c*")]
    );
    assert_eq!(
        inlines("*see https://a.com/x*y z"),
        [
            text("*see "),
            link("https://a.com/x*y", "https://a.com/x*y"),
            text(" z")
        ]
    );
    assert_eq!(
        inlines("https://a.com/[x y](z)"),
        [link("https://a.com/[x", "https://a.com/[x"), text(" y](z)")]
    );
    assert_eq!(
        inlines("https://a.com/`x y`"),
        [link("https://a.com/`x", "https://a.com/`x"), text(" y`")]
    );
    // Escaped punctuation shows unescaped, as in the rest of the text.
    assert_eq!(
        inlines("https://a.com/a\\_b\\*c"),
        [link("https://a.com/a_b*c", "https://a.com/a_b*c")]
    );
    // A link ends at the end of its line.
    assert_eq!(
        inlines("https://a.com/x\nwww.b.com"),
        [
            link("https://a.com/x", "https://a.com/x"),
            Inline::SoftBreak,
            link("http://www.b.com", "www.b.com")
        ]
    );
    // Text after the link is searched too.
    assert_eq!(
        links("https://a.com/*b* www.c.com"),
        pairs(&[
            ("https://a.com/*b", "https://a.com/*b"),
            ("www.c.com", "http://www.c.com")
        ])
    );
}

#[test]
fn literal_links_start_at_the_start_of_quotes_and_cells() {
    let document =
        parse(">www.a.com\n>https://b.com\n\n|www.c.com|https://d.com|\n|-|-|\n|e@f.com|x|\n");
    let BlockKind::Quote(quoted) = &document.blocks[0].kind else {
        panic!("not a quote");
    };
    assert_eq!(
        quoted[0].kind,
        BlockKind::Paragraph(vec![
            link("http://www.a.com", "www.a.com"),
            Inline::SoftBreak,
            link("https://b.com", "https://b.com"),
        ])
    );
    let BlockKind::Table { head, rows, .. } = &document.blocks[1].kind else {
        panic!("not a table");
    };
    assert_eq!(
        head,
        &[
            vec![link("http://www.c.com", "www.c.com")],
            vec![link("https://d.com", "https://d.com")]
        ]
    );
    assert_eq!(rows[0].cells[0], [link("mailto:e@f.com", "e@f.com")]);
}

#[test]
fn urls_start_after_anything_but_a_letter() {
    assert_eq!(
        links("\"https://a.com\" URL:https://b.com 1https://c.com"),
        pairs(&[
            ("https://a.com", "https://a.com"),
            ("https://b.com", "https://b.com"),
            ("https://c.com", "https://c.com"),
        ])
    );
    assert_eq!(links("xhttps://a.com \"www.b.com"), pairs(&[]));
    assert_eq!(links("www. and https://. and https://-"), pairs(&[]));
}

#[test]
fn a_tag_a_link_ends_in_keeps_the_rest_of_its_source() {
    assert_eq!(
        inlines("https://a.com/[x](z \"title\") tail"),
        [
            link("https://a.com/[x](z", "https://a.com/[x](z"),
            text(" \"title\") tail")
        ]
    );
    assert_eq!(
        inlines("https://a.com/![x](z \"t\") tail"),
        [
            link("https://a.com/![x](z", "https://a.com/![x](z"),
            text(" \"t\") tail")
        ]
    );
}

#[test]
fn literal_links_start_lines_after_breaks_in_compact_quotes() {
    for (markdown, separator) in [
        (">a\n>www.a.com\n>https://b.com", Inline::SoftBreak),
        (">a\\\n>www.a.com\\\n>https://b.com", Inline::HardBreak),
        ("a  \nwww.a.com  \nhttps://b.com", Inline::HardBreak),
    ] {
        let document = parse(markdown);
        let content = match &document.blocks[0].kind {
            BlockKind::Quote(quoted) => match &quoted[0].kind {
                BlockKind::Paragraph(content) => content.clone(),
                kind => panic!("not a paragraph: {kind:?}"),
            },
            BlockKind::Paragraph(content) => content.clone(),
            kind => panic!("not a quote or paragraph: {kind:?}"),
        };
        assert_eq!(
            content,
            [
                text("a"),
                separator.clone(),
                link("http://www.a.com", "www.a.com"),
                separator.clone(),
                link("https://b.com", "https://b.com"),
            ],
            "{markdown:?}"
        );
    }
}

#[test]
fn hosts_keep_an_underscore_their_link_goes_on_past() {
    assert_eq!(
        links("https://a.com_/x www.a.com_/x https://a.com_:8080 www.a.com_."),
        pairs(&[("www.a.com", "http://www.a.com")])
    );
}

#[test]
fn email_addresses_are_found_in_the_decoded_text() {
    assert_eq!(
        inlines("foo\\+bar@example.com"),
        [link("mailto:foo+bar@example.com", "foo+bar@example.com")]
    );
    assert_eq!(
        inlines("foo&#64;example.com."),
        [link("mailto:foo@example.com", "foo@example.com"), text(".")]
    );
    assert_eq!(
        inlines("*a@b.com* and a*b@c.com"),
        [
            Inline::Emphasis(vec![link("mailto:a@b.com", "a@b.com")]),
            text(" and a*"),
            link("mailto:b@c.com", "b@c.com"),
        ]
    );
    // Not in code spans, links or images.
    assert_eq!(
        links("`a@b.com` [c@d.com](x) ![e@f.com](y) https://g.com/h@i.com"),
        pairs(&[
            ("c@d.com", "x"),
            ("https://g.com/h@i.com", "https://g.com/h@i.com")
        ])
    );
}

#[test]
fn rejected_candidates_take_linear_time() {
    let n = 50_000;
    for markdown in [
        format!("{}_x.y", "_www.".repeat(n)),
        format!("{}x", "(www.a.b_/".repeat(n)),
        format!("{}x", "(www.a.b_!".repeat(n)),
        format!("{}x", "https://a_.b_".repeat(n)),
        format!("{}x", "a.b@".repeat(n)),
        "a*b ".repeat(n),
    ] {
        let started = std::time::Instant::now();
        parse(&markdown);
        let took = started.elapsed();
        assert!(took.as_secs() < 5, "{:?}: {took:?}", &markdown[..12]);
    }
}
