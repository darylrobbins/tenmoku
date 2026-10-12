//! Tests of the prose feature: `{ } ( )` and `ip ap is as` (see `src/prose.rs`).

use super::{commit, registers, run};
use crate::{
    HeadlessVim, Motion, MoveArgs, OperateArgs, Operator, RangeClass, SelectArgs, Step, Target,
    TextObject,
};

/// Paragraphs: rows 2, 6 and 7 are empty, row 4 has only blanks.
const TEXT: &str =
    "One two. Three four!\nFive six? Seven.\n\nEight nine. Ten.\n  \nEleven twelve.\n\n\nLast one";

/// `keys` over `text` from `cursor`: the text and the caret.
fn feed(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

/// Where `keys` leave the caret, the text unchanged.
fn lands(text: &str, cursor: (u32, usize), keys: &str) -> (u32, usize) {
    let (after, caret) = feed(text, cursor, keys);
    assert_eq!(after, text, "{keys}");
    caret
}

/// What `keys` delete (or yank) from `cursor`: the text left and register
/// `"` (its text and whether linewise).
fn takes(text: &str, cursor: (u32, usize), keys: &str) -> (String, String, bool) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    let register = vim.registers().get('"').expect(keys);
    (vim.text(), register.text.clone(), register.linewise)
}

#[test]
fn keys_commit_motions_and_objects() {
    let motions = [
        ("}", Motion::ParagraphForward, "paragraph_forward"),
        ("{", Motion::ParagraphBackward, "paragraph_backward"),
        (")", Motion::SentenceForward, "sentence_forward"),
        ("(", Motion::SentenceBackward, "sentence_backward"),
    ];
    for (key, motion, name) in motions {
        let committed = commit(&format!("3{key}"));
        assert_eq!(committed.command.as_str(), "vim.move", "{key}");
        let args = MoveArgs {
            motion,
            extend: false,
            count: Some(3),
        };
        assert_eq!(committed.args, serde_json::to_value(args).unwrap(), "{key}");
        let committed = commit(&format!("\"a2d{key}"));
        let args = OperateArgs {
            operator: Operator::Delete,
            target: Target::Motion(motion),
            count: Some(2),
            register: Some('a'),
        };
        assert_eq!(committed.args, serde_json::to_value(args).unwrap(), "{key}");
        assert_eq!(
            serde_json::to_value(motion).unwrap(),
            serde_json::json!(name)
        );
        // Exclusive jumps whose delete writes register `1` even in a row.
        assert_eq!(motion.class(), RangeClass::Exclusive, "{key}");
        assert!(motion.is_jump(), "{key}");
        assert!(Target::Motion(motion).writes_numbered(), "{key}");
    }
    let objects = [
        ("ip", TextObject::InnerParagraph, "inner_paragraph"),
        ("ap", TextObject::AParagraph, "a_paragraph"),
        ("is", TextObject::InnerSentence, "inner_sentence"),
        ("as", TextObject::ASentence, "a_sentence"),
    ];
    for (keys, object, name) in objects {
        let committed = commit(&format!("y2{keys}"));
        let args = OperateArgs {
            operator: Operator::Yank,
            target: Target::TextObject(object),
            count: Some(2),
            register: None,
        };
        assert_eq!(
            committed.args,
            serde_json::to_value(args).unwrap(),
            "{keys}"
        );
        assert_eq!(
            serde_json::to_value(object).unwrap(),
            serde_json::json!(name)
        );
    }
    // Others after `i` and `a` still cancel.
    assert_eq!(run("diq"), Step::Cancel);
}

#[test]
fn paragraph_motions() {
    // To the next empty row; a run of them is one boundary; a row of
    // blanks is none.
    assert_eq!(lands(TEXT, (0, 3), "}"), (2, 0));
    assert_eq!(lands(TEXT, (0, 3), "2}"), (6, 0));
    assert_eq!(lands(TEXT, (6, 0), "}"), (8, 7));
    assert_eq!(lands(TEXT, (5, 3), "{"), (2, 0));
    assert_eq!(lands(TEXT, (8, 2), "2{"), (2, 0));
    // Run out at the ends: the last char, the first.
    assert_eq!(lands(TEXT, (0, 3), "3}"), (8, 7));
    assert_eq!(lands(TEXT, (8, 7), "}"), (8, 7));
    assert_eq!(lands(TEXT, (1, 4), "{"), (0, 0));
    // `d}` from a first column is linewise; from mid row it stops at the
    // row's end; at the end it takes the last char.
    assert_eq!(
        takes(TEXT, (3, 0), "d}"),
        (
            "One two. Three four!\nFive six? Seven.\n\n\n\nLast one".into(),
            "Eight nine. Ten.\n  \nEleven twelve.\n".into(),
            true
        )
    );
    assert_eq!(takes("ab\ncd\n\nef", (0, 1), "d}").1, "b\ncd");
    assert_eq!(
        takes("ab\ncd", (1, 1), "d}"),
        ("ab\nc".into(), "d".into(), false)
    );
    // In a visual mode.
    assert_eq!(takes(TEXT, (8, 2), "v{y").1, "\nLas");
}

