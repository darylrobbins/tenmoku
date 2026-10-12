use fe_syntax::{CommentTokens, comment_tokens, language_names};

fn get(name: &str) -> CommentTokens {
    comment_tokens(name).unwrap_or_else(|| panic!("no tokens for {name}"))
}

#[test]
fn every_listed_language_has_tokens() {
    for name in language_names().chain(["markdown_inline"]) {
        assert!(comment_tokens(name).is_some(), "{name}");
    }
    assert_eq!(comment_tokens("no-such-language"), None);
}

#[test]
fn values() {
    let rust = get("rust");
    assert_eq!((rust.line, rust.block), (Some("//"), Some(("/*", "*/"))));
    let python = get("python");
    assert_eq!((python.line, python.block), (Some("#"), None));
    let lua = get("lua");
    assert_eq!((lua.line, lua.block), (Some("--"), Some(("--[[", "]]"))));
    let sql = get("sql");
    assert_eq!((sql.line, sql.block), (Some("--"), Some(("/*", "*/"))));
    let css = get("css");
    assert_eq!((css.line, css.block), (None, Some(("/*", "*/"))));
    let html = get("html");
    assert_eq!((html.line, html.block), (None, Some(("<!--", "-->"))));
    let json = get("json");
    assert_eq!((json.line, json.block), (None, None));
}
