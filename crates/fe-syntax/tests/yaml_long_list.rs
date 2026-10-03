//! YAML longer than 32,768 lines used to get error nodes, and from about 100k
//! lines an ERROR root: tree-sitter-yaml 0.7.2's scanner counted rows in an
//! `int16_t`. Fixed by the patched grammar in `vendor/tree-sitter-yaml/`.

#![cfg(feature = "yaml")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

#[test]
fn a_long_yaml_list_parses_without_an_error_root() {
    // Three times the row limit, crossing the wrap at 32,768 and 98,304.
    let text: String = (0..100_000).map(|n| format!("- item {n}\n")).collect();
    let buffer = Buffer::new(&text);
    let language = language_by_name("yaml").expect("yaml is compiled in");
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());

    let root = syntax.tree().expect("the list is parsed").root_node();
    assert!(!root.is_error(), "root is {}", root.kind());
    assert!(!root.has_error(), "the tree has error nodes");
}