#[test]
fn sentence_motions() {
    // A sentence ends at `.`, `!` or `?`, closers, then a blank or the row's
    // end; the next starts at the first non-blank after.
    let text = "He said \"Stop.\" Then (he left.) Done\n   Next row?  Yes.";
    assert_eq!(lands(text, (0, 0), ")"), (0, 16));
    assert_eq!(lands(text, (0, 0), "2)"), (0, 32));
    assert_eq!(lands(text, (0, 32), ")"), (1, 14));
    assert_eq!(lands(text, (1, 5), "("), (0, 32));
    assert_eq!(lands(text, (0, 33), "2("), (0, 16));
    // No blank after the mark: no end.
    assert_eq!(lands("a.b c. d", (0, 0), ")"), (0, 7));
    assert_eq!(lands("a.\tb", (0, 0), ")"), (0, 3));
    // An empty row is a start, and so is the first non-blank after one.
    assert_eq!(lands(TEXT, (1, 12), ")"), (2, 0));
    assert_eq!(lands(TEXT, (2, 0), ")"), (3, 0));
    assert_eq!(lands(TEXT, (3, 13), "3("), (2, 0));
    // Run out at the ends: the last char, the first.
    assert_eq!(lands(TEXT, (8, 2), ")"), (8, 7));
    assert_eq!(lands(TEXT, (0, 5), "("), (0, 0));
    // Exclusive, but taking the last char at the end.
    assert_eq!(
        takes(TEXT, (0, 9), "c)X<Esc>").0.lines().next().unwrap(),
        "One two. X"
    );
    assert_eq!(takes(TEXT, (1, 12), "y(").1, "Se");
    assert_eq!(
        takes("Ab. Cd.", (0, 4), "d)"),
        ("Ab. ".into(), "Cd.".into(), false)
    );
    assert!(takes(TEXT, (0, 0), "d2)").2);
    // A delete within a row writes register 1 and shifts nothing else in.
    let mut vim = HeadlessVim::new("Ab. Cd. Ef", (0, 0));
    vim.feed("d)").unwrap();
    assert_eq!(vim.registers().get('1').unwrap().text, "Ab. ");
    // The jump mark is where it left from.
    let mut vim = HeadlessVim::new(TEXT, (0, 3));
    vim.feed("2)").unwrap();
    assert_eq!(vim.mark('\''), Some((0, 3)));
    vim.feed("}``").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
}

#[test]
fn past_the_ends_fails_with_nothing_changed() {
    for (cursor, keys) in [
        ((0, 3), "9}"),
        ((3, 4), "5{"),
        ((8, 2), "2)"),
        ((0, 10), "3("),
        ((0, 3), "d9}"),
        ((8, 2), "y2)"),
        ((5, 0), "d9ip"),
        ((0, 0), "c5ap"),
    ] {
        let mut vim = HeadlessVim::new(TEXT, cursor);
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        assert_eq!(vim.text(), TEXT, "{keys}");
        assert_eq!(vim.cursor(), cursor, "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(vim.mark('\''), None, "{keys}");
        assert!(registers(&vim).is_empty(), "{keys}");
    }
    // In a visual mode the selection stays.
    let mut vim = HeadlessVim::new(TEXT, (8, 1));
    vim.feed("v2}").unwrap();
    assert!(vim.state().failed());
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((8, 1), Some((8, 1))));
}

