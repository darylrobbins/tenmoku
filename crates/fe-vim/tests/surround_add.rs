//! `ys{motion}{char}`, `yss`, `yS`, `ySS` and visual `S`, as vim-surround's
//! `doc/surround.txt` documents them (nvim has no surround, so there's no
//! oracle corpus).

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
fn surround_adds_pairs() {
    // The documented examples.
    assert_eq!(
        text_after("Hello world!", (0, 7), "ysiw)"),
        "Hello (world)!"
    );
    assert_eq!(text_after("Hello world!", (0, 7), "yssB"), "{Hello world!}");
    assert_eq!(text_after("if x>3 {", (0, 3), "ysW("), "if ( x>3 ) {");
    assert_eq!(
        text_after("\"Look ma, I'm HTML!\"", (0, 5), "cs\"<lt>q>"),
        "<q>Look ma, I'm HTML!</q>"
    );
    // A closing bracket adds no space, an opening one does, and quotes go
    // on both sides; the caret goes to the opening side.
    for (keys, after) in [
        ("ysiw)", "say (hello) now"),
        ("ysiwb", "say (hello) now"),
        ("ysiw(", "say ( hello ) now"),
        ("ysiw]", "say [hello] now"),
        ("ysiw{", "say { hello } now"),
        ("ysiw\"", "say \"hello\" now"),
        ("ysiw'", "say 'hello' now"),
        ("ysiw*", "say *hello* now"),
        ("ysiwa", "say <hello> now"),
    ] {
        let vim = run("say hello now", (0, 6), keys);
        assert_eq!((vim.text(), vim.cursor()), (after.into(), (0, 4)), "{keys}");
    }
    // Over a motion; a word's trailing blank stays outside the pair.
    assert_eq!(
        text_after("say hello now", (0, 4), "ys$\""),
        "say \"hello now\""
    );
    assert_eq!(
        text_after("say hello now", (0, 4), "ysaw)"),
        "say (hello) now"
    );
    assert_eq!(
        text_after("say hello now", (0, 0), "ys2w]"),
        "[say hello] now"
    );
    assert_eq!(text_after("f(a, b)", (0, 2), "ysi)\""), "f(\"a, b\")");
    // `yss` takes the row's text without its indent; a count, more rows.
    let vim = run("    return x", (0, 9), "yss)");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("    (return x)".into(), (0, 4))
    );
    assert_eq!(
        text_after("  one\n  two\nthree", (0, 0), "2yss]"),
        "  [one\n  two]\nthree"
    );
    // A linewise motion puts the pair on rows of its own.
    assert_eq!(
        text_after("  one\n  two\nthree", (0, 3), "ysj{"),
        "  {\n  one\n  two\n  }\nthree"
    );
    // `yS` and `ySS`: the text on its own indented rows.
    let vim = run("x foo y", (0, 3), "ySiw{");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("x {\n    foo\n} y".into(), (0, 2))
    );
    let vim = run("  call()\nnext", (0, 4), "ySS{");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("  {\n      call()\n  }\nnext".into(), (0, 2))
    );
    assert_eq!(
        text_after("  call()", (0, 4), "ySs("),
        "  (\n      call()\n  )"
    );
    // A tag: its name and attributes up to `>`, closing with its name.
    let vim = run("Hello world!", (0, 7), "ysiw<lt>em>");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("Hello <em>world</em>!".into(), (0, 6))
    );
    assert_eq!(
        text_after("Hello world!", (0, 7), "ysiwtem>"),
        "Hello <em>world</em>!"
    );
    assert_eq!(
        text_after("Hello world!", (0, 7), "ysiw<lt>a href=\"x\">"),
        "Hello <a href=\"x\">world</a>!"
    );
    assert_eq!(
        text_after("Hello world!", (0, 7), "ysiw<lt>b<CR>"),
        "Hello <b>world</b>!"
    );
    assert_eq!(
        text_after("Hello world!", (0, 7), "ysiw<lt>emx<BS>>"),
        "Hello <em>world</em>!"
    );
    assert_eq!(
        text_after("  text", (0, 3), "ySS<lt>p>"),
        "  <p>\n      text\n  </p>"
    );
    // `cs` takes a tag the same way, after `<` or `t`.
    assert_eq!(
        text_after("say \"hi\" now", (0, 5), "cs\"<lt>b>"),
        "say <b>hi</b> now"
    );
    assert_eq!(
        text_after("say \"hi\" now", (0, 5), "cs\"tb>"),
        "say <b>hi</b> now"
    );
    assert_eq!(text_after("<i>hi</i>", (0, 4), "csttb>"), "<b>hi</b>");
    // While a tag is typed the prefix waits; `<Esc>` cancels it, as does
    // `<BS>` with nothing typed or an empty tag.
    let text = "Hello world!";
    for keys in ["ysiw<lt>", "ysiwtem", "ysiw<lt>a href", "cs\"<lt>", "cs\"t"] {
        let mut vim = run(text, (0, 7), keys);
        assert_eq!(pending_kind(&vim).as_deref(), Some("surround"), "{keys}");
        vim.feed("<Esc>").unwrap();
        assert_eq!(pending_kind(&vim), None, "{keys}");
        assert_eq!(
            (vim.text(), vim.cursor(), vim.mode()),
            (text.into(), (0, 7), "normal"),
            "{keys}"
        );
    }
    for keys in [
        "ysiw<lt><BS>",
        "ysiw<lt>>",
        "ysiwx",
        "ysiw<CR>",
        "ys<Esc>",
        "ysiw<Esc>",
    ] {
        let vim = run(text, (0, 7), keys);
        assert_eq!(pending_kind(&vim), None, "{keys}");
        assert_eq!(
            (vim.text(), vim.cursor(), vim.mode()),
            (text.into(), (0, 7), "normal"),
            "{keys}"
        );
    }
    // `ys` and its target wait on the operator, then the prefix.
    let vim = run(text, (0, 7), "ys");
    assert_eq!(pending_kind(&vim).as_deref(), Some("operator"));
    let vim = run(text, (0, 7), "ysiw");
    assert_eq!(pending_kind(&vim).as_deref(), Some("surround"));
    // `.` repeats it with the same pair, and one `u` undoes each.
    let mut vim = run("one two three", (0, 0), "ysiw)");
    assert_eq!(vim.text(), "(one) two three");
    vim.feed("W.").unwrap();
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("(one) (two) three".into(), (0, 6))
    );
    vim.feed("W.").unwrap();
    assert_eq!(vim.text(), "(one) (two) (three)");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "(one) (two) three");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "(one) two three");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "one two three");
    let mut vim = run("a b", (0, 0), "ysiw<lt>em>W.");
    assert_eq!(vim.text(), "<em>a</em> <em>b</em>");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "<em>a</em> b");
    let mut vim = run("  x\ny", (0, 2), "ySS{");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "  x\ny");
}

