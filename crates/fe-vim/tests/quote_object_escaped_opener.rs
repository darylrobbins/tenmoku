//! With the caret on an unescaped quote, `i"` and `a"` pair the row's quotes
//! walked from its start, and an escaped quote outside a string opens one,
//! as nvim does. Each row was probed with `nvim --clean --headless`.

use fe_vim::HeadlessVim;

/// Feeds `keys` on `buffer` at byte column `col` and checks the text and the
/// unnamed register (`None` when nothing was yanked or deleted).
fn check(buffer: &str, col: usize, keys: &str, text: &str, register: Option<&str>) {
    let mut vim = HeadlessVim::new(buffer, (0, col));
    let _ = vim.feed(keys);
    let got = vim
        .registers()
        .get('"')
        .map(|r| r.text.clone())
        .filter(|t| !t.is_empty());
    assert_eq!(
        (vim.text(), got.as_deref()),
        (text.to_string(), register),
        "{keys:?} on {buffer:?} at {col}"
    );
}

#[test]
fn a_quote_object_on_a_quote_takes_an_escaped_quote_outside_a_string_as_an_opener() {
    check(r#"\"a""#, 3, r#"di""#, r#"\"""#, Some("a"));
    check(r#"\"a\"b""#, 6, r#"di""#, r#"\"""#, Some(r#"a\"b"#));
    check(r#"a"b" \"c""#, 8, r#"di""#, r#"a"b" \"""#, Some("c"));
    check(r#""x\"y" \"z""#, 10, r#"di""#, r#""x\"y" \"""#, Some("z"));
    check(r#"\"a" x"y""#, 6, r#"di""#, r#"\"a" x"""#, Some("y"));
    check(r#"x\"a"b""#, 6, r#"di""#, r#"x\"a"b""#, None);
    check(r"\'a'", 3, "di'", r"\''", Some("a"));
    check(r"\`a`", 3, "di`", r"\``", Some("a"));
    check(r#"\"a" "#, 3, r#"da""#, r"\", Some(r#""a" "#));
    check(r#"\"a""#, 3, r#"c2i"z<Esc>"#, r"\z", Some(r#""a""#));
    // Carets not on an unescaped quote keep today's pairs.
    check(r#"\"a" x"y""#, 5, r#"di""#, r#"\"a""y""#, Some(" x"));
    check(r#"\"a""#, 1, r#"di""#, r#"\"""#, Some("a"));
    check(r#""a\""#, 3, r#"di""#, r#""a\""#, None);
}
