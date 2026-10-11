//! Tests of the increment feature.
use crate::HeadlessVim;

/// `keys` over `text` from `cursor`: the text and the caret.
fn run(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

#[test]
fn numbers() {
    assert_eq!(run("x 9 y", (0, 0), "<C-a>"), ("x 10 y".into(), (0, 3)));
    assert_eq!(run("-3", (0, 0), "5<C-x>"), ("-8".into(), (0, 1)));
    assert_eq!(run("0xff", (0, 0), "<C-a>").0, "0x100");
    assert_eq!(run("0xFE", (0, 0), "<C-a>").0, "0xFF");
    assert_eq!(run("0b101", (0, 0), "<C-a>").0, "0b110");
    assert_eq!(run("007", (0, 0), "<C-a>").0, "008");
    assert_eq!(run("a 1 2", (0, 4), "<C-x>").0, "a 1 1");
    assert_eq!(run("x 9 y", (0, 0), "<C-a>.").0, "x 11 y");
    assert_eq!(run("1", (0, 0), "2<C-a>.").0, "5");

    // 64 bits: the count is capped as nvim's is, a wrap keeps the sign.
    let max = "18446744073709551615";
    assert_eq!(run(max, (0, 0), "<C-x>").0, "18446744073709551614");
    assert_eq!(run(max, (0, 0), "<C-a>").0, format!("-{max}"));
    assert_eq!(
        run(max, (0, 0), "4294967295<C-a>").0,
        "-18446744072709551617"
    );

    // No number: nothing changes, and no undo step is left.
    let mut vim = HeadlessVim::new("5\nx", (0, 0));
    vim.feed("<C-a>j<C-a>u").unwrap();
    assert_eq!(vim.text(), "5\nx");
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.text(), "6\nx");

    // Visual: the first number on each row, and progressively with `g`.
    assert_eq!(run("1\n2\n3", (0, 0), "Vjj<C-a>").0, "2\n3\n4");
    assert_eq!(run("1\n1\n1", (0, 0), "Vjjg<C-a>").0, "2\n3\n4");
    assert_eq!(run("5\n5\n5", (0, 0), "Vjj2g<C-x>").0, "3\n1\n-1");
    let mut vim = HeadlessVim::new("1\n1", (0, 0));
    vim.feed("Vj<C-a>").unwrap();
    assert_eq!(vim.mode(), "normal");
}