#[test]
fn objects_with_counts() {
    // `ip` is a run of text rows or of blank ones (a row of blanks counts);
    // `ap` takes the blank rows after it, or with none, those before.
    assert_eq!(takes(TEXT, (3, 4), "dip").1, "Eight nine. Ten.\n");
    assert_eq!(takes(TEXT, (3, 4), "dap").1, "Eight nine. Ten.\n  \n");
    assert_eq!(takes(TEXT, (4, 1), "yip").1, "  \n");
    assert_eq!(
        takes(TEXT, (0, 0), "y2ip").1,
        "One two. Three four!\nFive six? Seven.\n\n"
    );
    assert_eq!(
        takes(TEXT, (8, 1), "dap"),
        (
            "One two. Three four!\nFive six? Seven.\n\nEight nine. Ten.\n  \nEleven twelve.".into(),
            "\n\nLast one\n".into(),
            true
        )
    );
    assert_eq!(takes(TEXT, (6, 0), "yap").1, "\n\nLast one\n");
    // `is` is a sentence, or the blanks between two; `as` takes the blanks
    // after it, or with none, those before; a count takes the blanks as
    // one.
    assert_eq!(takes(TEXT, (0, 10), "dis").1, "Three four!");
    assert_eq!(takes(TEXT, (0, 10), "das").1, " Three four!");
    assert_eq!(takes(TEXT, (0, 2), "d2is").1, "One two. ");
    assert_eq!(takes(TEXT, (0, 8), "yis").1, " ");
    assert_eq!(takes(TEXT, (0, 8), "yas").1, " Three four!");
    assert_eq!(takes(TEXT, (3, 13), "das").1, "Ten.\n  ");
    // An empty row is a sentence, its delete linewise.
    assert_eq!(takes(TEXT, (2, 0), "dis").1, "\n");
    assert!(takes(TEXT, (2, 0), "dis").2);
    // `c` and `.`.
    assert_eq!(
        takes(TEXT, (1, 3), "cisNew one.<Esc>")
            .0
            .lines()
            .nth(1)
            .unwrap(),
        "New one. Seven."
    );
    assert_eq!(feed(TEXT, (0, 0), "dap.").0, "Eleven twelve.\n\n\nLast one");
}

#[test]
fn visual_objects() {
    let visual = |cursor, keys| {
        let mut vim = HeadlessVim::new(TEXT, cursor);
        vim.feed(keys).unwrap();
        (vim.mode(), vim.visual_anchor(), vim.cursor())
    };
    // `ip` and `ap` make it linewise; repeated, each adds the next.
    assert_eq!(visual((0, 3), "vip"), ("visual_line", Some((0, 3)), (1, 0)));
    assert_eq!(
        visual((0, 3), "vipip"),
        ("visual_line", Some((0, 3)), (2, 0))
    );
    assert_eq!(
        visual((0, 3), "vapap"),
        ("visual_line", Some((0, 3)), (4, 0))
    );
    assert_eq!(visual((1, 3), "vip"), ("visual_line", Some((0, 0)), (1, 0)));
    // Growing keeps the mode, the way the cursor faces.
    assert_eq!(visual((0, 3), "vjip"), ("visual", Some((0, 3)), (2, 0)));
    assert_eq!(
        visual((5, 3), "Vkip"),
        ("visual_line", Some((5, 3)), (3, 0))
    );
    // A linewise row that's a run of its own takes one more.
    assert_eq!(visual((3, 2), "Vip"), ("visual_line", Some((3, 2)), (4, 0)));
    // `is` and `as` over one position make it charwise.
    assert_eq!(visual((0, 10), "Vis"), ("visual", Some((0, 9)), (0, 19)));
    assert_eq!(visual((0, 10), "vas"), ("visual", Some((0, 8)), (0, 19)));
    // Past the end it fails, keeping what it got to.
    let mut vim = HeadlessVim::new(TEXT, (8, 2));
    vim.feed("vkip").unwrap();
    assert!(!vim.state().failed());
    vim.feed("V9ip").unwrap();
    assert!(vim.state().failed());
    // `vim.select` itself.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("v").unwrap();
    let args = SelectArgs {
        text_object: TextObject::AParagraph,
        count: Some(2),
    };
    vim.dispatch(&super::invocation("vim.select", args).context(crate::context(vim.state())))
        .unwrap();
    assert_eq!((vim.mode(), vim.cursor()), ("visual_line", (4, 0)));
}

/// `keys` from `cursor`: the mode, the visual anchor and the caret.
fn selects(
    text: &str,
    cursor: (u32, usize),
    keys: &str,
) -> (&'static str, Option<(u32, usize)>, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    assert!(!vim.state().failed(), "{keys}");
    (vim.mode(), vim.visual_anchor(), vim.cursor())
}

