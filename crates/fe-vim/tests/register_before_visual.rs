//! A register named before `v`, `V` or `<C-v>` is dropped and the visual
//! mode starts, as in nvim: the operator that ends the selection writes
//! the registers it would with no name given. Each expectation was probed
//! with `nvim --clean --headless` (0.12.6).

use fe_vim::HeadlessVim;

const TEXT: &str = "one two\nthree four\nfive";

/// The non-empty registers among `" 0-9 - a b`, as (name, text, block
/// width).
fn registers(vim: &HeadlessVim) -> Vec<(char, String, Option<usize>)> {
    "\"0123456789-ab"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            (!register.text.is_empty()).then(|| (name, register.text.clone(), register.block_width))
        })
        .collect()
}

/// Feeds `keys` to a vim on [`TEXT`] at (0, 0) and checks the text, the
/// mode and the registers.
fn check(keys: &str, text: &str, mode: &str, want: &[(char, &str, Option<usize>)]) {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed(keys).unwrap();
    let want: Vec<(char, String, Option<usize>)> = want
        .iter()
        .map(|(name, text, width)| (*name, text.to_string(), *width))
        .collect();
    assert_eq!(
        (vim.text(), vim.mode(), registers(&vim)),
        (text.to_string(), mode, want),
        "{keys:?}"
    );
}

#[test]
fn a_register_before_v_is_dropped_and_visual_mode_starts() {
    // The mode starts, anchored at the caret.
    for (keys, mode) in [
        ("\"av", "visual"),
        ("\"aV", "visual_line"),
        ("\"a<C-v>", "visual_block"),
        ("\"a<C-q>", "visual_block"),
    ] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(
            (vim.mode(), vim.visual_anchor(), vim.text()),
            (mode, Some((0, 0)), TEXT.to_string()),
            "{keys:?}"
        );
    }
    // The operator ending the selection writes as if no register was named.
    check(
        "\"avd",
        "ne two\nthree four\nfive",
        "normal",
        &[('"', "o", None), ('-', "o", None)],
    );
    check(
        "\"aVd",
        "three four\nfive",
        "normal",
        &[('"', "one two\n", None), ('1', "one two\n", None)],
    );
    check(
        "\"avjd",
        "hree four\nfive",
        "normal",
        &[('"', "one two\nt", None), ('1', "one two\nt", None)],
    );
    check(
        "\"a<C-v>jd",
        "ne two\nhree four\nfive",
        "normal",
        &[('"', "o\nt", Some(1)), ('1', "o\nt", Some(1))],
    );
    // Even the black hole: `"_` before `v` is forgotten too.
    check(
        "\"_vd",
        "ne two\nthree four\nfive",
        "normal",
        &[('"', "o", None), ('-', "o", None)],
    );
    check(
        "\"aviwy",
        TEXT,
        "normal",
        &[('"', "one", None), ('0', "one", None)],
    );
    check(
        "\"av$y",
        TEXT,
        "normal",
        &[('"', "one two\n", None), ('0', "one two\n", None)],
    );
    // A register named inside the mode still counts.
    check(
        "\"av\"bd",
        "ne two\nthree four\nfive",
        "normal",
        &[('"', "o", None), ('b', "o", None)],
    );
    // A visual put takes the unnamed register, not the one before `v`.
    check(
        "\"ayiwwyiwb\"avep",
        "two two\nthree four\nfive",
        "normal",
        &[
            ('"', "one", None),
            ('0', "two", None),
            ('-', "one", None),
            ('a', "one", None),
        ],
    );
}
