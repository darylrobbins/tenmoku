use fe_buffer::{Origin, ResolvedSelection};

use super::Measuring;
use crate::HeadlessVim;

fn carets(vim: &mut HeadlessVim, offsets: &[usize]) {
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
}

fn heads(vim: &HeadlessVim) -> Vec<usize> {
    vim.model().selections().iter().map(|s| s.head()).collect()
}

#[test]
fn carets_inside_another_carets_edit() {
    // The second caret is in the indentation the first's `J` removes: it
    // stays on the joined row.
    let mut vim = HeadlessVim::new("a\n          b\nc", (0, 0));
    carets(&mut vim, &[0, 7]);
    vim.feed("J").unwrap();
    assert_eq!(vim.text(), "a b\nc");
    assert_eq!(heads(&vim), [1, 2]);

    // Growing (`ß` is two bytes, `ẞ` three) and shrinking replacements.
    let mut vim = HeadlessVim::new("ßßß x", (0, 0));
    carets(&mut vim, &[0, 2]);
    vim.feed("3~").unwrap();
    assert_eq!(vim.text(), "ẞẞẞ x");
    // Two bytes into the new text is inside the first `ẞ`: its start.
    assert_eq!(heads(&vim), [0, 9]);
    let mut vim = HeadlessVim::new("ééé x", (0, 0));
    carets(&mut vim, &[0, 4]);
    vim.feed("3rZ").unwrap();
    assert_eq!(vim.text(), "ZZZ x");
    // Further in than the new text reaches: just after it.
    assert_eq!(heads(&vim), [2, 3]);
}

#[test]
fn carets_land_past_edits_made_while_measuring() {
    // Measuring text as the model catches up after a command's edit may
    // edit the buffer through another model: the caret lands where the
    // command put it all the same, here past a `!` another origin puts at
    // the start meanwhile.
    let run = |text: &str, cursor: (u32, usize), keys: &str, measuring: bool| {
        let mut vim = HeadlessVim::new(text, cursor);
        let mut split = vim.model().split();
        let mut edited = !measuring;
        let _measuring = Measuring::new(move |after_edit| {
            if after_edit && !edited {
                edited = true;
                let ai = Origin::Ai {
                    agent: "agent".into(),
                    session: "session".into(),
                };
                split.buffer_mut().edit([(0..0, "!")], ai);
            }
        });
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    for (text, cursor, keys) in [
        ("abc", (0, 1), "rλ"),
        ("ab\ncd", (0, 0), "J"),
        ("abc", (0, 0), "~"),
        ("ab", (0, 0), ">>"),
        ("abc", (0, 1), "x"),
        ("ab\ncd", (0, 1), "dd"),
        ("abc", (0, 0), "ylp"),
    ] {
        let (plain, at) = run(text, cursor, keys, false);
        let (edited, landed) = run(text, cursor, keys, true);
        assert_eq!(edited, format!("!{plain}"), "{keys}");
        let past = usize::from(at.0 == 0);
        assert_eq!(landed, (at.0, at.1 + past), "{keys}");
    }
}
