//! A counted word object over one visual position that runs past the text's
//! end selects to it, as nvim does, and fails as an error does.

use fe_vim::HeadlessVim;

type Pos = (u32, usize);

/// (buffer, cursor, keys, anchor, cursor after, mode), from `nvim --clean --headless`.
const ROWS: &[(&str, Pos, &str, Pos, Pos, &str)] = &[
    ("x y z", (0, 2), "v3aw", (0, 2), (0, 5), "visual"),
    ("x y z", (0, 3), "v2aw", (0, 3), (0, 5), "visual"),
    ("x  y  z", (0, 2), "v3aw", (0, 1), (0, 7), "visual"),
    ("ab cd", (0, 4), "v2aw", (0, 3), (0, 5), "visual"),
    ("ab cd\nef", (0, 3), "v4aw", (0, 3), (1, 2), "visual"),
    ("ab cd\n", (0, 3), "v2aw", (0, 3), (1, 0), "visual"),
    ("end; of [it]", (0, 11), "v3aW", (0, 8), (0, 12), "visual"),
    ("a  ", (0, 2), "vaw", (0, 2), (0, 3), "visual"),
    ("    \n    ", (0, 2), "vaw", (0, 2), (1, 4), "visual"),
    ("x  \ny", (0, 2), "v2aw", (0, 1), (1, 1), "visual"),
    ("x\ny\nz", (2, 0), "V3aw", (2, 0), (2, 1), "visual_line"),
    (
        "x\ny\nz",
        (2, 0),
        "<C-v>3aw",
        (2, 0),
        (2, 1),
        "visual_block",
    ),
    ("x\ny\nz", (0, 0), "V9iw", (0, 0), (2, 1), "visual_line"),
    (
        "x\ny\nz",
        (0, 0),
        "<C-v>9iW",
        (0, 0),
        (2, 1),
        "visual_block",
    ),
];

#[test]
fn word_objects_over_one_visual_position_run_out_to_the_end_as_nvim() {
    for &(buffer, cursor, keys, anchor, end, mode) in ROWS {
        let mut vim = HeadlessVim::new(buffer, cursor);
        vim.feed(keys).unwrap();
        let what = format!("{keys} on {buffer:?} at {cursor:?}");
        assert_eq!(vim.visual_anchor(), Some(anchor), "anchor of {what}");
        assert_eq!(vim.cursor(), end, "cursor of {what}");
        assert_eq!(vim.mode(), mode, "mode of {what}");
        assert!(vim.state().failed(), "{what} fails");
    }

    let mut vim = HeadlessVim::new("x y z", (0, 2));
    vim.feed("v3awy").unwrap();
    assert_eq!(vim.registers().get('"').unwrap().text, "y z");
    assert_eq!(vim.mark('<'), Some((0, 2)));
    assert_eq!(vim.mark('>'), Some((0, 5)));

    let mut vim = HeadlessVim::new("x y z", (0, 2));
    vim.feed("v3awd").unwrap();
    assert_eq!(vim.text(), "x ");
    assert_eq!(vim.cursor(), (0, 1));

    // Counts that don't run out, and the empty buffer, are as before.
    let mut vim = HeadlessVim::new("x y z", (0, 2));
    vim.feed("v2aw").unwrap();
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 1)), (0, 4)));
    assert!(!vim.state().failed());
    let mut vim = HeadlessVim::new("", (0, 0));
    vim.feed("vaw").unwrap();
    assert!(vim.state().failed());
}
