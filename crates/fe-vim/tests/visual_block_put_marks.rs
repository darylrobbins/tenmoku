//! A visual put of a blockwise register sets `'<` at the put's first byte and
//! `'>` at the last byte put on its last row, as nvim does. Each expectation
//! was probed with `nvim --clean --headless` (0.12.6).

use fe_vim::HeadlessVim;

type Pos = (u32, usize);

fn check(text: &str, cursor: Pos, keys: &str, start: Pos, end: Pos) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    assert_eq!(
        (vim.mark('<'), vim.mark('>')),
        (Some(start), Some(end)),
        "{keys:?} on {text:?} from {cursor:?}"
    );
}

#[test]
fn visual_put_of_a_block_sets_the_visual_marks_as_nvim() {
    let mixed = "héllo wörld ünï\n日本語 テスト 12\nabc";
    check(mixed, (0, 0), "<C-v>jly0viwp", (0, 0), (1, 5));
    check(mixed, (0, 0), "<C-v>jlyjvep", (1, 0), (2, 5));
    check(mixed, (0, 0), "<C-v>jlyjviwP", (1, 0), (2, 5));
    let wide = "日本語\nテスト\nabc\ndef";
    check(wide, (0, 0), "<C-v>jlyGvp", (3, 0), (4, 5));
    check(wide, (0, 0), "<C-v>jly2jviwp", (2, 0), (3, 5));
    let ragged = "ab日本cd\nüü\n\nxyz w";
    check(ragged, (2, 0), "<C-v>2jyjwv$p", (3, 4), (4, 4));
    check(ragged, (0, 2), "<C-v>jlyGvp", (3, 2), (4, 1));
    check(ragged, (1, 0), "<C-v>jlyjvep", (2, 0), (3, 0));
    check("ab\ncd\nef", (0, 0), "<C-v>jlyGvp", (2, 0), (3, 1));
}
