use fe_markdown::{Block, BlockKind, Document, Inline, parse};

const FIXTURE: &str = "\
# Title

A paragraph
over two lines.

- one
- two
  - nested
  - again
- three

```rust
fn main() {}
```

| a | b |
|---|---|
| 1 | 2 |
| 3 | 4 |

> quoted
> still quoted
";

fn kinds(document: &Document) -> Vec<&'static str> {
    document
        .blocks
        .iter()
        .map(|block| match block.kind {
            BlockKind::Paragraph(_) => "paragraph",
            BlockKind::Heading { .. } => "heading",
            BlockKind::Quote(_) => "quote",
            BlockKind::List { .. } => "list",
            BlockKind::Code { .. } => "code",
            BlockKind::Table { .. } => "table",
            BlockKind::Rule => "rule",
            BlockKind::Html(_) => "html",
        })
        .collect()
}

fn check_lines(text: &str) {
    let document = parse(text);
    assert_eq!(
        kinds(&document),
        ["heading", "paragraph", "list", "code", "table", "quote"]
    );
    let lines: Vec<_> = document
        .blocks
        .iter()
        .map(|block| block.lines.clone())
        .collect();
    assert_eq!(lines, [0..1, 2..4, 5..10, 11..14, 15..19, 20..22]);

    let BlockKind::List { items, tight, .. } = &document.blocks[2].kind else {
        panic!("not a list");
    };
    assert!(tight);
    let item_lines: Vec<_> = items.iter().map(|item| item.lines.clone()).collect();
    assert_eq!(item_lines, [5..6, 6..9, 9..10]);
    // The second item's text, then its nested list.
    let nested = &items[1].blocks;
    assert_eq!(nested.len(), 2);
    assert_eq!(nested[0].lines, 6..7);
    let BlockKind::List { items: nested, .. } = &nested[1].kind else {
        panic!("not a nested list");
    };
    let nested_lines: Vec<_> = nested.iter().map(|item| item.lines.clone()).collect();
    assert_eq!(nested_lines, [7..8, 8..9]);

    let BlockKind::Table { rows, .. } = &document.blocks[4].kind else {
        panic!("not a table");
    };
    let row_lines: Vec<_> = rows.iter().map(|row| row.lines.clone()).collect();
    assert_eq!(row_lines, [17..18, 18..19]);

    let BlockKind::Quote(quoted) = &document.blocks[5].kind else {
        panic!("not a quote");
    };
    assert_eq!(quoted[0].lines, 20..22);
}

#[test]
fn blocks_carry_their_source_lines() {
    check_lines(FIXTURE);
    check_lines(&FIXTURE.replace('\n', "\r\n"));
    // Without a final line ending, the quote still ends on its last row.
    check_lines(FIXTURE.trim_end());
}

#[test]
fn code_in_crlf_documents_has_lf_line_endings() {
    let document = parse("```\r\na\r\nb\r\n```\r\n");
    let BlockKind::Code { code, .. } = &document.blocks[0].kind else {
        panic!("not code");
    };
    assert_eq!(code, "a\nb\n");
}

#[test]
fn loose_lists_and_blank_lines_between_blocks() {
    let document = parse("- a\n\n- b\n\n\n\npara\n");
    let BlockKind::List { items, tight, .. } = &document.blocks[0].kind else {
        panic!("not a list");
    };
    assert!(!tight);
    assert_eq!(items[0].lines, 0..1);
    assert_eq!(items[1].lines, 2..3);
    assert_eq!(document.blocks[0].lines, 0..3);
    assert_eq!(document.blocks[1].lines, 6..7);
}

#[test]
fn setext_headings_rules_and_html_span_their_rows() {
    let document = parse("Title\n=====\n\n---\n\n<div>\nhi\n</div>\n");
    assert_eq!(document.blocks[0].lines, 0..2);
    assert_eq!(
        document.blocks[1],
        Block {
            kind: BlockKind::Rule,
            lines: 3..4
        }
    );
    assert_eq!(
        document.blocks[2],
        Block {
            kind: BlockKind::Html("<div>\nhi\n</div>\n".into()),
            lines: 5..8
        }
    );
}

fn slugs(text: &str) -> Vec<String> {
    parse(text)
        .headings()
        .map(|(slug, _)| slug.to_owned())
        .collect()
}

#[test]
fn headings_get_github_slugs() {
    assert_eq!(
        slugs("# Hello, World!\n\n## Hello, World!\n\n### C++ & Rust\n"),
        ["hello-world", "hello-world-1", "c--rust"]
    );
    // Markup is left out, and whitespace is kept one `-` each.
    assert_eq!(
        slugs("# `Code` *and* [link](x)  here\n\n# Ünïcode_ok-yes\n"),
        ["code-and-link--here", "ünïcode_ok-yes"]
    );
    // Headings in quotes and lists count, in document order.
    assert_eq!(slugs("> # A\n\n- # A\n\n# A\n"), ["a", "a-1", "a-2"]);
}

