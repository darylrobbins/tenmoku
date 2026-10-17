//! `~` over graphemes without a case changes no text but is still an undo
//! step (and a repeatable change), as in nvim: `u` puts the caret back where
//! the `~` started.

use fe_vim::HeadlessVim;

const BUFFER: &str = "ab.c 12\nxy 日本";

fn run(cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(BUFFER, cursor);
    vim.feed(keys).unwrap();
    vim
}

#[test]
fn tilde_over_characters_without_case_is_an_undo_step() {
    // Rows from nvim --clean --headless: (keys, caret after).
    let rows = [
        ("ll~u", (0, 2)),
        ("ll~jju", (0, 2)),
        ("jll~ku", (1, 2)),
        ("5|~ku", (0, 4)),
        ("jllll~ku", (1, 6)),
        ("ll~h.jju", (0, 2)),
        // Unchanged: a count, a grapheme with a case, r, visual and g~.
        ("ll2~jju", (0, 2)),
        ("l~jju", (0, 1)),
        ("rajju", (0, 0)),
        ("llv~jju", (0, 2)),
        ("llg~ljju", (0, 2)),
    ];
    for (keys, caret) in rows {
        let vim = run((0, 0), keys);
        assert_eq!(vim.cursor(), caret, "{keys}");
    }
    assert_eq!(run((0, 0), "ll~u").text(), BUFFER);
}
