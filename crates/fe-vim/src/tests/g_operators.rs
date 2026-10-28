//! Tests of the `g` operators: `gu gU g~ g? gq gw` (see `src/g_operators.rs`).

use fe_buffer::{Origin, ResolvedSelection};

use super::{Measuring, invocation};
use crate::{HeadlessVim, Motion, OperateArgs, Operator, Target, TextObject, context};

/// The text and caret after `keys` from `cursor` in `text`, checking no
/// register was written.
fn after(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    assert_eq!(vim.registers().get('"'), None, "{keys}");
    (vim.text(), vim.cursor())
}

#[track_caller]
fn check(text: &str, cursor: (u32, usize), keys: &str, want: &str, caret: (u32, usize)) {
    assert_eq!(
        after(text, cursor, keys),
        (want.to_string(), caret),
        "{keys}"
    );
}

/// `vim` with a caret at each of `offsets`.
fn carets(vim: &mut HeadlessVim, offsets: &[usize]) {
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
}

/// Every caret, as `(row, byte column)`.
fn heads(vim: &HeadlessVim) -> Vec<(u32, usize)> {
    let text = vim.model().text().clone();
    vim.model()
        .selections()
        .iter()
        .map(|selection| {
            let point = text.offset_to_point(selection.head());
            (point.row, point.column as usize)
        })
        .collect()
}

/// The text and every caret after `keys` from `cursor` in `text`.
fn every(text: &str, cursor: (u32, usize), keys: &str) -> (String, Vec<(u32, usize)>) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), heads(&vim))
}

const ROWS: &str = "Hello World foo\nSecond Row\nthird";

