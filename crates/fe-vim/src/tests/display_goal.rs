//! Tests of the display column `gj` and `gk` keep with a caret's goal (see
//! `src/line_motions.rs`).

use crate::HeadlessVim;

/// "xxxxxxxxx y" then a tab: wrapped at 10, the tab starts one column into
/// its display row, where it takes 3 columns but only 1 in the buffer.
const TEXT: &str = "xxxxxxxxx y\twwwwwwwwwwwwwwwwwwwwwwww\nzzz";

fn after(cursor: (u32, usize), keys: &str) -> (u32, usize) {
    let mut vim = HeadlessVim::new(TEXT, cursor);
    vim.model_mut().set_soft_wrap(true);
    vim.model_mut().set_wrap_width(10.0);
    vim.feed(keys).unwrap();
    vim.cursor()
}

#[test]
fn gj_gk_return_inside_a_wide_wrapped_tab() {
    // Two columns into the tab on its display row: back there, not at its
    // last column's buffer neighbour.
    assert_eq!(after((0, 23), "2gk2gj"), (0, 23));
    assert_eq!(after((0, 24), "2gk2gj"), (0, 24));
    assert_eq!(after((0, 13), "gkgj"), (0, 13));
    assert_eq!(after((0, 1), "gjgk"), (0, 1));
    assert_eq!(after((0, 2), "gjgk"), (0, 2));
}

#[test]
fn j_and_k_and_horizontal_motions_drop_the_display_column() {
    // After `gk` into the tab, `h` then `l` come back to the same caret
    // with the buffer goal reset, so `gj` measures from the caret itself.
    let (tab, back) = (after((0, 23), "2gk"), after((0, 23), "2gkhl"));
    assert_eq!(tab, back);
    let from_goal = after((0, 23), "2gk2gj");
    let from_caret = after((0, 23), "2gkhl2gj");
    assert_eq!(from_goal, (0, 23));
    assert_ne!(from_caret, from_goal);
}

fn wrapped(text: &str, cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.model_mut().set_soft_wrap(true);
    vim.model_mut().set_wrap_width(10.0);
    vim
}

#[test]
fn visual_modes_keep_the_display_column() {
    for keys in ["v2gk2gj", "V2gk2gj", "<C-v>2gk2gj"] {
        let mut vim = wrapped(TEXT, (0, 23));
        vim.feed(keys).unwrap();
        assert_eq!(vim.cursor(), (0, 23), "{keys}");
    }
}

#[test]
fn a_blocked_gj_or_gk_keeps_the_display_column() {
    let text = "xxxxxxxxx y\t";
    // At the bottom row `gj` fails, then `gk` still returns into the tab.
    assert_eq!(after_in(text, (0, 1), "gjgjgk"), (0, 1));
    assert_eq!(after_in(text, (0, 1), "gkgjgk"), (0, 1));
}

fn after_in(text: &str, cursor: (u32, usize), keys: &str) -> (u32, usize) {
    let mut vim = wrapped(text, cursor);
    vim.feed(keys).unwrap();
    vim.cursor()
}

#[test]
fn a_scroll_edit_or_visual_toggle_between_drops_the_display_column() {
    // Each of these returns to the tab with the buffer goal set afresh, so
    // the `2gj` measures from the caret itself, as after `h` and `l`.
    let fresh = after_in(TEXT, (0, 23), "2gkhl2gj");
    assert_ne!(fresh, (0, 23));
    for keys in [
        "2gk<C-d><C-u>2gj",
        "2gkvhlv2gj",
        "2gkvhl<Esc>2gj",
        "2gkjk2gj",
    ] {
        assert_eq!(after_in(TEXT, (0, 23), keys), fresh, "{keys}");
    }
    // An edit away from the tab and back keeps nothing of it either.
    let edited = after_in(TEXT, (0, 23), "2gkix<Esc>x2gj");
    assert_eq!(edited, after_in(TEXT, (0, 23), "2gkix<Esc>xhl2gj"));
}

#[test]
fn a_column_clamped_by_the_wrap_width_is_not_revived() {
    let text = format!("{}\nxx\nxx\n{}", "a".repeat(25), "b".repeat(100));
    let direct = after_in(&text, (0, 0), "99|gjgjj");
    let plain = {
        let mut vim = wrapped(&text, (0, 0));
        vim.feed("99|gjgj").unwrap();
        vim.feed("j").unwrap();
        vim.cursor()
    };
    assert_eq!(direct, plain);
    assert!(direct.1 < 20, "{direct:?}");
}
