#![cfg(all(
    feature = "go",
    feature = "bash",
    feature = "markdown",
    feature = "rust"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, fold_ranges, language_by_name};

fn parse(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax
}

#[test]
fn go_window_starting_on_a_blank_row_keeps_its_folds() {
    let go = parse(
        "go",
        "package p\n\nfunc f() {\n\ta()\n\n\tb()\n}\n\nfunc g() {\n\tc()\n}\n",
    );
    let all = fold_ranges(&go, 0..11);
    assert!(!all.is_empty());
    assert_eq!(fold_ranges(&go, 1..11), all);
    let second = fold_ranges(&go, 8..11);
    assert!(!second.is_empty());
    assert_eq!(fold_ranges(&go, 7..11), second);
}

#[test]
fn bash_and_markdown_windows_starting_on_a_blank_row_keep_their_folds() {
    for (language, text) in [
        ("bash", "f() {\n  echo a\n}\n\ng() {\n  echo b\n}\n"),
        ("markdown", "# A\n\ntext\n\n## B\n\nmore\n"),
    ] {
        let syntax = parse(language, text);
        let from_header = fold_ranges(&syntax, 4..7);
        assert!(!from_header.is_empty(), "{language}");
        assert_eq!(fold_ranges(&syntax, 3..7), from_header, "{language}");
    }
}

#[test]
fn rust_window_starting_on_a_blank_row_keeps_its_folds() {
    let rust = parse("rust", "fn f() {\n    a();\n}\n\nfn g() {\n    b();\n}\n");
    let from_header = fold_ranges(&rust, 4..7);
    assert!(!from_header.is_empty());
    assert_eq!(fold_ranges(&rust, 3..7), from_header);
}