#[test]
fn case_and_rot13() {
    // Over a motion, an object and a count: the caret goes to the start.
    check(
        ROWS,
        (0, 3),
        "gUw",
        "HelLO World foo\nSecond Row\nthird",
        (0, 3),
    );
    check(
        ROWS,
        (0, 8),
        "gub",
        "Hello world foo\nSecond Row\nthird",
        (0, 6),
    );
    check(
        ROWS,
        (0, 3),
        "g~iw",
        "hELLO World foo\nSecond Row\nthird",
        (0, 0),
    );
    check(
        ROWS,
        (0, 3),
        "g?w",
        "Helyb World foo\nSecond Row\nthird",
        (0, 3),
    );
    check(
        ROWS,
        (0, 3),
        "3gUl",
        "HelLO World foo\nSecond Row\nthird",
        (0, 3),
    );
    check(
        ROWS,
        (0, 3),
        "gU3w",
        "HelLO WORLD FOO\nSecond Row\nthird",
        (0, 3),
    );
    // Over rows, from where the motion started or ended, whichever is
    // first.
    check(
        ROWS,
        (0, 3),
        "gUj",
        "HELLO WORLD FOO\nSECOND ROW\nthird",
        (0, 3),
    );
    check(
        ROWS,
        (1, 3),
        "guk",
        "hello world foo\nsecond row\nthird",
        (0, 3),
    );

    // Doubled, both spellings, over `count` rows: one row puts the caret on
    // its first non-blank, if it was past it.
    for keys in ["guu", "gugu"] {
        check(
            ROWS,
            (0, 3),
            keys,
            "hello world foo\nSecond Row\nthird",
            (0, 0),
        );
    }
    for keys in ["2gUU", "2gUgU"] {
        check(
            ROWS,
            (0, 3),
            keys,
            "HELLO WORLD FOO\nSECOND ROW\nthird",
            (0, 3),
        );
    }
    for keys in ["g~~", "g~g~"] {
        check(
            ROWS,
            (0, 3),
            keys,
            "hELLO wORLD FOO\nSecond Row\nthird",
            (0, 0),
        );
    }
    for keys in ["g??", "g?g?"] {
        check(
            ROWS,
            (0, 3),
            keys,
            "Uryyb Jbeyq sbb\nSecond Row\nthird",
            (0, 0),
        );
    }
    check(
        ROWS,
        (0, 3),
        "2g??",
        "Uryyb Jbeyq sbb\nFrpbaq Ebj\nthird",
        (0, 3),
    );
    check("  Hello", (0, 4), "guu", "  hello", (0, 2));

    // `.` repeats each.
    check(
        ROWS,
        (0, 3),
        "gUwj.",
        "HelLO World foo\nSecOND Row\nthird",
        (1, 3),
    );
    check(
        ROWS,
        (0, 3),
        "g~~j.",
        "hELLO wORLD FOO\nsECOND rOW\nthird",
        (1, 0),
    );
    check(
        ROWS,
        (0, 0),
        "g??j.",
        "Uryyb Jbeyq sbb\nFrpbaq Ebj\nthird",
        (1, 0),
    );

    // One undo step each, back to where it left the caret.
    check(ROWS, (0, 3), "gUju", ROWS, (0, 3));
    check(ROWS, (0, 3), "guuu", ROWS, (0, 0));
    // Even when nothing changed, as in nvim: `u` undoes it, not the `x`.
    let mut vim = HeadlessVim::new("ABC", (0, 0));
    vim.feed("xgUUu").unwrap();
    assert_eq!((vim.text(), vim.cursor()), ("BC".to_string(), (0, 0)));
    // A `.` that changes nothing too, its undo and redo leaving one caret
    // where it was.
    let rows = "abc\nDEF";
    for keys in ["gUUj.u", "gUUj.u<C-r>", "gUwj.u"] {
        assert_eq!(
            every(rows, (0, 0), keys),
            ("ABC\nDEF".to_string(), vec![(1, 0)]),
            "{keys}"
        );
    }

    // In v, V and ctrl-v, over the selection.
    check(
        ROWS,
        (0, 4),
        "vjgu",
        "Hello world foo\nsecond Row\nthird",
        (0, 4),
    );
    check(
        ROWS,
        (0, 4),
        "Vjg?",
        "Uryyb Jbeyq sbb\nFrpbaq Ebj\nthird",
        (0, 0),
    );
    check(
        ROWS,
        (0, 4),
        "<C-v>jlgU",
        "HellO World foo\nSecoND Row\nthird",
        (0, 4),
    );
    check(
        ROWS,
        (0, 4),
        "<C-v>jlg?",
        "Hellb World foo\nSecoaq Row\nthird",
        (0, 4),
    );
    // Even when they change nothing: `u` undoes them, not the `x`.
    for mode in ["v", "V", "<C-v>"] {
        for operator in ["gu", "gU", "g~", "g?"] {
            let keys = format!("x{mode}l{operator}u");
            assert_eq!(
                every("x12 3\n45", (0, 0), &keys),
                ("12 3\n45".to_string(), vec![(0, 0)]),
                "{keys}"
            );
        }
    }
}

