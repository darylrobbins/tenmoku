//! fe-vim's commenting (`gc{motion}`, `gcc`, visual `gc` and the `gc`
//! object) against recorded Neovim: `tests/corpus/commentary.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes.
//! Every case sets 'commentstring' to `# %s`, which the harness gives
//! [`HeadlessVim`] as the line token `#`; where fastedit's toggle differs
//! from nvim's, `src/feature/comment.rs` lists it and the corpus leaves it
//! out.

#[allow(dead_code)]
mod support;

use fe_editor::CommentTokens;
use fe_vim::HeadlessVim;
use support::{check, load};

/// `text` at `cursor`, its host giving `#` as the line token, as the
/// corpus's 'commentstring' does.
fn commenting(text: &str, cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.set_comment_tokens(Some(CommentTokens {
        line: Some("#"),
        block: None,
    }));
    vim
}

#[test]
fn commentary_matches_neovim() {
    let recorded = load("commentary").unwrap();
    assert!(recorded.len() >= 16, "commentary: {} cases", recorded.len());
    for case in &recorded {
        assert_eq!(
            case.set.as_deref(),
            Some("commentstring=#\\ %s"),
            "{}",
            case.name
        );
    }
    // What the card asks the corpus to cover, by case name.
    for name in [
        "gcc",
        "gcc_count",
        "gcc_uncomments",
        "gc_j",
        "gc_ip",
        "gc_visual_rows",
        "gc_visual_charwise",
        "gc_mixed_rows_comment",
        "gc_indented_rows",
        "gc_dot_repeat",
        "gc_undo",
        "dgc_object",
        "ygc_object",
        "cgc_object",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "commentary: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "commentary: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// With no comment tokens (no language), `gcc` and `gcip` change nothing
/// and leave no undo step: `u` then undoes the change before them.
#[test]
fn comment_keys_without_tokens_do_nothing() {
    const TEXT: &str = "one\ntwo\n\nthree";
    for keys in ["gcc", "gcip", "3gcc", "Vjgc", "dgc"] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed("x").unwrap();
        let before = (vim.text(), vim.cursor(), vim.model().snapshot().version());
        vim.feed(keys).unwrap();
        assert_eq!(
            (vim.text(), vim.cursor(), vim.model().snapshot().version()),
            before,
            "{keys}"
        );
        assert_eq!(vim.mode(), "normal", "{keys}");
        vim.feed("u").unwrap();
        assert_eq!(vim.text(), TEXT, "{keys}: u undoes the x");
    }
}

/// `gc` over an empty object (`i"` in `""`, `i(` in `()`, `it` in
/// `<p></p>`) changes nothing and leaves no undo step, as nvim's leaves it;
/// `gc0` at column 0 still comments its row.
#[test]
fn comment_over_an_empty_object_does_nothing() {
    for (text, cursor, keys, landed) in [
        ("x = \"\";\ny", (0, 5), "gci\"", (0, 5)),
        ("x = ();\ny", (0, 5), "gci(", (0, 5)),
        ("<p></p>\ny", (0, 1), "gcit", (0, 3)),
    ] {
        let mut vim = commenting(text, (1, 0));
        vim.feed("x").unwrap();
        vim.feed(&format!("{}G{}|", cursor.0 + 1, cursor.1 + 1))
            .unwrap();
        let version = vim.model().snapshot().version();
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), text.replace("\ny", "\n"), "{keys}");
        assert_eq!(vim.model().snapshot().version(), version, "{keys}");
        assert_eq!(vim.cursor(), landed, "{keys}");
        vim.feed("u").unwrap();
        assert_eq!(vim.text(), text, "{keys}: u undoes the x");
    }
    let mut vim = commenting("foo", (0, 0));
    vim.feed("gc0").unwrap();
    assert_eq!(vim.text(), "# foo");
}

/// `zf` over the `gc` object closes a fold over the commented run, as any
/// operator over a selection does, and changes nothing.
#[test]
fn fold_over_the_comment_object() {
    let mut vim = commenting("x\n# a\n# b\ny", (1, 0));
    vim.feed("zfgc").unwrap();
    let folds: Vec<_> = (vim.model().display().folds().iter())
        .map(|fold| (fold.start, fold.end))
        .collect();
    assert_eq!(folds, vec![(1, 2)]);
    assert_eq!(vim.text(), "x\n# a\n# b\ny");
    assert_eq!((vim.cursor(), vim.mode()), ((1, 0), "normal"));
}

/// `gc` puts `'[` and `']` where nvim's does (probed with nvim 0.12.4,
/// which the oracle doesn't record): at its target's start and end, kept at
/// their rows and byte columns through the toggle.
#[test]
fn comment_marks_match_neovim() {
    for (text, cursor, keys, start, end) in [
        ("foo bar", (0, 1), "gcc", (0, 0), (0, 1)),
        ("  foo bar", (0, 6), "gcc", (0, 2), (0, 6)),
        ("  # foo bar", (0, 8), "gcc", (0, 2), (0, 8)),
        ("a\nbb\nccc\nd", (0, 0), "3gcc", (0, 0), (2, 0)),
        ("one\ntwo\nthree", (0, 1), "gcj", (0, 1), (1, 1)),
        ("one\ntwo\nthree", (2, 3), "gck", (1, 2), (2, 3)),
        ("a\nbb\nccc\n\nd", (1, 1), "gcip", (0, 0), (2, 0)),
        ("one two\nthree", (0, 4), "gcw", (0, 4), (0, 6)),
        ("one two", (0, 0), "gce", (0, 0), (0, 2)),
        ("one two", (0, 2), "gc$", (0, 2), (0, 6)),
        ("foo", (0, 2), "gc0", (0, 0), (0, 1)),
        ("a\nbbb\nccc", (1, 2), "gcG", (1, 2), (2, 2)),
        ("aaa\nbbb\nccc", (2, 2), "gcgg", (0, 2), (2, 2)),
        ("aaa\nbbb\nccc", (0, 2), "majjgc'a", (0, 0), (2, 2)),
        ("aaa\nbbb\nccc", (0, 2), "majjgc`a", (0, 2), (2, 1)),
        ("foo\nbar baz", (0, 1), "gccj$.", (1, 0), (1, 6)),
        // Over a selection.
        ("one\ntwo\nthree", (0, 1), "Vjgc", (0, 0), (1, 1)),
        ("one\ntwo\nthree", (2, 2), "Vkgc", (1, 2), (2, 0)),
        ("one", (0, 2), "Vgc", (0, 0), (0, 2)),
        ("one\ntwo\nthree", (0, 1), "vjlgc", (0, 1), (1, 2)),
        ("one\ntwo\nthree", (0, 1), "<C-v>jjlgc", (0, 1), (2, 2)),
    ] {
        let mut vim = commenting(text, cursor);
        vim.feed(keys).unwrap();
        assert_eq!(
            (vim.mark('['), vim.mark(']')),
            (Some(start), Some(end)),
            "{keys} on {text:?}"
        );
    }
}
