//! Visual `o` and `O` onto a tab before the anchor aim at its first column.

use fe_vim::HeadlessVim;

fn run(text: &str, at: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, at);
    vim.feed(keys).unwrap();
    vim
}

#[test]
fn visual_swap_onto_a_tab_aims_at_its_first_column() {
    let rows = [
        ("xyzw\n\ta", (1, 0), "vlOk", (0, 0)),
        ("xyzw\n\ta", (1, 0), "vlok", (0, 0)),
        ("xyzw\n\ta", (1, 1), "vhoOk", (0, 0)),
        ("xyzw\n\ta\nxyzw", (1, 0), "Vlok", (0, 0)),
        ("xyzw\n\ta\nxyzw", (1, 0), "<C-v>lok", (0, 0)),
        ("xyzw\n\ta\nxyzw", (1, 0), "<C-v>jlok", (0, 0)),
        ("xyzwvu\n\t\ta", (1, 1), "vlOk", (0, 4)),
        ("xyzwvu\n\t\ta", (1, 1), "vhOk", (0, 6)),
    ];
    for (text, at, keys, want) in rows {
        assert_eq!(run(text, at, keys).cursor(), want, "{keys} on {text:?}");
    }
    let vim = run("xyzw\n\ta", (1, 0), "vlOD");
    assert_eq!(vim.cursor(), (0, 0));
    assert_eq!(vim.text(), "xyzw");
}
