//! `>{N}|` and `<{N}|` leave the caret at display column N-1 of the shifted
//! row, clamped to it, as nvim does.

use fe_vim::HeadlessVim;

fn run(text: &str, from: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, from);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

#[test]
fn shift_to_a_column_leaves_the_caret_there_as_nvim_does() {
    #[allow(clippy::type_complexity)]
    let cases: &[(&str, (u32, usize), &str, &str, (u32, usize))] = &[
        ("abcdefghij\n", (0, 0), ">5|", "    abcdefghij\n", (0, 4)),
        ("abcdefghij\n", (0, 8), ">5|", "    abcdefghij\n", (0, 4)),
        ("abcdefghij\n", (0, 2), ">20|", "    abcdefghij\n", (0, 13)),
        ("abc\n", (0, 0), ">9|", "    abc\n", (0, 6)),
        ("  abcdefgh\n", (0, 0), ">6|", "      abcdefgh\n", (0, 5)),
        ("  abcdefgh\n", (0, 7), "<3|", "abcdefgh\n", (0, 2)),
        ("abc\ndef\n", (0, 1), ">3|", "    abc\ndef\n", (0, 2)),
        ("\tab\tcd\n", (0, 0), ">6|", "        ab\tcd\n", (0, 5)),
        ("abcdefghij\n", (0, 3), ">|", "    abcdefghij\n", (0, 0)),
        ("abcdefghij\n", (0, 3), ">1|", "    abcdefghij\n", (0, 0)),
        ("abcdefghij\n", (0, 3), ">4|", "    abcdefghij\n", (0, 3)),
        ("abcdefghij\n", (0, 6), ">3|", "    abcdefghij\n", (0, 2)),
        ("abcdefghij\n", (0, 6), "<3|", "abcdefghij\n", (0, 2)),
        (
            "abcdefghij\nabcdefghij\n",
            (0, 2),
            "2>3|",
            "    abcdefghij\nabcdefghij\n",
            (0, 5),
        ),
        (
            "abcdefghij\nabcdefghijklmnopqrstuvwxyz\n",
            (0, 2),
            ">20|j",
            "    abcdefghij\nabcdefghijklmnopqrstuvwxyz\n",
            (1, 13),
        ),
        (
            "abc def ghi\nxyz\n",
            (0, 2),
            ">2|j",
            "    abc def ghi\nxyz\n",
            (1, 1),
        ),
        ("abc def ghi\n", (0, 1), ">w", "    abc def ghi\n", (0, 1)),
        ("abcdefghij\n", (0, 2), ">3|u", "abcdefghij\n", (0, 2)),
        (
            "abcdefghij\n",
            (0, 2),
            ">3|.",
            "        abcdefghij\n",
            (0, 2),
        ),
    ];
    for (text, from, keys, want, caret) in cases {
        assert_eq!(
            run(text, *from, keys),
            (want.to_string(), *caret),
            "{keys:?} on {text:?} at {from:?}"
        );
    }
}
