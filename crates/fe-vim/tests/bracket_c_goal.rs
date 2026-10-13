//! `]c` and `[c` keep the desired column in normal mode, as nvim's do.

use fe_vim::HeadlessVim;

const ROWS: u32 = 30;

/// 30 long rows, with rows 3, 10 and 20 changed against the diff base.
fn at(row: u32, column: usize) -> HeadlessVim {
    let base = |changed: bool| {
        (0..ROWS)
            .map(|row| match changed && [3, 10, 20].contains(&row) {
                true => format!("changed {row} with a long tail of text\n"),
                false => format!("row {row} with a long tail of text\n"),
            })
            .collect::<String>()
    };
    let mut vim = HeadlessVim::new(&base(true), (row, column));
    vim.model_mut().set_diff_base(Some(base(false).into()));
    vim
}

#[test]
fn bracket_c_then_j_returns_to_the_goal() {
    let mut vim = at(0, 6);
    vim.feed("]c").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    vim.feed("j").unwrap();
    assert_eq!(vim.cursor(), (4, 6));
}

#[test]
fn bracket_c_back_then_k_returns_to_the_goal() {
    let mut vim = at(15, 6);
    vim.feed("[c").unwrap();
    assert_eq!(vim.cursor(), (10, 0));
    vim.feed("k").unwrap();
    assert_eq!(vim.cursor(), (9, 6));
}

#[test]
fn bracket_c_keeps_the_desired_column() {
    let mut text = String::new();
    for row in 0..ROWS {
        match row {
            1 => text.push_str("ab\n"),
            3 | 10 | 20 => text.push_str(&format!("changed {row} {}\n", "x".repeat(40))),
            _ => text.push_str(&format!("row {row} {}\n", "x".repeat(40))),
        }
    }
    let base = text.replace("changed", "row");
    let mut vim = HeadlessVim::new(&text, (0, 20));
    vim.model_mut().set_diff_base(Some(base.into()));
    vim.feed("j").unwrap();
    assert_eq!(vim.cursor(), (1, 1));
    vim.feed("]c").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    vim.feed("j").unwrap();
    assert_eq!(vim.cursor(), (4, 20));
}
