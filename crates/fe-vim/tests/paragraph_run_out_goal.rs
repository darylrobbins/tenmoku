//! A counted `}` or `{` that runs out leaves the caret where it was and sets
//! the goal column to the caret's, as nvim's do.

use fe_vim::HeadlessVim;

fn run(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let _ = vim.feed(keys);
    vim
}

#[test]
fn counted_paragraph_motions_that_run_out_set_the_goal_column_to_the_caret() {
    let normal = [
        ("abcdef\nxy", (0, 4), "j3}k", (0, 1)),
        ("xy\nabcdef", (1, 4), "k3{j", (1, 1)),
        ("abcdef\nab\n\ncd\n\nabcdef", (0, 4), "j9}k", (0, 1)),
        ("abcdef\nxyz", (0, 4), "j$3}k", (0, 2)),
        ("abcdef\n日本", (0, 4), "j3}k", (0, 2)),
        ("abcdef\nxy", (0, 4), "jd3}k", (0, 1)),
        ("xy\nabcdef", (1, 4), "kd3{j", (1, 1)),
    ];
    for (text, from, keys, want) in normal {
        let vim = run(text, from, keys);
        assert_eq!(vim.cursor(), want, "{text:?} {keys}");
        assert_eq!(vim.mode(), "normal", "{text:?} {keys}");
    }
    let visual = [
        ("abcdef\nxy\n\nabcd", (0, 4), "vj9}k", (0, 2)),
        ("xy\nabcdef", (1, 4), "vk3{j", (1, 2)),
    ];
    for (text, from, keys, want) in visual {
        let vim = run(text, from, keys);
        assert_eq!(vim.cursor(), want, "{text:?} {keys}");
        assert_eq!(vim.mode(), "visual", "{text:?} {keys}");
    }
}