#[test]
fn visual_s_surrounds() {
    // The documented example: charwise, as `ys` (the selection kept whole).
    let vim = run("my $str = whee!;", (0, 10), "vllllS'");
    assert_eq!(
        (vim.text(), vim.cursor(), vim.mode()),
        ("my $str = 'whee!';".into(), (0, 10), "normal")
    );
    assert_eq!(text_after("hello world", (0, 1), "viwS)"), "(hello) world");
    assert_eq!(
        text_after("hello world", (0, 1), "viwS("),
        "( hello ) world"
    );
    assert_eq!(
        text_after("hello world", (0, 0), "veeS\""),
        "\"hello world\""
    );
    assert_eq!(text_after("say hi\nnow", (0, 4), "vjS]"), "say [hi\nnow]");
    // Linewise: the pair on rows of its own, at the first row's indent.
    let vim = run("  one\n  two\nthree", (0, 3), "VjS{");
    assert_eq!(
        (vim.text(), vim.cursor(), vim.mode()),
        ("  {\n  one\n  two\n  }\nthree".into(), (0, 2), "normal")
    );
    assert_eq!(text_after("one", (0, 0), "VS{"), "{\none\n}");
    assert_eq!(
        text_after("one", (0, 0), "VS<lt>div>"),
        "<div>\none\n</div>"
    );
    // A block: each row's part.
    let vim = run("abcd\nabcd\nab\nabcd", (0, 1), "<C-v>3jlS)");
    assert_eq!(
        (vim.text(), vim.cursor(), vim.mode()),
        ("a(bc)d\na(bc)d\na(b)\na(bc)d".into(), (0, 1), "normal")
    );
    let vim = run("abcd\na\nabcd", (0, 1), "<C-v>2jS\"");
    assert_eq!(vim.text(), "a\"b\"cd\na\na\"b\"cd");
    // One `u` undoes it.
    let mut vim = run("  one\n  two", (0, 3), "VjS{");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "  one\n  two");
    // Waiting, and cancelled.
    let mut vim = run("hello world", (0, 1), "viwS");
    assert_eq!(pending_kind(&vim).as_deref(), Some("surround"));
    vim.feed("<lt>em").unwrap();
    assert_eq!(pending_kind(&vim).as_deref(), Some("surround"));
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "hello world");
    // Visual `R` (and `C`) still change the rows.
    let mut vim = run("one\ntwo\nthree", (1, 1), "VRnew<Esc>");
    assert_eq!(vim.text(), "one\nnew\nthree");
    vim.feed("ggVCfirst<Esc>").unwrap();
    assert_eq!(vim.text(), "first\nnew\nthree");
}