/// 30 words, 199 columns.
fn words() -> String {
    (0..30)
        .map(|n| format!("word{n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn format() {
    let long = words();
    let formatted = "word0 word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11\n\
        word12 word13 word14 word15 word16 word17 word18 word19 word20 word21 word22\n\
        word23 word24 word25 word26 word27 word28 word29";
    assert!(formatted.lines().all(|row| row.len() <= 79));
    // `gq` breaks a long row at blanks, leaving the caret on the last row's
    // first non-blank; `gw` leaves it on its text.
    for keys in ["gqq", "gqgq"] {
        check(&long, (0, 5), keys, formatted, (2, 0));
    }
    check(&long, (0, 80), "gww", formatted, (1, 6));
    check(&long, (0, 80), "gwgw", formatted, (1, 6));
    // Every row takes the first's indent.
    let indented = format!("    {long}");
    let want = "    word0 word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11\n    \
        word12 word13 word14 word15 word16 word17 word18 word19 word20 word21\n    \
        word22 word23 word24 word25 word26 word27 word28 word29";
    check(&indented, (0, 5), "gqq", want, (2, 4));
    // Rows join as `J` joins them, then break.
    let short = "short one\nshort two\nshort three";
    check(
        short,
        (0, 3),
        "gqj",
        "short one short two\nshort three",
        (0, 0),
    );
    check(
        short,
        (1, 3),
        "gwj",
        "short one\nshort two short three",
        (1, 3),
    );
    check(
        short,
        (1, 3),
        "gwk",
        "short one short two\nshort three",
        (0, 13),
    );
    check("abc\n  def", (0, 0), "gqj", "abc def", (0, 0));
    // A blank row parts paragraphs.
    check(
        "aaa\n\nbbb\nccc\n\nddd",
        (0, 0),
        "gqG",
        "aaa\n\nbbb ccc\n\nddd",
        (4, 0),
    );

    // One undo step, back to the caret before it.
    check(&long, (0, 5), "gqqu", &long, (0, 5));
    check(short, (1, 3), "gqku", short, (1, 3));
    // `.` repeats it.
    let rows = "short one\nshort two\nshort three\nfour";
    check(
        rows,
        (0, 3),
        "gqj.",
        "short one short two short three\nfour",
        (0, 0),
    );
    // Its undo and redo leave one caret where it was.
    let rows = "aaa\nbbb\nccc\nddd";
    assert_eq!(
        every(rows, (0, 0), "gqjj.u"),
        ("aaa bbb\nccc\nddd".to_string(), vec![(1, 0)])
    );
    assert_eq!(
        every(rows, (0, 0), "gqjj.u<C-r>"),
        ("aaa bbb\nccc ddd".to_string(), vec![(1, 0)])
    );
    let two = format!("{long}\n{long}");
    let (text, carets) = every(&two, (0, 0), "gqqj.u");
    assert_eq!((text.lines().count(), carets), (4, vec![(3, 0)]));
    let (text, carets) = every(&two, (0, 0), "gqqj.u<C-r>");
    assert_eq!((text.lines().count(), carets), (6, vec![(3, 0)]));
}

#[test]
fn several_carets() {
    // Each caret's rows format apart, even next to another's.
    for keys in ["gqq", "gwgw"] {
        let mut vim = HeadlessVim::new("aaa\nbbb", (0, 0));
        carets(&mut vim, &[0, 4]);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), "aaa\nbbb", "{keys}");
    }
    let long = words();
    let two = format!("{long}\n{long}");
    let (once, _) = every(&long, (0, 0), "gqq");
    for keys in ["gqq", "gwgw"] {
        let mut vim = HeadlessVim::new(&two, (0, 0));
        carets(&mut vim, &[0, long.len() + 1]);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), format!("{once}\n{once}"), "{keys}");
    }
    // Rows that overlap format together, each caret where `gq` over its
    // rows leaves it: on the last row of the paragraph its last row is in.
    let mut vim = HeadlessVim::new("a\nb\nc\nd", (0, 0));
    carets(&mut vim, &[0, 2]);
    vim.feed("gqj").unwrap();
    assert_eq!(vim.text(), "a b c\nd");
    assert!(heads(&vim).iter().all(|&caret| caret == (0, 0)));
    let three = format!("{long}\n{long}\nend");
    let (want, caret) = every(&three, (0, 0), "gq2j");
    let mut vim = HeadlessVim::new(&three, (0, 0));
    carets(&mut vim, &[0, long.len() + 1]);
    vim.feed("gqj").unwrap();
    assert_eq!(vim.text(), want);
    assert!(heads(&vim).iter().all(|&at| vec![at] == caret));
    // A caret whose motion fails goes where it moves it, as with one
    // caret, while another's rows format.
    for keys in ["gq3b", "gw3b"] {
        let mut vim = HeadlessVim::new("\nA B\nC D E", (0, 0));
        carets(&mut vim, &[3, 9]);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), "\nA B C D E", "{keys}");
        assert_eq!(heads(&vim)[0], (0, 0), "{keys}");
    }
}

