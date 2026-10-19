//! `ia aa`: a function argument, as VSCodeVim's argument object gives it.

use std::time::Instant;

use fe_vim::HeadlessVim;
use fe_vim::search::DEFAULT_TIME_LIMIT;

fn run(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let _ = vim.feed(keys);
    vim
}

fn text_after(text: &str, cursor: (u32, usize), keys: &str) -> String {
    run(text, cursor, keys).text()
}

/// The text a visual selection from `keys` covers, by yanking it.
fn selected(text: &str, cursor: (u32, usize), keys: &str) -> String {
    let vim = run(text, cursor, &format!("{keys}y"));
    vim.registers()
        .get('"')
        .map(|r| r.text.clone())
        .unwrap_or_default()
}

#[test]
fn argument_objects() {
    let call = "f(aa, bb, cc)";
    // The first, a middle and the last argument.
    assert_eq!(text_after(call, (0, 3), "ciaX<Esc>"), "f(X, bb, cc)");
    assert_eq!(text_after(call, (0, 3), "daa"), "f(bb, cc)");
    assert_eq!(text_after(call, (0, 7), "ciaX<Esc>"), "f(aa, X, cc)");
    assert_eq!(text_after(call, (0, 7), "daa"), "f(aa, cc)");
    assert_eq!(text_after(call, (0, 11), "ciaX<Esc>"), "f(aa, bb, X)");
    assert_eq!(text_after(call, (0, 11), "daa"), "f(aa, bb)");
    // On a comma or the white space after it: the next argument.
    assert_eq!(text_after(call, (0, 4), "daa"), "f(aa, cc)");
    assert_eq!(text_after(call, (0, 5), "dia"), "f(aa, , cc)");
    // An only argument takes itself.
    assert_eq!(text_after("f( only )", (0, 4), "ciaX<Esc>"), "f( X )");
    assert_eq!(text_after("f( only )", (0, 4), "daa"), "f(  )");
    // Inside a nested call, and the nested call as an argument (from its
    // name or its brackets).
    let nested = "f(a, g(b, c), d)";
    assert_eq!(text_after(nested, (0, 10), "daa"), "f(a, g(b), d)");
    assert_eq!(text_after(nested, (0, 7), "ciaX<Esc>"), "f(a, g(X, c), d)");
    assert_eq!(text_after(nested, (0, 5), "daa"), "f(a, d)");
    assert_eq!(text_after(nested, (0, 6), "ciaX<Esc>"), "f(a, X, d)");
    assert_eq!(text_after(nested, (0, 11), "ciaX<Esc>"), "f(a, X, d)");
    // A string holding a comma (and an escaped quote, and a bracket).
    let strings = r#"f("a, \"(b", 'c, d', e)"#;
    assert_eq!(text_after(strings, (0, 4), "daa"), "f('c, d', e)");
    assert_eq!(
        text_after(strings, (0, 14), "ciaX<Esc>"),
        r#"f("a, \"(b", X, e)"#
    );
    assert_eq!(
        text_after(strings, (0, 21), "daa"),
        r#"f("a, \"(b", 'c, d')"#
    );
    // Over a multi-row list, with a trailing comma.
    let rows = "f(\n    one,\n    two(x, y),\n    three,\n)\n";
    assert_eq!(
        text_after(rows, (1, 5), "daa"),
        "f(\n    two(x, y),\n    three,\n)\n"
    );
    assert_eq!(
        text_after(rows, (2, 4), "ciaX<Esc>"),
        "f(\n    one,\n    X,\n    three,\n)\n"
    );
    assert_eq!(
        text_after(rows, (3, 6), "daa"),
        "f(\n    one,\n    two(x, y),\n)\n"
    );
    // On the trailing comma: the last argument.
    assert_eq!(
        text_after(rows, (3, 9), "ciaX<Esc>"),
        "f(\n    one,\n    two(x, y),\n    X,\n)\n"
    );
    // Without one, the last argument takes the comma before it.
    assert_eq!(text_after("f(\n  a,\n  b\n)", (2, 2), "daa"), "f(\n  a\n)");
    // An only argument with a trailing comma takes it too.
    assert_eq!(text_after("f(a,)", (0, 2), "daa"), "f()");
    assert_eq!(text_after("f(a, b,)", (0, 5), "daa"), "f(a,)");
    // In brackets and braces.
    assert_eq!(text_after("[1, 2, 3]", (0, 4), "daa"), "[1, 3]");
    assert_eq!(
        text_after("{ a: 1, b: 2 }", (0, 8), "ciaX<Esc>"),
        "{ a: 1, X }"
    );
    assert_eq!(text_after("{ a: 1, b: 2 }", (0, 3), "daa"), "{ b: 2 }");
    // A count takes that many arguments from the cursor's.
    let four = "f(a, b, c, d)";
    assert_eq!(text_after(four, (0, 5), "2daa"), "f(a, d)");
    assert_eq!(text_after(four, (0, 5), "c2iaX<Esc>"), "f(a, X, d)");
    assert_eq!(text_after(four, (0, 8), "d2aa"), "f(a, b)");
    assert_eq!(text_after(four, (0, 2), "4daa"), "f()");
    // Past the last argument, or outside any list: nothing.
    assert_eq!(text_after(four, (0, 8), "3daa"), four);
    assert_eq!(text_after("a, b", (0, 3), "dia"), "a, b");
    assert_eq!(text_after("f(a, b]", (0, 2), "dia"), "f(a, b]");
    // `.` repeats.
    assert_eq!(text_after(four, (0, 2), "daa."), "f(c, d)");
    // In a visual mode: the argument, and again, the next one too.
    let wide = "f(aa, bb, cc, dd)";
    assert_eq!(selected(wide, (0, 6), "via"), "bb");
    assert_eq!(selected(wide, (0, 6), "viaia"), "bb, cc");
    assert_eq!(selected(wide, (0, 6), "viaiaia"), "bb, cc, dd");
    assert_eq!(selected(wide, (0, 6), "vaa"), "bb, ");
    assert_eq!(selected(wide, (0, 6), "vaaaa"), "bb, cc, ");
    assert_eq!(selected(wide, (0, 6), "vaaaaaa"), ", bb, cc, dd");
    assert_eq!(selected(wide, (0, 6), "v2ia"), "bb, cc");
    assert_eq!(selected(wide, (0, 6), "via2ia"), "bb, cc, dd");
    // From a fresh `v` on a one-letter argument: that argument; again, the
    // next one too.
    assert_eq!(selected(four, (0, 5), "via"), "b");
    assert_eq!(selected(four, (0, 5), "viaia"), "b, c");
    assert_eq!(selected(four, (0, 5), "via2ia"), "b, c, d");
    assert_eq!(selected(four, (0, 5), "vaaaa"), "b, c, ");
    // From a fresh `v` on a comma or white space: the next argument.
    assert_eq!(selected(wide, (0, 4), "via"), "bb");
    assert_eq!(selected(wide, (0, 4), "vaa"), "bb, ");
    assert_eq!(selected(wide, (0, 5), "via"), "bb");
    let vim = run(wide, (0, 6), "via");
    assert_eq!(
        (vim.mode(), vim.visual_anchor(), vim.cursor()),
        ("visual", Some((0, 6)), (0, 7))
    );
    // Past the last argument, the selection stays.
    let vim = run(wide, (0, 14), "viaia");
    assert_eq!(
        (vim.visual_anchor(), vim.cursor()),
        (Some((0, 14)), (0, 15))
    );
    let vim = run(wide, (0, 6), "vaaaaaaaa");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 4)), (0, 15)));
    // Over rows.
    assert_eq!(selected(rows, (1, 4), "viaia"), "one,\n    two(x, y)");
    assert_eq!(selected(rows, (1, 4), "vaa"), "one,\n    ");
    assert_eq!(selected(rows, (2, 0), "via"), "two(x, y)");
}

