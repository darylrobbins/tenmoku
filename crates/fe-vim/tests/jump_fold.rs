//! A jump with a fold in view: the letters shown after `{c1}` are the
//! labels after `{c2}`, over the rows the view shows, the fold one row.

use fe_vim::{HeadlessVim, jump::JumpLabelState};

/// A row with no `ab`, then 30 of two `ab`s, in a view 10 rows tall with
/// rows 3 to 11 folded behind row 2.
fn vim() -> HeadlessVim {
    let rows = std::iter::once("x").chain(std::iter::repeat_n("ab ab", 30));
    let text = rows.collect::<Vec<_>>().join("\n");
    let mut vim = HeadlessVim::new(&text, (0, 0));
    vim.set_window_rows(10);
    vim.model_mut().fold_rows(2..=11);
    vim
}

/// The overlay's labels in `state`: row, column and text.
fn labels(vim: &HeadlessVim, state: JumpLabelState) -> Vec<(u32, u32, String)> {
    let text = vim.model().text();
    (vim.jump_overlay().unwrap().labels.into_iter())
        .filter(|label| label.state == state)
        .map(|label| {
            let at = text.offset_to_point(label.offset);
            (at.row, at.column, label.text)
        })
        .collect()
}

#[test]
fn preview_letters_are_labels_with_a_fold() {
    let mut vim = vim();
    vim.feed("gsa").unwrap();
    let direct = labels(&vim, JumpLabelState::Direct);
    let letters = labels(&vim, JumpLabelState::Preview);
    // Rows 1, 2 and, past the fold, 12 to 18: 18 targets, which autojump.
    assert_eq!(direct, [(1, 0, String::new())]);
    let rows: Vec<u32> = letters.iter().map(|&(row, ..)| row).collect();
    let mut want = vec![1, 2, 2];
    want.extend((12..19).flat_map(|row| [row, row]));
    assert_eq!(rows, want);

    vim.feed("b").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(labels(&vim, JumpLabelState::Live), letters);
    let (row, column, letter) = letters.last().unwrap().clone();
    vim.feed(&letter).unwrap();
    assert_eq!(vim.cursor(), (row, column as usize));
}
