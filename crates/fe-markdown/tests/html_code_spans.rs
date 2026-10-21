//! rustdoc's `<code>` and other phrasing tags, as rust-analyzer's hovers
//! carry them, read as Markdown inlines (T256).

use fe_markdown::{Block, BlockKind, Inline, parse};

/// rust-analyzer 1.98.1's hover over `Option::transpose`.
const TRANSPOSE: &str = include_str!("fixtures/ra_hover_transpose.md");

const SOME: &str = "https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.Some";
const OK: &str = "https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Ok";

fn text(text: &str) -> Inline {
    Inline::Text(text.into())
}

fn code(code: &str) -> Inline {
    Inline::Code(code.into())
}

fn html(html: &str) -> Inline {
    Inline::Html(html.into())
}

fn code_link(dest: &str, shown: &str) -> Inline {
    Inline::Link {
        dest: dest.into(),
        title: String::new(),
        content: vec![code(shown)],
    }
}

/// The inlines of a one-paragraph document.
fn inlines(markdown: &str) -> Vec<Inline> {
    match &parse(markdown).blocks[..] {
        [
            Block {
                kind: BlockKind::Paragraph(content),
                ..
            },
        ] => content.clone(),
        blocks => panic!("not one paragraph: {blocks:?}"),
    }
}