#[test]
fn visual_sentences_grow() {
    // Each item is a sentence or the blanks before the next one, the way
    // the cursor faces, from the position past it: the rest of a sentence
    // is one. `as` is two.
    let text = "Ab. Cd. Ef.";
    assert_eq!(
        selects(text, (0, 0), "vlis"),
        ("visual", Some((0, 0)), (0, 2))
    );
    assert_eq!(
        selects(text, (0, 0), "vl2is"),
        ("visual", Some((0, 0)), (0, 3))
    );
    assert_eq!(
        selects(text, (0, 0), "vlas"),
        ("visual", Some((0, 0)), (0, 3))
    );
    assert_eq!(
        selects(text, (0, 9), "vhis"),
        ("visual", Some((0, 9)), (0, 7))
    );
    assert_eq!(
        selects(text, (0, 9), "vh2as"),
        ("visual", Some((0, 9)), (0, 0))
    );
    // Running out of text keeps what it got to, and doesn't fail.
    assert_eq!(
        selects(text, (0, 0), "vis9is"),
        ("visual", Some((0, 0)), (0, 10))
    );
    assert_eq!(
        selects(text, (0, 9), "viso9as"),
        ("visual", Some((0, 10)), (0, 0))
    );
    assert_eq!(
        selects(text, (0, 10), "v$is"),
        ("visual", Some((0, 10)), (0, 10))
    );
    // An empty row is two items going forward: the blanks before it, none
    // here, then the row; going back the row is one.
    let text = "A.\n\nB.";
    assert_eq!(
        selects(text, (0, 0), "visis"),
        ("visual", Some((0, 0)), (1, 0))
    );
    assert_eq!(
        selects(text, (0, 0), "vis2is"),
        ("visual", Some((0, 0)), (1, 0))
    );
    assert_eq!(
        selects(text, (0, 0), "vis3is"),
        ("visual", Some((0, 0)), (2, 1))
    );
    assert_eq!(
        selects(text, (2, 0), "visois"),
        ("visual", Some((2, 1)), (1, 0))
    );
    assert_eq!(
        selects(text, (2, 0), "visoisis"),
        ("visual", Some((2, 1)), (0, 0))
    );
    // Growing keeps the mode; over one position it becomes charwise.
    let text = "Ab. Cd.\nEf.";
    assert_eq!(
        selects(text, (0, 0), "Vlis"),
        ("visual_line", Some((0, 0)), (0, 2))
    );
    assert_eq!(
        selects(text, (0, 0), "<C-v>las"),
        ("visual_block", Some((0, 0)), (0, 3))
    );
    assert_eq!(
        selects(text, (0, 5), "Vis"),
        ("visual", Some((0, 4)), (0, 6))
    );
    assert_eq!(
        takes(text, (0, 0), "Vlisd"),
        ("Ef.".into(), "Ab. Cd.\n".into(), true)
    );
}

#[test]
fn visual_sentences_on_empty_rows() {
    // A sentence of one char before an empty row: `as` or two `is` take
    // the row as the blanks after it.
    assert_eq!(
        selects("A\n\nB", (0, 0), "vis"),
        ("visual", Some((0, 0)), (0, 0))
    );
    assert_eq!(
        selects("A\n\nB", (0, 0), "vas"),
        ("visual", Some((0, 0)), (1, 0))
    );
    assert_eq!(
        selects("A\n\nB", (0, 0), "v2is"),
        ("visual", Some((0, 0)), (1, 0))
    );
    // On an empty row: blanks before the next sentence, with any starting
    // the next row.
    let text = "A.\n\nB. C.";
    assert_eq!(
        selects(text, (1, 0), "vis"),
        ("visual", Some((1, 0)), (1, 0))
    );
    assert_eq!(
        selects(text, (1, 0), "vas"),
        ("visual", Some((1, 0)), (2, 1))
    );
    assert_eq!(
        selects(text, (1, 0), "v3is"),
        ("visual", Some((1, 0)), (2, 1))
    );
    assert_eq!(
        selects(text, (1, 0), "v4is"),
        ("visual", Some((1, 0)), (2, 2))
    );
    assert_eq!(
        selects("A.\n\n  C. D.", (1, 0), "vis"),
        ("visual", Some((1, 0)), (2, 1))
    );
    // Not a run's last empty row: up to it, or back to the text before a
    // longer run.
    assert_eq!(
        selects("A.\n\n\nB.", (1, 0), "vis"),
        ("visual", Some((1, 0)), (2, 0))
    );
    assert_eq!(
        selects("A.\n\n\n\nB.", (1, 0), "vis"),
        ("visual", Some((1, 0)), (0, 1))
    );
    assert_eq!(
        selects("A.\n\n\nB.", (1, 0), "v3is"),
        ("visual", Some((2, 0)), (3, 1))
    );
}

