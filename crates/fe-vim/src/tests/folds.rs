//! Tests of the folds feature.

use std::ops::{Range, RangeInclusive};

use fe_command::Effect;
use fe_editor::commands::{EditorHost, HeadlessHost};
use serde_json::json;

use super::commit;
use crate::folds::{FoldAction, FoldArgs, run};
use crate::{HeadlessVim, VimState, context};

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
    assert!(matches!(super::run("zQ"), crate::Step::Cancel));

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

#[test]
fn dispatch_reports_changes() {
    let mut vim = HeadlessVim::new(TEXT, (3, 4));
    let context = context(&VimState::new());
    let effect = |vim: &mut HeadlessVim, keys: &str| {
        vim.dispatch(&commit(keys).context(context.clone()))
            .unwrap()
            .effect
    };
    assert_eq!(effect(&mut vim, "zc"), Effect::Changed);
    assert_eq!(effect(&mut vim, "zo"), Effect::Changed);
    // Nothing to open: no change.
    assert_eq!(effect(&mut vim, "zo"), Effect::Nothing);
    assert_eq!(effect(&mut vim, "zR"), Effect::Nothing);
    assert_eq!(effect(&mut vim, "zM"), Effect::Changed);
    assert_eq!(effect(&mut vim, "zM"), Effect::Nothing);
}

/// A host with a syntax candidate far from the caret.
struct Elsewhere(HeadlessHost);

impl EditorHost for Elsewhere {
    fn page_rows(&self) -> u32 {
        self.0.page_rows()
    }
    fn read_clipboard(&mut self) -> Option<String> {
        None
    }
    fn write_clipboard(&mut self, _: String) {}
    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }
    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        let rows = 5..=6;
        vec![rows]
    }
}

#[test]
fn indentation_backs_up_a_syntax_candidate_elsewhere() {
    let mut vim = HeadlessVim::new(TEXT, (3, 4));
    let host = Elsewhere(HeadlessHost::new());
    for (action, expected) in [
        (FoldAction::Close, vec![(1, 3)]),
        (FoldAction::Open, vec![]),
        (FoldAction::CloseAll, vec![(0, 4), (1, 3)]),
    ] {
        run(
            vim.model_mut(),
            &host,
            FoldArgs {
                action,
                count: None,
            },
        );
        assert_eq!(folds(&vim), expected, "{action:?}");
    }
}

#[test]
fn recursive_open_reaches_every_nested_fold() {
    let text = "a\n  b\n    c\n    d\n  e\n    f\n    g\nh";
    for (keys, expected) in [("zA", vec![]), ("zO", vec![]), ("zo", vec![(1, 3), (4, 6)])] {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed("zM").unwrap();
        assert_eq!(folds(&vim), vec![(0, 6), (1, 3), (4, 6)]);
        vim.feed(keys).unwrap();
        assert_eq!(folds(&vim), expected, "{keys}");
    }
}
