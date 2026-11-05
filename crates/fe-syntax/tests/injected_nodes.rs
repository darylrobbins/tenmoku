//! The token boundaries of injected layers, for callers that need a whole
//! comment or string of an injected language.

#![cfg(all(feature = "html", feature = "javascript"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `text` as HTML, with the layers of `range` highlighted and compiled.
fn highlighted(text: &str, range: std::ops::Range<usize>) -> Syntax {
    let mut syntax = Syntax::new(
        language_by_name("html").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    loop {
        syntax.highlights(range.clone());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            return syntax;
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

#[test]
fn injected_nodes_give_the_tokens_of_the_layer() {
    let text = "<p>x</p>\n<script>\nlet a = 'one two';\n// remark here\n</script>\n";
    let syntax = highlighted(text, 0..text.len());
    let nodes = syntax.injected_nodes(0..text.len());
    let string = nodes
        .iter()
        .find(|node| node.kind == "string")
        .expect("a string node");
    assert_eq!(string.language, "javascript");
    assert_eq!(&text[string.range.clone()], "'one two'");
    assert!(string.named);
    let comment = nodes.iter().find(|node| node.kind == "comment").unwrap();
    assert_eq!(&text[comment.range.clone()], "// remark here");
    // Children name their parent in the list.
    let fragment = nodes
        .iter()
        .find(|node| node.kind == "string_fragment")
        .unwrap();
    let parent = &nodes[fragment.parent.unwrap()];
    assert_eq!(parent.kind, "string");
    assert_eq!(&text[fragment.range.clone()], "one two");

    // Only those overlapping the range, and none for the host's own tokens.
    let at = text.find("one").unwrap();
    let near = syntax.injected_nodes(at..at + 1);
    assert!(near.iter().all(|n| n.range.start <= at && n.range.end > at));
    assert!(near.iter().any(|node| node.kind == "string"));
    assert!(near.iter().all(|node| node.language == "javascript"));
    assert!(syntax.injected_nodes(0..3).is_empty());
}
