//! `cx{motion}`, `cxx` and `cxc` as vim-exchange documents them (its
//! README and `doc/exchange.txt`). nvim has no exchange operator to record
//! an oracle against: each case's expectation is worked out from that
//! documentation.

use fe_editor::CommentTokens;
use fe_vim::HeadlessVim;

/// Feeds `keys` to a vim on `text` at `cursor`.
fn after(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    vim
}

/// Asserts the text and caret `keys` leave.
#[track_caller]
fn check(text: &str, cursor: (u32, usize), keys: &str, expected: &str, caret: (u32, usize)) {
    let vim = after(text, cursor, keys);
    assert_eq!(vim.text(), expected, "{keys:?} on {text:?}");
    assert_eq!(vim.cursor(), caret, "{keys:?} on {text:?}: the caret");
    assert_eq!(vim.mode(), "normal", "{keys:?} on {text:?}: the mode");
}

#[test]
fn exchange_swaps_regions() {
    // The first `cx` changes nothing, the caret going to `'[` as `g@`'s.
    let vim = after("a bb ccc", (0, 3), "cxiw");
    assert_eq!(vim.text(), "a bb ccc");
    assert_eq!(vim.cursor(), (0, 2));
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((0, 2)), Some((0, 3))));
    // The second swaps: the caret goes to the start of what fills its own
    // region.
    check("a bb ccc", (0, 0), "cxiwwwcxiw", "ccc bb a", (0, 7));
    check("a bb ccc", (0, 5), "cxiw0cxiw", "ccc bb a", (0, 0));
    // A count, and a motion's.
    check(
        "one two three four",
        (0, 0),
        "cx2ewwcxe",
        "three one two four",
        (0, 6),
    );

    // `cxx` twice swaps rows, as `cc` takes them (`count` of them).
    check(
        "one\ntwo\nthree",
        (0, 1),
        "cxxjjcxx",
        "three\ntwo\none",
        (2, 0),
    );
    check(
        "one\ntwo\nthree",
        (0, 0),
        "2cxxGcxx",
        "three\none\ntwo",
        (1, 0),
    );
    check(
        "one\ntwo\nthree",
        (0, 0),
        "cxjGcxx",
        "three\none\ntwo",
        (1, 0),
    );
    // Rows take the indentation of the rows they replace.
    check(
        "    one\n      deep\ntwo",
        (0, 0),
        "cxjGcxx",
        "    two\none\n  deep",
        (1, 0),
    );

    // A charwise region with a linewise one: each text takes the kind of
    // the region it goes into.
    check("x y\none\ntwo", (0, 2), "cxiwjcxj", "x one\ntwo\ny", (2, 0));
    check(
        "x y\none\ntwo",
        (1, 0),
        "cxjggwcxiw",
        "x one\ntwo\ny",
        (0, 2),
    );

    // A contained region replaces its container, whichever comes first.
    check("foo bar baz\nnext", (0, 0), "cxxwcxiw", "bar\nnext", (0, 0));
    check("foo bar baz\nnext", (0, 4), "cxiwcxx", "bar\nnext", (0, 0));
    check("(a (b) c)", (0, 4), "cxibcxa(", "(a b c)", (0, 3));
    // The same region twice changes nothing.
    check("foo bar", (0, 4), "cxiwcxiw", "foo bar", (0, 4));

    // Overlapping regions fail and clear the mark: the next `cx` marks.
    let mut vim = after("one two three", (0, 0), "cx2ewcx2e");
    assert_eq!(vim.text(), "one two three");
    assert_eq!(
        vim.state().message(),
        Some("Exchange aborted: overlapping text")
    );
    vim.feed("cxiw").unwrap();
    assert_eq!(vim.text(), "one two three", "the mark was cleared");
    vim.feed("0cxiw").unwrap();
    assert_eq!(vim.text(), "two one three");

    // `cxc` clears it.
    let mut vim = after("a b c", (0, 0), "cxiwcxc");
    vim.feed("wcxiw").unwrap();
    assert_eq!(vim.text(), "a b c", "the mark was cleared");
    vim.feed("wcxiw").unwrap();
    assert_eq!(vim.text(), "a c b");
    // With nothing marked it does nothing.
    check("a b", (0, 0), "cxcwcxiw0cxiw", "b a", (0, 0));

    // `.` repeats `cx`: it marks, then swaps.
    check("a b c", (0, 0), "cxiww.", "b a c", (0, 2));
    check("a b c d", (0, 0), "cxiww.w.w.", "b a d c", (0, 6));
    check(
        "one\ntwo\nthree",
        (0, 0),
        "cxxj.",
        "two\none\nthree",
        (1, 0),
    );

    // One `u` undoes the swap, both halves.
    let mut vim = after("a bb ccc", (0, 0), "cxiwwwcxiw");
    assert_eq!(vim.text(), "ccc bb a");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "a bb ccc");
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.text(), "ccc bb a");

    // The marked region follows an edit made between the two `cx`, text
    // typed at its edge staying out of it.
    check("foo bar", (0, 0), "cxiwiXX<Esc>$cxiw", "XXbar foo", (0, 6));
    check("foo bar", (0, 0), "cxiwea!<Esc>wcxiw", "bar! foo", (0, 5));
    check(
        "foo bar",
        (0, 4),
        "cxiw0iXX <Esc>wcxiw",
        "XX bar foo",
        (0, 3),
    );
    check(
        "one\ntwo\nthree",
        (1, 0),
        "cxxggOzero<Esc>Gcxx",
        "zero\none\nthree\ntwo",
        (3, 0),
    );
}