#[test]
fn anchors_find_their_heading_as_written_then_lowercased() {
    let document = parse("# Intro\n\ntext\n\n## Usage Notes\n");
    let row = |slug: &str| document.heading(slug).map(|block| block.lines.start);
    assert_eq!(row("intro"), Some(0));
    assert_eq!(row("usage-notes"), Some(4));
    assert_eq!(row("Usage-Notes"), Some(4));
    assert_eq!(row("missing"), None);
}

#[test]
fn heading_content_keeps_its_inlines() {
    let document = parse("## A *b*\n");
    let BlockKind::Heading { level, content, .. } = &document.blocks[0].kind else {
        panic!("not a heading");
    };
    assert_eq!(*level, 2);
    assert_eq!(
        content,
        &[
            Inline::Text("A ".into()),
            Inline::Emphasis(vec![Inline::Text("b".into())])
        ]
    );
}

#[test]
fn documents_can_be_sent_to_another_thread() {
    fn send<T: Send + 'static>() {}
    send::<Document>();
    let document = std::thread::spawn(|| parse("# a\n")).join().unwrap();
    assert_eq!(document.blocks.len(), 1);
}

#[test]
fn code_and_html_keep_their_blank_last_rows() {
    for newline in ["\n", "\r\n"] {
        let text = |markdown: &str| markdown.replace('\n', newline);
        // An unclosed fence runs to the end, blank rows and all.
        let document = parse(&text("```\na\n\n\n"));
        assert_eq!(document.blocks[0].lines, 0..4, "{newline:?}");
        let BlockKind::Code { code, .. } = &document.blocks[0].kind else {
            panic!("not code");
        };
        assert_eq!(code, "a\n\n\n");
        // So does unclosed preformatted HTML.
        let document = parse(&text("<pre>\na\n\n\n"));
        assert_eq!(document.blocks[0].lines, 0..4, "{newline:?}");
        // A closed fence ends at its fence, and blank rows after it aren't its.
        let document = parse(&text("```\na\n\n```\n\n\npara\n"));
        assert_eq!(document.blocks[0].lines, 0..4, "{newline:?}");
        assert_eq!(document.blocks[1].lines, 6..7, "{newline:?}");
        // Nor are an indented block's.
        let document = parse(&text("    a\n\n\npara\n"));
        assert_eq!(document.blocks[0].lines, 0..1, "{newline:?}");
        // A list item and quote end with the code they end with.
        let document = parse(&text("- a\n\n  ```\n  b\n\n\n"));
        let BlockKind::List { items, .. } = &document.blocks[0].kind else {
            panic!("not a list");
        };
        assert_eq!(items[0].blocks[1].lines, 2..6, "{newline:?}");
        assert_eq!(items[0].lines, 0..6, "{newline:?}");
        assert_eq!(document.blocks[0].lines, 0..6, "{newline:?}");
    }
}

/// How deep quotes, list items and inlines nest.
fn depth(blocks: &[Block]) -> usize {
    fn inline_depth(inlines: &[Inline]) -> usize {
        inlines
            .iter()
            .map(|inline| match inline {
                Inline::Emphasis(content)
                | Inline::Strong(content)
                | Inline::Strikethrough(content)
                | Inline::Link { content, .. } => 1 + inline_depth(content),
                _ => 1,
            })
            .max()
            .unwrap_or(0)
    }
    blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Quote(blocks) => 1 + depth(blocks),
            // An item and its list are one level.
            BlockKind::List { items, .. } => {
                1 + items
                    .iter()
                    .map(|item| depth(&item.blocks))
                    .max()
                    .unwrap_or(0)
            }
            BlockKind::Paragraph(content) | BlockKind::Heading { content, .. } => {
                1 + inline_depth(content)
            }
            _ => 1,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn deep_nesting_is_cut_short_and_its_content_kept() {
    let n = 20_000;
    let documents = [
        format!("{}deep\n", ">".repeat(n)),
        format!("{}deep\n", "> - ".repeat(n)),
        format!("{}deep\n", "1. ".repeat(n)),
        format!("{}deep{}\n", "*".repeat(n), "*".repeat(n)),
        format!("{}deep{}\n", "~~*".repeat(n), "*~~".repeat(n)),
        format!("{}deep{}\n", "![".repeat(n), "](x)".repeat(n)),
        format!("{}deep{}\n", "> *".repeat(n / 2), "*".repeat(n / 2)),
    ];
    for markdown in documents {
        // On a thread with the usual 2 MiB stack, as the preview parses.
        let thread = std::thread::Builder::new().stack_size(2 << 20);
        let (depth, text) = thread
            .spawn(move || {
                let document = parse(&markdown);
                (depth(&document.blocks), format!("{document:?}"))
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(depth <= 70, "{depth}");
        assert!(text.contains("deep"), "lost the content");
    }
}
