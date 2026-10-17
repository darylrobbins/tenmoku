use fe_markdown::{BlockKind, parse};

/// A one-block document's code, its language and the text of each span
/// highlighted `keyword`.
fn code_block(markdown: &str) -> (String, Option<&'static str>, Vec<String>) {
    let document = parse(markdown);
    let BlockKind::Code {
        code,
        language,
        highlights,
        ..
    } = &document.blocks[0].kind
    else {
        panic!("not code: {:?}", document.blocks[0]);
    };
    let keywords = highlights
        .iter()
        .filter(|span| span.highlight.name() == "keyword")
        .map(|span| code[span.range.clone()].to_owned())
        .collect();
    (code.clone(), *language, keywords)
}

#[test]
fn fenced_code_is_highlighted_by_its_fence_language() {
    let (code, language, keywords) = code_block("```rust\nfn main() { let x = 1; }\n```\n");
    assert_eq!(code, "fn main() { let x = 1; }\n");
    assert_eq!(language, Some("rust"));
    assert!(keywords.contains(&"fn".into()), "{keywords:?}");
    assert!(keywords.contains(&"let".into()), "{keywords:?}");

    let (_, language, keywords) = code_block("~~~py3\ndef f():\n    return 1\n~~~\n");
    assert_eq!(language, Some("python"));
    assert!(keywords.contains(&"def".into()), "{keywords:?}");
    assert!(keywords.contains(&"return".into()), "{keywords:?}");

    let (_, language, keywords) = code_block("```js:app.js\nconst a = 1;\n```\n");
    assert_eq!(language, Some("javascript"));
    assert!(keywords.contains(&"const".into()), "{keywords:?}");

    // Only the info string's first word names the language.
    let (_, language, _) = code_block("```Rust ignore\nfn a() {}\n```\n");
    assert_eq!(language, Some("rust"));

    // Unknown, absent and indented code get no highlights.
    for markdown in [
        "```klingon\nfn main() {}\n```\n",
        "```\nfn main() {}\n```\n",
        "    fn main() {}\n",
    ] {
        let document = parse(markdown);
        let BlockKind::Code {
            language,
            highlights,
            ..
        } = &document.blocks[0].kind
        else {
            panic!("not code");
        };
        assert_eq!(*language, None, "{markdown:?}");
        assert!(highlights.is_empty(), "{markdown:?}");
    }
}

#[test]
fn fences_keep_their_info_string() {
    let document = parse("```js:app.js\nx\n```\n\n    y\n");
    let infos: Vec<_> = document
        .blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Code { info, .. } => info.clone(),
            kind => panic!("not code: {kind:?}"),
        })
        .collect();
    assert_eq!(infos, ["js:app.js", ""]);
}

#[test]
fn highlights_line_up_with_crlf_documents_code() {
    let (code, _, keywords) = code_block("```rust\r\nlet a = 1;\r\nlet b = 2;\r\n```\r\n");
    assert_eq!(code, "let a = 1;\nlet b = 2;\n");
    assert_eq!(keywords, ["let", "let"]);
}

#[test]
fn injected_languages_are_highlighted_too() {
    // HTML's script element is JavaScript, compiled on the way if need be.
    let (_, language, keywords) = code_block("```html\n<script>const a = 1;</script>\n```\n");
    assert_eq!(language, Some("html"));
    assert!(keywords.contains(&"const".into()), "{keywords:?}");
}