#[test]
fn marked_empty_rows_keep_their_place() {
    // A row put in above an empty marked row (`O`) or below it (`o`)
    // leaves the mark on that row.
    check("\nfoo", (0, 0), "cxxOnew<Esc>Gcxx", "new\nfoo\n", (2, 0));
    check("\nfoo", (0, 0), "cxxonew<Esc>Gcxx", "foo\nnew\n", (2, 0));
    // Rows ending on an empty one: a row put in above it is among them; one
    // put in below it isn't.
    check("a\n\nb", (0, 0), "cxjjOx<Esc>Gcxx", "b\na\nx\n", (1, 0));
    check("a\n\nb", (0, 0), "cxjjox<Esc>Gcxx", "b\nx\na\n", (2, 0));
    // Rows starting on an empty one: a row put in above it isn't.
    check("\na\nb", (0, 0), "cxjOnew<Esc>Gcxx", "new\nb\n\na", (2, 0));
    // Blank rows put in, as marks follow them.
    check("\nfoo", (0, 0), "cxxO<Esc>Gcxx", "\nfoo\n", (2, 0));
    check("\nfoo", (0, 0), "cxxo<Esc>Gcxx", "foo\n\n", (2, 0));
    check("a\n\nb", (0, 0), "cxjjO<Esc>Gcxx", "b\na\n\n", (1, 0));
    check("a\n\nb", (0, 0), "cxjjo<Esc>Gcxx", "b\n\na\n", (2, 0));
    // One above, then one below.
    check("\nfoo", (0, 0), "cxxO<Esc>jo<Esc>Gcxx", "\nfoo\n\n", (3, 0));
    // Rows of multibyte text.
    check("\nfoo", (0, 0), "cxxOé<Esc>Gcxx", "é\nfoo\n", (2, 0));
    check("\nfoo", (0, 0), "cxxoé<Esc>Gcxx", "foo\né\n", (2, 0));
    check("é\n\nfoo", (0, 0), "cxjjOü<Esc>Gcxx", "foo\né\nü\n", (1, 0));
    // Deleting a marked row leaves the others; deleting all of them clears
    // the mark, the next `cx` marking anew.
    check("a\nb\nc\nd", (0, 0), "cxjjddGcxx", "d\nc\na", (2, 0));
    check("a\nb\nc\nd", (0, 0), "cxjddGcxx", "d\nc\nb", (2, 0));
    check("a\nb\nc", (0, 0), "cxxddcxxGcxx", "c\nb", (1, 0));
}

#[test]
fn a_failed_exchange_leaves_dot() {
    // The overlapping `cx` fails, and `.` still repeats `rO`.
    let mut vim = after("one two three", (0, 0), "cx2erOwcx2e");
    assert_eq!(vim.text(), "One two three");
    assert_eq!(
        vim.state().message(),
        Some("Exchange aborted: overlapping text")
    );
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "One Owo three");
}

/// A vim on `text` at `cursor` with `#` line comments.
fn commented(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.set_comment_tokens(Some(CommentTokens {
        line: Some("#"),
        block: None,
    }));
    vim.feed(keys).unwrap();
    vim
}

#[test]
fn exchange_takes_the_comment_object() {
    // `cxgc` marks the run of commented rows, leaving no visual mode.
    let vim = commented("x\n# a\n# b\nc\nd", (2, 2), "cxgc");
    assert_eq!(vim.text(), "x\n# a\n# b\nc\nd");
    assert_eq!((vim.cursor(), vim.mode()), ((1, 0), "normal"));
    // And swaps with it, either way round.
    let vim = commented("# a\n# b\nc\nd", (0, 0), "cxgcGcxx");
    assert_eq!(vim.text(), "d\nc\n# a\n# b");
    assert_eq!((vim.cursor(), vim.mode()), ((2, 0), "normal"));
    let vim = commented("# a\n# b\nc\nd", (3, 0), "cxxggcxgc");
    assert_eq!(vim.text(), "d\nc\n# a\n# b");
    assert_eq!((vim.cursor(), vim.mode()), ((0, 0), "normal"));
    // `.` repeats it over the run at the caret.
    let vim = commented("# a\nx\n# b\ny", (0, 0), "cxgcjj.");
    assert_eq!(vim.text(), "# b\nx\n# a\ny");
    assert_eq!((vim.cursor(), vim.mode()), ((2, 0), "normal"));
}
