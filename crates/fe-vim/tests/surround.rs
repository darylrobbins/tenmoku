//! `ds{char}` and `cs{old}{new}`, as vim-surround's `doc/surround.txt`
//! documents them (nvim has no surround, so there's no oracle corpus).

use fe_buffer::ResolvedSelection;
use fe_command::ContextFrame;
use fe_vim::HeadlessVim;

fn run(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let _ = vim.feed(keys);
    vim
}

fn text_after(text: &str, cursor: (u32, usize), keys: &str) -> String {
    run(text, cursor, keys).text()
}

fn pending_kind(vim: &HeadlessVim) -> Option<String> {
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    frame.get("vim.pending_kind").map(str::to_string)
}

#[test]
fn surround_deletes_pairs() {
    // The documented examples.
    assert_eq!(
        text_after("\"Hello world!\"", (0, 7), "ds\""),
        "Hello world!"
    );
    assert_eq!(text_after("(123+456)/2", (0, 5), "ds)"), "123+456/2");
    assert_eq!(text_after("<div>Yo!</div>", (0, 7), "dst"), "Yo!");
    // The caret goes to where the pair began.
    let vim = run("say \"Hello world!\" now", (0, 11), "ds\"");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("say Hello world! now".into(), (0, 4))
    );
    // Each bracket and its alias; the opening one trims the blanks inside.
    for (keys, after) in [
        ("ds(", "f(x) a, b y"),
        ("ds)", "f(x)  a, b  y"),
        ("dsb", "f(x)  a, b  y"),
    ] {
        assert_eq!(text_after("f(x) ( a, b ) y", (0, 8), keys), after, "{keys}");
    }
    for (keys, after) in [("ds{", "a b"), ("ds}", " a b "), ("dsB", " a b ")] {
        assert_eq!(text_after("{ a b }", (0, 2), keys), after, "{keys}");
    }
    for (keys, after) in [("ds[", "x[1]"), ("ds]", "x[ 1 ]"), ("dsr", "x[ 1 ]")] {
        assert_eq!(text_after("x[[ 1 ]]", (0, 4), keys), after, "{keys}");
    }
    for (keys, after) in [("ds<", "Vecu8"), ("ds>", "Vecu8"), ("dsa", "Vecu8")] {
        assert_eq!(text_after("Vec<u8>", (0, 4), keys), after, "{keys}");
    }
    assert_eq!(text_after("x = 'one two';", (0, 7), "ds'"), "x = one two;");
    assert_eq!(
        text_after("run `ls -l` now", (0, 6), "ds`"),
        "run ls -l now"
    );
    // A tag around rows keeps its contents.
    assert_eq!(
        text_after("<p>\n  <b>bold</b> text\n</p>", (1, 7), "dst"),
        "<p>\n  bold text\n</p>"
    );
    assert_eq!(
        text_after("<p>\n  <b>bold</b> text\n</p>", (1, 7), "2dst"),
        "\n  <b>bold</b> text\n"
    );
    // A count takes the count-th enclosing pair.
    assert_eq!(text_after("((a) b)", (0, 2), "2ds("), "(a) b");
    assert_eq!(text_after("((a) b)", (0, 2), "d2s)"), "(a) b");
    assert_eq!(text_after("((a) b)", (0, 2), "ds("), "(a b)");
    // A pair over rows.
    assert_eq!(
        text_after("call(\n  one,\n  two\n)", (1, 3), "ds)"),
        "call\n  one,\n  two\n"
    );
    // No pair around the caret: nothing changes.
    for (text, keys) in [
        ("plain text", "ds("),
        ("plain text", "ds\""),
        ("(a) b", "ds)"),
        ("(a)", "2ds("),
        ("no tag", "dst"),
    ] {
        let at = (0, text.len() - 1);
        let vim = run(text, at, keys);
        assert_eq!(
            (vim.text(), vim.cursor()),
            (text.into(), at),
            "{keys} on {text}"
        );
    }
}

