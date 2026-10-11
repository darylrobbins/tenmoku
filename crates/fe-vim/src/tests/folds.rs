//! Tests of the folds feature.

use serde_json::json;

use super::commit;
use crate::HeadlessVim;

const TEXT: &str = "a\n  b\n    c\n    d\n  e\nf\n  g\nh";

/// The folds as `(header, last)` rows.
fn folds(vim: &HeadlessVim) -> Vec<(u32, u32)> {
    let mut folds: Vec<_> = vim
        .model()
        .display()
        .folds()
        .iter()
        .map(|fold| (fold.start, fold.end))
        .collect();
    folds.sort();
    folds
}

#[test]
fn z_keys_drive_the_model() {
    for (keys, action) in [
        ("za", "toggle"),
        ("zc", "close"),
        ("zo", "open"),
        ("zA", "toggle_all"),
        ("zC", "close_all"),
        ("zO", "open_all"),
        ("zR", "open_every"),
        ("zM", "close_every"),
        ("zv", "reveal"),
    ] {
        let fold = commit(keys);
        assert_eq!(fold.command.as_str(), "vim.fold", "{keys}");
        assert_eq!(fold.args, json!({"action": action}), "{keys}");
    }
    assert_eq!(commit("2zo").args, json!({"action": "open", "count": 2}));
    assert!(matches!(
        crate::step(crate::Pending::new(), crate::parse_keys("z").unwrap()[0]),
        crate::Step::Wait(_)
    ));
    assert!(matches!(super::run("zz"), crate::Step::Cancel));

    let registry = crate::registry();
    let command = registry.get(&"vim.fold".parse().unwrap()).unwrap();
    assert!(!command.mutates_buffer);

    // Candidates are the indented blocks 0..=4, 1..=3 and 5..=6.
    let mut vim = HeadlessVim::new(TEXT, (3, 4));
    let before = vim.text();
    let version = vim.model().buffer().version();

    // `zc` closes the innermost block around the row, `zo` opens it.
    vim.feed("zc").unwrap();
    assert_eq!(folds(&vim), vec![(1, 3)]);
    vim.feed("zo").unwrap();
    assert_eq!(folds(&vim), vec![]);

    // `za` toggles.
    vim.feed("za").unwrap();
    assert_eq!(folds(&vim), vec![(1, 3)]);
    vim.feed("za").unwrap();
    assert_eq!(folds(&vim), vec![]);

    // `zM` closes every candidate, `zR` opens them.
    vim.feed("zM").unwrap();
    assert_eq!(folds(&vim), vec![(0, 4), (1, 3), (5, 6)]);
    vim.feed("zR").unwrap();
    assert_eq!(folds(&vim), vec![]);

    // `2zc` from the inner block's header closes both levels, the caret
    // moving to the outer header; `zo` opens one level, `2zo` two.
    vim.feed("ggj2zc").unwrap();
    assert_eq!(folds(&vim), vec![(0, 4), (1, 3)]);
    vim.feed("zo").unwrap();
    assert_eq!(folds(&vim), vec![(1, 3)]);
    vim.feed("zc").unwrap();
    assert_eq!(folds(&vim), vec![(0, 4), (1, 3)]);
    vim.feed("2zo").unwrap();
    assert_eq!(folds(&vim), vec![]);

    // `zC` and `zO` act on every level at the row.
    vim.feed("jzC").unwrap();
    assert_eq!(folds(&vim), vec![(0, 4), (1, 3)]);
    vim.feed("zO").unwrap();
    assert_eq!(folds(&vim), vec![]);

    // `zv` opens only what hides the cursor: a fold the caret heads stays.
    vim.feed("jzc").unwrap();
    vim.feed("zv").unwrap();
    assert_eq!(folds(&vim), vec![(1, 3)]);

    // A caret put inside the fold, as another origin's selection might,
    // has `zv` open it.
    let inside = vim.model().text().row_start(2);
    vim.model_mut().set_cursor(inside);
    if vim.cursor().0 == 2 {
        vim.feed("zv").unwrap();
        assert_eq!(folds(&vim), vec![]);
    }

    assert_eq!(vim.text(), before);
    assert_eq!(vim.model().buffer().version(), version);
}
