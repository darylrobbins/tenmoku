use fe_chat::{EditBlock, EditBlockError, EditBody, parse_edit_block};

fn replace(path: Option<&str>, text: &str) -> EditBlock {
    EditBlock {
        path: path.map(str::to_owned),
        body: EditBody::Replace(text.to_owned()),
    }
}

#[test]
fn edit_blocks_parse() {
    // The path from the info string's words, after the language or alone.
    let code = "fn main() {}\n";
    let parsed = |info| parse_edit_block(info, code);
    assert_eq!(
        parsed("rust path=src/main.rs"),
        Ok(replace(Some("src/main.rs"), code))
    );
    assert_eq!(parsed("path=a.rs"), Ok(replace(Some("a.rs"), code)));
    assert_eq!(
        parsed("rust title=x path=\"my dir/a.rs\" extra"),
        Ok(replace(Some("my dir/a.rs"), code))
    );
    // Not a word of its own, or empty: no path.
    assert_eq!(parsed("rust subpath=a.rs"), Ok(replace(None, code)));
    assert_eq!(parsed("rust path="), Ok(replace(None, code)));
    assert_eq!(parsed(""), Ok(replace(None, code)));

    // Markers that aren't on lines of their own leave it a replacement.
    let lookalike = "let s = \"<<<<<<< SEARCH\";\n  =======\n";
    assert_eq!(
        parse_edit_block("rust", lookalike),
        Ok(replace(None, lookalike))
    );

    // Two sections, in order, blank lines between them.
    let code = "\
<<<<<<< SEARCH
fn a() {}
=======
fn a() -> u8 { 1 }
>>>>>>> REPLACE

<<<<<<< SEARCH
fn b() {}
fn c() {}
=======
>>>>>>> REPLACE
";
    let sections = vec![
        ("fn a() {}\n".to_owned(), "fn a() -> u8 { 1 }\n".to_owned()),
        ("fn b() {}\nfn c() {}\n".to_owned(), String::new()),
    ];
    assert_eq!(
        parse_edit_block("rust path=src/lib.rs", code),
        Ok(EditBlock {
            path: Some("src/lib.rs".to_owned()),
            body: EditBody::Sections(sections),
        })
    );
    // CRLF markers count, the last with no newline.
    let crlf = "<<<<<<< SEARCH\r\na\r\n=======\r\nb\r\n>>>>>>> REPLACE";
    let sections = vec![("a\r\n".to_owned(), "b\r\n".to_owned())];
    assert_eq!(
        parse_edit_block("", crlf).map(|block| block.body),
        Ok(EditBody::Sections(sections))
    );

    // Unbalanced: at the marker out of place, or the one never closed.
    let malformed = |code| parse_edit_block("rust path=a.rs", code);
    let at = |line| Err(EditBlockError::Malformed { line });
    assert_eq!(malformed("<<<<<<< SEARCH\na\n=======\nb\n"), at(1));
    assert_eq!(malformed("a\n=======\nb\n>>>>>>> REPLACE\n"), at(1));
    assert_eq!(malformed("\n=======\nb\n>>>>>>> REPLACE\n"), at(2));
    assert_eq!(malformed("<<<<<<< SEARCH\na\n>>>>>>> REPLACE\n"), at(3));
    assert_eq!(
        malformed("<<<<<<< SEARCH\na\n=======\nb\n=======\n>>>>>>> REPLACE\n"),
        at(5)
    );
    assert_eq!(
        malformed("<<<<<<< SEARCH\na\n<<<<<<< SEARCH\nb\n=======\n>>>>>>> REPLACE\n"),
        at(3)
    );
    let between = "<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\nstray\n";
    assert_eq!(malformed(between), at(6));
    assert_eq!(malformed("<<<<<<< SEARCH"), at(1));

    // Paths that leave the root.
    for path in [
        "/etc/passwd",
        "../outside.rs",
        "src/../../x",
        "a\\..\\b",
        "C:/x",
        "\\x",
    ] {
        assert_eq!(
            parse_edit_block(&format!("rust path={path}"), "x\n"),
            Err(EditBlockError::Path(path.to_owned())),
            "{path}"
        );
    }
    // A `..` inside a name is fine, as is `./`.
    assert_eq!(
        parse_edit_block("path=./a..b.rs", "x\n"),
        Ok(replace(Some("./a..b.rs"), "x\n"))
    );
}