#[test]
fn argument_objects_skip_strings_over_rows() {
    // A backtick string, and a string with an escaped line break, span rows.
    assert_eq!(text_after("f(`a\n)b`, c)", (0, 3), "ciaX<Esc>"), "f(X, c)");
    assert_eq!(
        text_after("f(\"a\\\n), b\", c)", (0, 3), "ciaX<Esc>"),
        "f(X, c)"
    );
    // An apostrophe in prose ends with its row.
    assert_eq!(
        text_after("// don't\nf(a, b)", (1, 2), "daa"),
        "// don't\nf(b)"
    );
}

#[test]
fn argument_object_unclosed_list_is_bounded() {
    let mut text = String::from("f(a, b\n");
    text.push_str(&"x, y\n".repeat(100_000));
    let mut vim = HeadlessVim::new(&text, (0, 2));
    let started = Instant::now();
    let _ = vim.feed("dia");
    let took = started.elapsed();
    assert!(took < DEFAULT_TIME_LIMIT, "{took:?}");
    assert_eq!(vim.text(), text);
}

#[test]
fn argument_object_long_rows_are_bounded() {
    // An unclosed list on one long row.
    let text = format!("f(a, b{}", "x, ".repeat(100_000));
    let mut vim = HeadlessVim::new(&text, (0, 2));
    let started = Instant::now();
    let _ = vim.feed("dia");
    let took = started.elapsed();
    assert!(took < DEFAULT_TIME_LIMIT, "{took:?}");
    assert_eq!(vim.text(), text);
    // Growing a selection of many arguments on one row: matching it to the
    // arguments doesn't rebuild the row for each one.
    let text = format!("f({}aa)", "aa, ".repeat(4_000));
    let mut vim = HeadlessVim::new(&text, (0, 2));
    let started = Instant::now();
    let _ = vim.feed("v3600iaia");
    let took = started.elapsed();
    assert!(took < DEFAULT_TIME_LIMIT, "{took:?}");
    assert_eq!(
        (vim.visual_anchor(), vim.cursor()),
        (Some((0, 2)), (0, 2 + 4 * 3600 + 1))
    );
}
