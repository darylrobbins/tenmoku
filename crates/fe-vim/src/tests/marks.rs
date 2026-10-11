use serde_json::json;

use super::{commit, registers};
use crate::{HeadlessVim, MarkMotion, Motion, RangeClass};

fn ai() -> fe_buffer::Origin {
    fe_buffer::Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

#[test]
fn marks_as_anchors_and_motions() {
    // `ma` sets mark `a`; `'a` and `` `a `` move to it, linewise and exactly.
    let set = commit("ma");
    assert_eq!(set.command.as_str(), "vim.set_mark");
    assert_eq!(set.args, json!({"name": "a"}));
    for (keys, exact, class) in [
        ("'a", false, RangeClass::Linewise),
        ("`a", true, RangeClass::Exclusive),
    ] {
        let moved = commit(keys);
        assert_eq!(moved.command.as_str(), "vim.move", "{keys}");
        assert_eq!(
            moved.args,
            json!({"motion": {"mark": {"name": "a", "exact": exact}}, "extend": false}),
            "{keys}"
        );
        assert_eq!(Motion::Mark(MarkMotion { name: 'a', exact }).class(), class);
    }

    let text = "one\n  two three\nfour\nfive";
    let mut vim = HeadlessVim::new(text, (1, 6));
    vim.feed("maG'a").unwrap();
    assert_eq!(vim.cursor(), (1, 2));
    vim.feed("G`a").unwrap();
    assert_eq!(vim.cursor(), (1, 6));

    // A row put in above, by vim or another origin, moves the mark down with
    // its row: `'a` lands on that row's first non-blank and `` `a `` on its
    // column.
    vim.feed("ggOzero<Esc>").unwrap();
    vim.model_mut()
        .buffer_mut()
        .edit([(0..0, "new\n")], ai())
        .unwrap();
    assert_eq!(vim.mark('a'), Some((3, 6)));
    vim.feed("gg'a").unwrap();
    assert_eq!(vim.cursor(), (3, 2));
    vim.feed("gg`a").unwrap();
    assert_eq!(vim.cursor(), (3, 6));

    // `dd` over its row deletes it: `'a` is E20 and moves nothing.
    vim.feed("ddgg0'a").unwrap();
    assert_eq!(vim.mark('a'), None);
    assert_eq!(vim.state().message(), Some("E20: Mark not set"));
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("d`b").unwrap();
    assert_eq!(vim.state().message(), Some("E20: Mark not set"));
    assert_eq!(vim.text(), "new\nzero\none\nfour\nfive");

    // `d'a` takes whole rows, `` d`a `` up to the mark, not including it.
    let mut vim = HeadlessVim::new("abc\ndef\nghi\njkl", (1, 1));
    vim.feed("majd'a").unwrap();
    assert_eq!(vim.text(), "abc\njkl");
    assert_eq!(registers(&vim)[0], ('"', "def\nghi\n".into(), true));
    let mut vim = HeadlessVim::new("abc\ndef\nghi\njkl", (1, 1));
    vim.feed("majld`a").unwrap();
    assert_eq!(vim.text(), "abc\ndi\njkl");
    assert_eq!(registers(&vim)[0], ('"', "ef\ngh".into(), false));

    // `''` goes back to where each jump left from, and `''` again returns.
    let text = "a\n(b\nc)\nd\nxe\nf";
    for (jump, there) in [("G", (5, 0)), ("/x<CR>", (4, 0)), ("%", (2, 1))] {
        let mut vim = HeadlessVim::new(text, (1, 0));
        vim.feed(jump).unwrap();
        assert_eq!(vim.cursor(), there, "{jump}");
        vim.feed("''").unwrap();
        assert_eq!(vim.cursor(), (1, 0), "{jump}");
        vim.feed("''").unwrap();
        assert_eq!(vim.cursor(), (there.0, 0), "{jump}");
        vim.feed("``").unwrap();
        assert_eq!(vim.cursor(), (1, 0), "{jump}");
    }
    // `:5` isn't a jump (`:help jump-motions`): as in nvim, `''` after it
    // goes back to where the last jump left from, and before any, is E20.
    let mut vim = HeadlessVim::new(text, (1, 0));
    vim.feed(":5<CR>''").unwrap();
    assert_eq!(vim.cursor(), (4, 0));
    assert_eq!(vim.state().message(), Some("E20: Mark not set"));
    let mut vim = HeadlessVim::new(text, (1, 0));
    vim.feed("G:5<CR>''").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    vim.feed("''").unwrap();
    assert_eq!(vim.cursor(), (4, 0));

    // `:'a,'bd` deletes the rows from one mark's through the other's.
    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne\nf", (1, 0));
    vim.feed("majjmb:'a,'bd<CR>").unwrap();
    assert_eq!(vim.text(), "a\ne\nf");
    vim.feed(":'a<CR>").unwrap();
    assert_eq!(vim.state().message(), Some("E20: Mark not set"));

    // In a visual mode `'a` moves the vim cursor, keeping the anchor.
    let mut vim = HeadlessVim::new("abc\n  def\nghi", (1, 4));
    vim.feed("majhv'a").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((2, 1)), (1, 2)));
    vim.feed("`a").unwrap();
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((2, 1)), (1, 4)));
    // `m` there sets a mark at the vim cursor and stays in the mode.
    vim.feed("mbk").unwrap();
    assert_eq!((vim.mode(), vim.mark('b')), ("visual", Some((1, 4))));
}

#[test]
fn marks_follow_joins_and_other_origins() {
    // Joined to the row above, a mark goes with its text.
    let mut vim = HeadlessVim::new("aa\n  bbb\ncc", (1, 3));
    vim.feed("makJ").unwrap();
    assert_eq!(vim.mark('a'), Some((0, 4)));
    // Edits within its row leave its column alone, as nvim's do.
    let mut vim = HeadlessVim::new("abcdef\nxyz", (0, 4));
    vim.feed("ma0xx").unwrap();
    assert_eq!(vim.mark('a'), Some((0, 4)));
    vim.feed("`a").unwrap();
    assert_eq!(vim.cursor(), (0, 3));

    // Another origin deleting the row (its text and line break) deletes the
    // mark; emptying it doesn't.
    let mut vim = HeadlessVim::new("aa\nbb\ncc", (1, 1));
    vim.feed("majmb").unwrap();
    vim.model_mut()
        .buffer_mut()
        .edit([(3..5, "")], ai())
        .unwrap();
    assert_eq!(vim.mark('a'), Some((1, 1)));
    vim.model_mut()
        .buffer_mut()
        .edit([(3..4, "")], ai())
        .unwrap();
    assert_eq!((vim.mark('a'), vim.mark('b')), (None, Some((1, 1))));

    // A mark on the last row is deleted with it once rows were opened below.
    let mut vim = HeadlessVim::new("aa\nbb", (1, 1));
    vim.feed("maox<Esc>kdd").unwrap();
    assert_eq!(vim.mark('a'), None);
    // A linewise change keeps its first row's marks and deletes the rest.
    let mut vim = HeadlessVim::new("aa\nbb\ncc\ndd", (1, 1));
    vim.feed("majmbk2ccX<Esc>").unwrap();
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((1, 1)), None));

    // `m'` sets the jump mark; `m` takes no other names.
    let mut vim = HeadlessVim::new("a\nb\nc", (1, 0));
    vim.feed("m'jj''").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(vim.mark('\''), Some((2, 0)));
    vim.feed("mA").unwrap();
    assert_eq!(vim.mark('A'), None);
}
