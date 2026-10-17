use fe_syntax::comment_tokens;

#[test]
fn json_and_python_comment_tokens_match_vscode() {
    let json = comment_tokens("json").unwrap();
    assert_eq!((json.line, json.block), (Some("//"), Some(("/*", "*/"))));
    let python = comment_tokens("python").unwrap();
    assert_eq!(
        (python.line, python.block),
        (Some("#"), Some(("\"\"\"", "\"\"\"")))
    );
}
