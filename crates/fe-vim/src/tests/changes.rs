//! Tests of `]c`, `[c`, `do` and `dp` (see `src/changes.rs`).

use crate::{HeadlessVim, HostCommand as H};

const ROWS: u32 = 30;

/// 30 rows, with rows 3, 10 and 20 changed against the diff base.
fn at(row: u32, column: usize) -> HeadlessVim {
    let base = |changed: bool| {
        (0..ROWS)
            .map(|row| match changed && [3, 10, 20].contains(&row) {
                true => format!("  changed {row} with a long tail\n"),
                false => format!("row {row}\n"),
            })
            .collect::<String>()
    };
    let mut vim = HeadlessVim::new(&base(true), (row, column));
    vim.model_mut().set_diff_base(Some(base(false).into()));
    vim
}

#[test]
fn bracket_c() {
    // nvim's `]c` in diff mode lands on column 0 (checked by hand with
    // `:diffthis`, from the end of a long row onto an indented one), not the
    // first non-blank.
    let mut vim = at(0, 3);
    vim.feed("]c").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    assert!(!vim.state().failed());
    let mut vim = at(0, 0);
    vim.feed("2]c").unwrap();
    assert_eq!(vim.cursor(), (10, 0));
    let mut vim = at(15, 2);
    vim.feed("[c").unwrap();
    assert_eq!(vim.cursor(), (10, 0));
    let mut vim = at(15, 0);
    vim.feed("2[c").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    // A count past the last change goes as far as there is, as nvim's does.
    let mut vim = at(0, 0);
    vim.feed("9]c").unwrap();
    assert_eq!(vim.cursor(), (20, 0));
    // No change that way: nothing moves, and it fails.
    let mut vim = at(20, 2);
    let _ = vim.feed("]c");
    assert_eq!(vim.cursor(), (20, 2));
    assert!(vim.state().failed());
    let mut vim = at(3, 2);
    let _ = vim.feed("[c");
    assert_eq!(vim.cursor(), (3, 2));
    assert!(vim.state().failed());
    // In a visual mode it extends the selection.
    let mut vim = at(0, 0);
    vim.feed("v]c").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!(vim.visual_anchor(), Some((0, 0)));
    assert_eq!(vim.cursor(), (3, 0));
}

#[test]
fn deleted_rows() {
    let mut vim = HeadlessVim::new("a\nb\nd\ne\n", (0, 0));
    vim.model_mut()
        .set_diff_base(Some("a\nb\nc\nd\ne\n".into()));
    vim.feed("]c").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
}

#[test]
fn jump_and_cancel() {
    let mut vim = at(1, 2);
    vim.feed("]c").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    vim.feed("''").unwrap();
    assert_eq!(vim.cursor().0, 1);
    for keys in ["]x", "[q", "d]c<Esc>", "]<Esc>"] {
        let mut vim = at(1, 2);
        let _ = vim.feed(keys);
        assert_eq!(vim.cursor(), (1, 2), "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(vim.text(), at(1, 2).text(), "{keys}");
        assert!(vim.state().pending().kind().is_none(), "{keys}");
    }
}

#[test]
fn do_dp() {
    for (keys, row) in [("do", 4), ("3do", 4), ("dp", 4), ("3dp", 4)] {
        let mut vim = at(row, 1);
        let text = vim.text();
        vim.feed(keys).unwrap();
        let rows = row..row + 1;
        let want = match keys.ends_with('o') {
            true => H::DiffGet { rows },
            false => H::DiffPut { rows },
        };
        assert_eq!(vim.state_mut().take_host_command(), Some(want), "{keys}");
        assert_eq!(vim.text(), text, "{keys}");
        assert_eq!(vim.cursor(), (row, 1), "{keys}");
    }
    let mut vim = HeadlessVim::new("one two\n", (0, 0));
    vim.feed("dw").unwrap();
    assert_eq!(vim.text(), "two\n");
    assert_eq!(vim.state_mut().take_host_command(), None);
}