#[test]
fn surround_changes_pairs() {
    // The documented examples.
    assert_eq!(
        text_after("\"Hello world!\"", (0, 7), "cs\"'"),
        "'Hello world!'"
    );
    assert_eq!(
        text_after("'Hello world!'", (0, 7), "cs'("),
        "( Hello world! )"
    );
    assert_eq!(
        text_after("'Hello world!'", (0, 7), "cs')"),
        "(Hello world!)"
    );
    assert_eq!(text_after("(123+456)/2", (0, 5), "cs)]"), "[123+456]/2");
    assert_eq!(text_after("(123+456)/2", (0, 5), "cs)["), "[ 123+456 ]/2");
    assert_eq!(text_after("[123+456]/2", (0, 5), "cs]{"), "{ 123+456 }/2");
    assert_eq!(text_after("<div>Yo!</div>", (0, 7), "cst\""), "\"Yo!\"");
    // An opening bracket as {old} trims; any other punctuation as {new}
    // goes on both sides; an alias as {new} doesn't add spaces.
    assert_eq!(text_after("( a )", (0, 2), "cs(]"), "[a]");
    assert_eq!(text_after("f(x)", (0, 2), "cs)*"), "f*x*");
    assert_eq!(text_after("f(x)", (0, 2), "csbB"), "f{x}");
    assert_eq!(text_after("f(x)", (0, 2), "csba"), "f<x>");
    // `.` repeats it on the next pair.
    let mut vim = run("\"a\" and \"b\"", (0, 1), "cs\"'");
    assert_eq!((vim.text(), vim.cursor()), ("'a' and \"b\"".into(), (0, 0)));
    vim.feed("fb.").unwrap();
    assert_eq!((vim.text(), vim.cursor()), ("'a' and 'b'".into(), (0, 8)));
    // One `u` undoes a change, and one more the change before.
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "'a' and \"b\"");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "\"a\" and \"b\"");
    let mut vim = run("x [ 1 ]", (0, 4), "ds[");
    assert_eq!(vim.text(), "x 1");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "x [ 1 ]");
    // Two carets each change their pair, one undo step for both.
    let text = "f(a) + g(b) + h";
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.model_mut()
        .set_selections([2, 9].map(|offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
    vim.feed("cs)]").unwrap();
    assert_eq!(vim.text(), "f[a] + g[b] + h");
    let heads: Vec<usize> = vim.model().selections().iter().map(|s| s.head()).collect();
    assert_eq!(heads, [1, 8]);
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), text);
    // A caret with no pair leaves the others to change.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.model_mut()
        .set_selections([2, 14].map(|offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
    vim.feed("ds)").unwrap();
    assert_eq!(vim.text(), "fa + g(b) + h");
    // Two carets in one pair, or in pairs sharing a quote, change only the
    // first.
    for (text, offsets, keys, after) in [
        ("f(ab) x", [2, 3], "cs)]", "f[ab] x"),
        ("\"a\"b\"", [1, 3], "cs\"'", "'a'b\""),
    ] {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.model_mut()
            .set_selections(offsets.map(|offset| ResolvedSelection {
                range: offset..offset,
                reversed: false,
            }));
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), after, "{keys} on {text}");
    }
}

#[test]
fn feature_prefix_waits_and_cancels() {
    let text = "say \"hi\" (x)";
    for keys in ["ds", "cs", "cs\"", "2ds", "cs\"<lt>", "cs\"t"] {
        let mut vim = run(text, (0, 6), keys);
        assert_eq!(pending_kind(&vim).as_deref(), Some("surround"), "{keys}");
        vim.feed("<Esc>").unwrap();
        assert_eq!(pending_kind(&vim), None, "{keys}");
        assert_eq!(
            (vim.text(), vim.cursor(), vim.mode()),
            (text.into(), (0, 6), "normal")
        );
    }
    // A char naming no pair, or no replacement, cancels.
    for keys in ["dsx", "ds1", "csx", "cs\"x", "ds<CR>"] {
        let vim = run(text, (0, 6), keys);
        assert_eq!(pending_kind(&vim), None, "{keys}");
        assert_eq!(
            (vim.text(), vim.cursor(), vim.mode()),
            (text.into(), (0, 6), "normal"),
            "{keys}"
        );
    }
    // After the cancel the next key is read afresh.
    let vim = run(text, (0, 6), "dsxx");
    assert_eq!(vim.text(), "say \"h\" (x)");
    // `s` after `y` starts surround's operator, not its prefix
    // (`surround_add.rs`).
    let vim = run(text, (0, 6), "ys");
    assert_eq!(pending_kind(&vim).as_deref(), Some("operator"));
}
