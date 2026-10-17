//! `J`, `gJ`, `:j` and `:j!` carry `'<` and `'>` on a joined row with their
//! text, as nvim does. Each expectation was probed with `nvim --clean
//! --headless` (0.12.6).

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
fn joins_carry_visual_marks_with_their_text() {
    check("ab\ncd\nef", (0, 0), "vj<Esc>kJ", (0, 0), (0, 3));
    check("ab\ncd\nef", (1, 1), "v<Esc>ggJ", (0, 4), (0, 4));
    // In the indentation `J` drops: where the rows met.
    check("ab\n  cd\nef", (0, 0), "vjl<Esc>kJ", (0, 0), (0, 2));
    check("ab\ncd\nef", (0, 0), "vj<Esc>kgJ", (0, 0), (0, 2));
    // Counted, twice, and through `.`.
    check("ab\ncd\nef\ngh", (0, 0), "vjj<Esc>gg3J", (0, 0), (0, 6));
    check("ab\ncd\nef\ngh", (0, 0), "vjj<Esc>ggJJ", (0, 0), (0, 6));
    check("ab\ncd\nef\ngh", (0, 0), "vjj<Esc>ggJ.", (0, 0), (0, 6));
    // After a trailing blank, before `)`, onto an empty row.
    check("ab \ncd\nef", (0, 0), "vj<Esc>kJ", (0, 0), (0, 3));
    check("ab\n)cd\nef", (0, 0), "vjl<Esc>kJ", (0, 0), (0, 3));
    check("\ncd\nef", (0, 0), "vj<Esc>kJ", (0, 0), (0, 0));
    // Linewise `'>` stays on its row's last byte.
    check("ab\ncd\nef", (0, 0), "Vj<Esc>kJ", (0, 0), (0, 4));
    check("ab\ncd\nef", (1, 0), "Vj<Esc>kkJ", (0, 0), (1, 1));
    check("ab\ncd\nef", (0, 0), "Vj<Esc>kgJ", (0, 0), (0, 3));
}

#[test]
fn ex_joins_carry_visual_marks_with_their_text() {
    check("ab\ncd\nef", (0, 0), "vj<Esc>:1,2j<CR>", (0, 0), (0, 3));
    check("ab\ncd\nef", (0, 0), "vj<Esc>:%j<CR>", (0, 0), (0, 3));
    check("ab\ncd\nef", (0, 0), "vj<Esc>:%j!<CR>", (0, 0), (0, 2));
    check("ab\ncd\nef", (0, 0), "Vj<Esc>:1,2j<CR>", (0, 0), (0, 4));
    check(
        "ab\n  cd\nef\n)g",
        (0, 1),
        "vjjj<Esc>gg:%j!<CR>",
        (0, 1),
        (0, 9),
    );
}