#[test]
fn huge_counts_end_as_long_ones_do() {
    // `as` doubles the largest count without overflowing, and the steps
    // that go round at a text's end are skipped: it ends where a long count
    // of the same parity does, at once (nvim itself takes minutes).
    let started = std::time::Instant::now();
    let text = "Ab. Cd. Ef.\n\nGh. ";
    let end = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.feed(keys).unwrap();
        (
            vim.text(),
            vim.cursor(),
            vim.visual_anchor(),
            registers(&vim),
        )
    };
    for (cursor, huge, long) in [
        ((0, 4), "d4294967295as", "d99as"),
        ((0, 4), "d4294967295is", "d99is"),
        ((0, 4), "d4294967294is", "d98is"),
        ((2, 0), "v4294967295as", "v99as"),
        ((0, 4), "vl4294967295is", "vl99is"),
        ((0, 4), "vl4294967294is", "vl98is"),
        ((2, 3), "vh4294967295as", "vh99as"),
        ((2, 3), "vh4294967294is", "vh98is"),
    ] {
        assert_eq!(end(cursor, huge), end(cursor, long), "{huge}");
    }
    // So do `(` and `)` where a step gets nowhere: on punctuation alone, or
    // a text starting with it, in each mode (each fails, as nvim's does).
    for text in ["!", "!. A", ". .", "!\n\n."] {
        for (huge, long) in [
            ("4294967295(", "99("),
            ("4294967294(", "98("),
            ("4294967295)", "99)"),
            ("v4294967295(", "v99("),
            ("d4294967295(", "d99("),
            ("y4294967294)", "y98)"),
        ] {
            let end = |keys: &str| {
                let mut vim = HeadlessVim::new(text, (0, 0));
                vim.feed(keys).unwrap();
                let state = (vim.text(), vim.cursor(), vim.visual_anchor());
                (state, registers(&vim), vim.mark('\''))
            };
            assert_eq!(end(huge), end(long), "{text:?} {huge}");
        }
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn visual_sentences_of_marks_and_blanks() {
    // A sentence of punctuation alone before an empty row keeps the
    // anchor, taking the row.
    assert_eq!(
        selects("!\n\nB.", (0, 0), "vis"),
        ("visual", Some((0, 0)), (1, 0))
    );
    assert_eq!(takes("!\n\nB.", (0, 0), "visd").0, "B.");
    // One mark alone: the anchor at its row's end.
    assert_eq!(
        selects("!", (0, 0), "vis"),
        ("visual", Some((0, 1)), (0, 0))
    );
    // On blanks alone `as` takes one; two items keep the anchor and go back
    // to the start.
    assert_eq!(
        selects("   ", (0, 0), "vas"),
        ("visual", Some((0, 0)), (0, 0))
    );
    assert_eq!(
        selects("   ", (0, 2), "v2as"),
        ("visual", Some((0, 2)), (0, 0))
    );
    assert_eq!(takes("   ", (0, 2), "v2asd").1, "   ");
}

#[test]
fn visual_sentences_from_a_row_end() {
    // After `$` the cursor is at the row's end, and grows from there: on to
    // the next row's sentence, or back to the sentence ending the row.
    assert_eq!(
        selects("Ab. Cd.\nEf.", (0, 0), "v$is"),
        ("visual", Some((0, 0)), (1, 2))
    );
    assert_eq!(
        selects("Ab. Cd.\nEf. Gh.", (1, 6), "vk$is"),
        ("visual", Some((1, 6)), (0, 4))
    );
    // Back from a row of blanks' end, its blank; after blanks before an
    // empty row, on to that row.
    assert_eq!(
        selects("A.\n \nB.", (2, 1), "vkis"),
        ("visual", Some((2, 1)), (1, 0))
    );
    assert_eq!(takes("A.\n \nB.", (2, 1), "vkisd").0, "A.\n");
    assert_eq!(
        selects("Ab. \n\n Cd.", (2, 3), "v2k$is"),
        ("visual", Some((2, 3)), (1, 0))
    );
}