/// Every block, those in quotes and lists too, in document order.
fn all_blocks(blocks: &[Block]) -> Vec<&Block> {
    let mut out = Vec::new();
    for block in blocks {
        out.push(block);
        match &block.kind {
            BlockKind::Quote(blocks) => out.extend(all_blocks(blocks)),
            BlockKind::List { items, .. } => {
                for item in items {
                    out.extend(all_blocks(&item.blocks));
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether raw HTML is anywhere in `inlines`.
fn has_html(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Html(_) => true,
        Inline::Emphasis(content)
        | Inline::Strong(content)
        | Inline::Strikethrough(content)
        | Inline::Link { content, .. } => has_html(content),
        _ => false,
    })
}

#[test]
fn rust_analyzer_hover_has_no_html_left() {
    let document = parse(TRANSPOSE);
    for block in all_blocks(&document.blocks) {
        match &block.kind {
            BlockKind::Html(html) => panic!("an HTML block at {:?}: {html:?}", block.lines),
            BlockKind::Paragraph(content) | BlockKind::Heading { content, .. } => {
                assert!(!has_html(content), "HTML in {content:?}")
            }
            _ => {}
        }
    }
}

#[test]
fn rust_analyzer_code_html_block_becomes_code_links() {
    let document = parse(TRANSPOSE);
    let transposes = (document.blocks.iter())
        .position(|block| match &block.kind {
            BlockKind::Paragraph(content) => Inline::plain_text(content).starts_with("Transposes "),
            _ => false,
        })
        .expect("the summary");
    let block = &document.blocks[transposes + 1];
    // The `<code>` block's rows, all of them.
    assert_eq!(block.lines, 18..26);
    let BlockKind::Paragraph(content) = &block.kind else {
        panic!("not a paragraph: {block:?}");
    };
    // `Some(Ok(_))`, its names links with code labels, the rest code.
    assert_eq!(
        content[..5],
        [
            code_link(SOME, "Some"),
            code("("),
            code_link(OK, "Ok"),
            code("(_))"),
            text(" is mapped to "),
        ]
    );
    assert_eq!(
        Inline::plain_text(content),
        "Some(Ok(_)) is mapped to Ok(Some(_)), Some(Err(_)) is mapped to Err(_), \
         and None will be mapped to Ok(None)."
    );
    assert!(!Inline::plain_text(content).contains('\\'));
}

#[test]
fn inline_code_tags_mid_paragraph_are_code() {
    assert_eq!(
        inlines("a <code>x</code> b"),
        [text("a "), code("x"), text(" b")]
    );
    // An escape resolves, and a code span joins the code around it.
    assert_eq!(inlines("<code>a\\_`b`</code>"), [code("a_b")]);
    // A line ending inside the tags goes outside them.
    assert_eq!(
        inlines("a<code>\nx\ny\n</code>b"),
        [
            text("a"),
            Inline::SoftBreak,
            code("x y"),
            Inline::SoftBreak,
            text("b")
        ]
    );
    // One beside a space outside goes.
    assert_eq!(inlines("a <code>\nx</code>"), [text("a "), code("x")]);
    // `<kbd>` is code too, in any case.
    assert_eq!(inlines("<KBD>k</KBD>"), [code("k")]);
    // A space may end a tag.
    assert_eq!(inlines("a <code >x</code >"), [text("a "), code("x")]);
}

#[test]
fn phrasing_tags_are_their_markdown() {
    assert_eq!(
        inlines("<b>a</b> <strong>b</strong>"),
        [
            Inline::Strong(vec![text("a")]),
            text(" "),
            Inline::Strong(vec![text("b")]),
        ]
    );
    assert_eq!(
        inlines("<i>a</i><em>b</em><s>c</s><del>d</del>"),
        [
            Inline::Emphasis(vec![text("a")]),
            Inline::Emphasis(vec![text("b")]),
            Inline::Strikethrough(vec![text("c")]),
            Inline::Strikethrough(vec![text("d")]),
        ]
    );
    // No inline for these: their content shows.
    assert_eq!(inlines("x<sup>2</sup>"), [text("x2")]);
    assert_eq!(
        inlines("a<br>b<br/>c<br />d"),
        [
            text("a"),
            Inline::HardBreak,
            text("b"),
            Inline::HardBreak,
            text("c"),
            Inline::HardBreak,
            text("d"),
        ]
    );
    // Inside code, emphasis stays and its text is code.
    assert_eq!(
        inlines("<code>a <b>b</b></code>"),
        [code("a "), Inline::Strong(vec![code("b")])]
    );
}

#[test]
fn other_html_stays_html() {
    // An unknown tag.
    assert_eq!(
        inlines("a <span>x</span>"),
        [text("a "), html("<span>"), text("x"), html("</span>")]
    );
    // A tag with an attribute.
    assert_eq!(
        inlines("<code class=\"x\">y</code>"),
        [html("<code class=\"x\">"), text("y"), html("</code>")]
    );
    // An unbalanced tag.
    assert_eq!(
        inlines("a <code>x"),
        [text("a "), html("<code>"), text("x")]
    );
    assert_eq!(inlines("x</code>"), [text("x"), html("</code>")]);
    // A closer is the innermost open tag's, attributes or not.
    assert_eq!(
        inlines("<code><code class=\"x\">a</code>"),
        [
            html("<code>"),
            html("<code class=\"x\">"),
            text("a"),
            html("</code>")
        ]
    );
    assert_eq!(
        inlines("<b><span>a</b></span>"),
        [
            html("<b>"),
            html("<span>"),
            text("a"),
            html("</b>"),
            html("</span>")
        ]
    );
    // Tags left open in a Markdown tag stay open after it.
    assert_eq!(
        inlines("a <code>*<code class=\"x\">b*</code> c"),
        [
            text("a "),
            html("<code>"),
            Inline::Emphasis(vec![html("<code class=\"x\">"), text("b")]),
            html("</code>"),
            text(" c"),
        ]
    );
    assert_eq!(
        inlines("<b>*<span>a*</b>"),
        [
            html("<b>"),
            Inline::Emphasis(vec![html("<span>"), text("a")]),
            html("</b>"),
        ]
    );
    assert_eq!(
        inlines("<code>[<code>a](u)</code>"),
        [
            html("<code>"),
            Inline::Link {
                dest: "u".into(),
                title: String::new(),
                content: vec![html("<code>"), text("a")],
            },
            html("</code>"),
        ]
    );
    // Void and self-closing tags open nothing.
    assert_eq!(
        inlines("<b>a<img>b<x/></b>"),
        [Inline::Strong(vec![
            text("a"),
            html("<img>"),
            text("b"),
            html("<x/>")
        ])]
    );
    // Tags paired across Markdown's.
    assert_eq!(
        inlines("<code>*a</code>*"),
        [
            html("<code>"),
            Inline::Emphasis(vec![text("a"), html("</code>")])
        ]
    );
    assert_eq!(
        inlines("*a <b>b* c</b>"),
        [
            Inline::Emphasis(vec![text("a "), html("<b>"), text("b")]),
            text(" c"),
            html("</b>"),
        ]
    );
    // Crossed pairs: the inner one maps.
    assert_eq!(
        inlines("<b><i>x</b></i>"),
        [html("<b>"), Inline::Emphasis(vec![text("x"), html("</b>")])]
    );
}

#[test]
fn html_blocks_of_phrasing_tags_are_paragraphs() {
    assert_eq!(
        inlines("<code>\n[a](u)(\\_)</code>"),
        [
            Inline::Link {
                dest: "u".into(),
                title: String::new(),
                content: vec![code("a")],
            },
            code("(_)"),
        ]
    );
    // A reference defined elsewhere in the document resolves in one.
    let document = parse("<b>\n[a]</b>\n\n[a]: https://a.com");
    assert_eq!(
        document.blocks[0].kind,
        BlockKind::Paragraph(vec![Inline::Strong(vec![Inline::Link {
            dest: "https://a.com".into(),
            title: String::new(),
            content: vec![text("a")],
        }])])
    );
    // A space may end its first tag.
    assert_eq!(inlines("<code >\nx</code>"), [code("x")]);
    // Its rows are the block's.
    let document = parse("x\n\n<b>\ny\nz</b>\n\nw");
    assert_eq!(document.blocks[1].lines, 2..5);
}

#[test]
fn html_blocks_with_other_html_stay_html() {
    for markdown in [
        // An unknown tag, one with attributes, and an unbalanced one.
        "<span>\nx</span>\n",
        "<code class=\"x\">\nx</code>\n",
        "<code>\nx\n",
        "</code>\nx\n",
        // Other HTML with them.
        "<code>\nx</code><span>y</span>\n",
        "<code>\nx</code><!-- y -->\n",
        // A block tag.
        "<div>\nx\n</div>\n",
        // A closer that's an attributed tag's, not the mapped one's.
        "<code>\n<code class=\"x\">a</code>\n",
        "<b>\n<span>a</b></span>\n",
    ] {
        let document = parse(markdown);
        assert!(
            matches!(&document.blocks[..], [Block { kind: BlockKind::Html(html), .. }] if html == markdown),
            "{markdown:?}: {:?}",
            document.blocks
        );
    }
}

#[test]
fn html_blocks_in_containers_are_paragraphs() {
    let document = parse("> <code>\n> x</code>\n");
    let BlockKind::Quote(blocks) = &document.blocks[0].kind else {
        panic!("not a quote: {document:?}");
    };
    assert_eq!(blocks[0].kind, BlockKind::Paragraph(vec![code("x")]));
}

#[test]
fn html_block_lines_that_look_like_blocks_are_text() {
    for (markdown, shown) in [
        ("<code>\n# title</code>", "# title"),
        ("<code>\n- item</code>", "- item"),
        ("<code>\n1. item</code>", "1. item"),
        ("<code>\n> quote</code>", "> quote"),
        ("<code>\n```</code>", "```"),
        ("<b>\nx\n===\ny</b>", "x === y"),
        ("<b>\nx\n---</b>", "x ---"),
        ("<b>\n|a|b|\n|-|-|</b>", "|a|b| |-|-|"),
    ] {
        let document = parse(markdown);
        let [
            Block {
                kind: BlockKind::Paragraph(content),
                ..
            },
        ] = &document.blocks[..]
        else {
            panic!("{markdown:?}: {:?}", document.blocks);
        };
        assert_eq!(Inline::plain_text(content), shown, "{markdown:?}");
    }
}

#[test]
fn html_block_inlines_read_as_a_paragraph_reads_them() {
    assert_eq!(
        inlines("<b>\n`a\nb`</b>"),
        [Inline::Strong(vec![code("a b")])]
    );
    assert_eq!(
        inlines("<b>\n[a](u \"t\nu\")</b>"),
        [Inline::Strong(vec![Inline::Link {
            dest: "u".into(),
            title: "t\nu".into(),
            content: vec![text("a")],
        }])]
    );
    // Multi-line code spans and titles keep no indent the block's lines
    // were read with, and space at a line's start is dropped as in any
    // paragraph.
    for markdown in [
        "`a\nb`",
        "`a\n   b`",
        "`` a\n` ``",
        "` a\nb`",
        "`  `",
        "[a](u \"t\nu\")",
        "[a](u 't\n  u')",
        "![a\nb](u \"t\nu\")",
        "a\n   b",
        "a  \nb",
        "a\\\nb",
    ] {
        let block = parse(&format!("<b>\n{markdown}</b>"));
        let [
            Block {
                kind: BlockKind::Paragraph(content),
                ..
            },
        ] = &block.blocks[..]
        else {
            panic!("{markdown:?}: {:?}", block.blocks);
        };
        assert_eq!(
            content,
            &[Inline::Strong(inlines(markdown))],
            "{markdown:?}"
        );
    }
}

#[test]
fn html_block_rows_that_start_blocks_keep_their_text() {
    // A row that would start a block is kept from it by an escape, which a
    // code span, a title or a destination doesn't show.
    assert_eq!(
        inlines("<b>\n`a\n# b`</b>"),
        [Inline::Strong(vec![code("a # b")])]
    );
    assert_eq!(
        inlines("<b>\n`a\n1. b`</b>"),
        [Inline::Strong(vec![code("a 1. b")])]
    );
    assert_eq!(
        inlines("<b>\n[a](u \"t\n- u\")</b>"),
        [Inline::Strong(vec![Inline::Link {
            dest: "u".into(),
            title: "t\n- u".into(),
            content: vec![text("a")],
        }])]
    );
    // Whitespace a title's entity makes is kept, in a link, an image or a
    // reference's definition.
    assert_eq!(
        inlines("<b>\n[a](u \"t&#10;    u\")</b>"),
        [Inline::Strong(vec![Inline::Link {
            dest: "u".into(),
            title: "t\n    u".into(),
            content: vec![text("a")],
        }])]
    );
    assert_eq!(
        inlines("<b>\n![a](u \"t&#10;    u\")</b>"),
        [Inline::Strong(vec![Inline::Image {
            dest: "u".into(),
            title: "t\n    u".into(),
            alt: "a".into(),
        }])]
    );
    let document = parse("<b>\n[a]</b>\n\n[a]: u \"t&#10;    u\"");
    assert_eq!(
        document.blocks[0].kind,
        BlockKind::Paragraph(vec![Inline::Strong(vec![Inline::Link {
            dest: "u".into(),
            title: "t\n    u".into(),
            content: vec![text("a")],
        }])])
    );
}