#[test]
fn other_operators_refused() {
    // `vim.operate` with `J r i a` over a motion or object refuses, changing
    // nothing.
    let targets = [
        Target::Motion(Motion::WordForward),
        Target::TextObject(TextObject::InnerWord),
    ];
    for operator in [
        Operator::Join,
        Operator::Replace('x'),
        Operator::Insert,
        Operator::Append,
    ] {
        for target in targets {
            let mut vim = HeadlessVim::new("Abc def", (0, 0));
            let args = OperateArgs {
                operator,
                target,
                count: None,
                register: None,
            };
            let invocation = invocation("vim.operate", args).context(context(vim.state()));
            assert!(
                vim.dispatch(&invocation).is_err(),
                "{operator:?} {target:?}"
            );
            vim.feed(".").unwrap();
            assert_eq!(vim.text(), "Abc def", "{operator:?} {target:?}");
        }
    }
}

#[test]
fn long_unbroken_run() {
    // Rows of `)` join with nothing between into one word, which formats in
    // time linear in its length.
    let rows = vec![")"; 10_000].join("\n");
    let mut vim = HeadlessVim::new(&rows, (0, 0));
    vim.feed("gqG").unwrap();
    assert_eq!(vim.text(), ")".repeat(10_000));
}

#[test]
fn edits_made_while_measuring() {
    // Measuring text after `gq`'s reflow may edit the buffer through another
    // model: the indents become spaces and the caret lands on the rows
    // formatted all the same, past rows another origin puts in or takes out
    // above them meanwhile.
    let run = |text: &str, keys: &str, other: Option<(std::ops::Range<usize>, &'static str)>| {
        let mut vim = HeadlessVim::new(text, (1, 0));
        let mut split = vim.model().split();
        let mut other = other;
        let _measuring = Measuring::new(move |after_edit| {
            if let Some((range, new)) = other.take_if(|_| after_edit) {
                let ai = Origin::Ai {
                    agent: "agent".into(),
                    session: "session".into(),
                };
                split.buffer_mut().edit([(range, new)], ai);
            }
        });
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    let text = "z\n\ta\n\tb";
    for keys in ["gqj", "gwj", "Vjgq", "Vjgw"] {
        let (plain, (row, column)) = run(text, keys, None);
        assert_eq!(plain, "z\n    a b", "{keys}");
        let (put, at) = run(text, keys, Some((0..0, "\tX\n")));
        assert_eq!(
            (put, at),
            (format!("\tX\n{plain}"), (row + 1, column)),
            "{keys}"
        );
        let (taken, at) = run(text, keys, Some((0..2, "")));
        assert_eq!(
            (taken, at),
            (plain[2..].to_string(), (row - 1, column)),
            "{keys}"
        );
        // An indent it changes, or a row it takes out, stays as it left
        // it.
        assert_eq!(run(text, keys, Some((2..3, "X"))).0, "z\nXa b", "{keys}");
        assert_eq!(run(text, keys, Some((1..7, ""))).0, "z", "{keys}");
    }
}

#[test]
fn a_paragraph_motion_that_runs_out_resets_the_goal_column() {
    // `gu3}` from the last row fails without moving, and the goal column
    // 4 that `j` kept goes, as nvim's does.
    let mut vim = HeadlessVim::new("abcdef\nxy", (0, 4));
    vim.feed("jgu3}k").unwrap();
    assert_eq!(vim.cursor(), (0, 1));
}

/// `'[` and `']` after a visual case operator span the selection's rows or
/// ends, whether or not the text changed (nvim 0.12.4's `getpos`).
#[test]
fn visual_case_marks_follow_the_selection() {
    let marks = |text: &str, keys: &str| {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed(keys).unwrap();
        (vim.mark('['), vim.mark(']'))
    };
    // A linewise selection ending on an empty row marks that row.
    for keys in ["VjU", "Vju"] {
        assert_eq!(
            marks("abc\n\nxyz", keys),
            (Some((0, 0)), Some((1, 0))),
            "{keys}"
        );
    }
    assert_eq!(
        marks("abc\n\n", "VjjU"),
        (Some((0, 0)), Some((2, 0))),
        "final empty row"
    );
    // A charwise selection starting on a line break marks the row's end.
    for keys in ["v$ojU", "v$oju"] {
        assert_eq!(
            marks("abc\nDEF", keys),
            (Some((0, 2)), Some((1, 0))),
            "{keys}"
        );
    }
}
