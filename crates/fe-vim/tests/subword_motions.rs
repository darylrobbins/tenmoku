//! `w b e ge` by subwords, while [`HeadlessVim::set_subword_motions`] is
//! on (`feature/subword.rs`). No nvim oracle has subwords: where a keyword
//! run has no subword boundary inside it, they're checked against the word
//! motions, which the oracle corpora check.

use fe_buffer::{Buffer, Point};
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_vim::{HeadlessVim, Registers, VimState};

fn subwords(text: &str, cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.set_subword_motions(true);
    vim
}

/// The byte columns `keys`, fed again and again from `column` on the one
/// row of `text`, stop at, until one doesn't move.
fn stops(text: &str, column: usize, keys: &str) -> Vec<usize> {
    let mut vim = subwords(text, (0, column));
    let mut stops = Vec::new();
    loop {
        let before = vim.cursor();
        vim.feed(keys).unwrap();
        if vim.cursor() == before {
            return stops;
        }
        stops.push(vim.cursor().1);
    }
}

/// Where `keys` from `column` leave the caret.
fn after(text: &str, column: usize, keys: &str) -> usize {
    let mut vim = subwords(text, (0, column));
    vim.feed(keys).unwrap();
    vim.cursor().1
}

/// `keys` from `cursor`: the text and where the caret is.
fn run(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = subwords(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

#[test]
fn subword_motions_step_through_names() {
    const TEXT: &str = "fooBarBaz HTTPServer snake_case utf8Decode";
    // Bar Baz HTTP Server snake case utf 8 Decode, then the last char.
    assert_eq!(stops(TEXT, 0, "w"), [3, 6, 10, 14, 21, 27, 32, 35, 36, 41]);
    // Each subword's last char: foo Bar Baz HTTP Server snake case utf 8
    // Decode.
    assert_eq!(stops(TEXT, 0, "e"), [2, 5, 8, 13, 19, 25, 30, 34, 35, 41]);
    assert_eq!(stops(TEXT, 41, "b"), [36, 35, 32, 27, 21, 14, 10, 6, 3, 0]);
    assert_eq!(stops(TEXT, 41, "ge"), [35, 34, 30, 25, 19, 13, 8, 5, 2, 0]);
    // A count takes that many.
    assert_eq!(after(TEXT, 0, "3w"), 10);
    assert_eq!(after(TEXT, 0, "7w"), 32);
    assert_eq!(after(TEXT, 0, "4e"), 13);
    assert_eq!(after(TEXT, 41, "2b"), 35);
    assert_eq!(after(TEXT, 41, "5ge"), 19);
    assert_eq!(after(TEXT, 15, "b"), 14);
    assert_eq!(after(TEXT, 15, "e"), 19);
    // From the `_`, as from a blank.
    assert_eq!(after(TEXT, 26, "w"), 27);
    assert_eq!(after(TEXT, 26, "b"), 21);
    assert_eq!(after(TEXT, 26, "e"), 30);
    assert_eq!(after(TEXT, 26, "ge"), 25);

    // Around `_` runs, a keyword run of only `_` is a subword, and other
    // classes and rows are as for words.
    const MORE: &str = "__init__ x86_64 a ___ b.cD";
    assert_eq!(stops(MORE, 0, "w"), [2, 9, 10, 13, 16, 18, 22, 23, 24, 25]);
    assert_eq!(stops(MORE, 0, "e"), [5, 9, 11, 14, 16, 20, 22, 23, 24, 25]);
    assert_eq!(stops(MORE, 25, "b"), [24, 23, 22, 18, 16, 13, 10, 9, 2, 0]);
    let mut vim = subwords("abCd\n\nefGh", (0, 0));
    let mut stops = Vec::new();
    for _ in 0..5 {
        vim.feed("w").unwrap();
        stops.push(vim.cursor());
    }
    assert_eq!(stops, [(0, 2), (1, 0), (2, 0), (2, 2), (2, 3)]);
    vim.feed("bbbb").unwrap();
    assert_eq!(vim.cursor(), (0, 2));
    vim.feed("e").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    vim.feed("e").unwrap();
    assert_eq!(vim.cursor(), (2, 1));
    vim.feed("gege").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
}

/// The texts [`subword_motions_equal_word_motions_on_plain_words`] moves
/// over: no keyword run in them has a subword boundary inside it, nor any
/// that deleting the text between runs makes (each holds one kind of
/// keyword char, or a capital only at the start, where nothing comes
/// before it).
const PLAIN: [&str; 11] = [
    "foo bar.baz  qux\n\n  end(x) y\nlast",
    "abc",
    "",
    "a\n\n\nb",
    "  Lead, ++ abc éa\ntrail  \n",
    "日本語 x😀y ©é",
    "___ .. _ ++ __\n_",
    "\tx\ty\n\n",
    "123 456.7\n89",
    "ABC DEF+GHI",
    "one two\nthree",
];

/// `vim` as [`HeadlessVim::new`] makes it for `text` and `cursor`, but for
/// its registry (which takes far longer to build than the cases here take
/// to run), with subword motions `on` or off.
fn reset(vim: &mut HeadlessVim, text: &str, cursor: (u32, usize), on: bool) {
    let mut model = EditorModel::new(Buffer::new(text), None);
    let offset = model
        .text()
        .point_to_offset(Point::new(cursor.0, cursor.1 as u32));
    model.set_cursor(offset);
    *vim.model_mut() = model;
    *vim.state_mut() = VimState::new();
    *vim.registers_mut() = Registers::new();
    vim.set_subword_motions(on);
}

/// What `vim` shows after `keys`: whether they fed, the text, caret,
/// anchor, mode, the last key failing, and the unnamed register.
fn outcome(vim: &mut HeadlessVim, keys: &str) -> String {
    let fed = vim.feed(keys).is_ok();
    let register = vim
        .registers()
        .get('"')
        .map(|register| (register.text.clone(), register.linewise));
    format!(
        "{fed} {:?} {:?} {:?} {} {} {register:?}",
        vim.text(),
        vim.cursor(),
        vim.visual_anchor(),
        vim.mode(),
        vim.state().failed(),
    )
}

#[test]
fn subword_motions_equal_word_motions_on_plain_words() {
    const KEYS: [&str; 52] = [
        "w",
        "b",
        "e",
        "ge",
        "3w",
        "2b",
        "3e",
        "2ge",
        "9w",
        "9b",
        "9e",
        "9ge",
        "dw",
        "db",
        "de",
        "dge",
        "2dw",
        "d3w",
        "d9w",
        "d2e",
        "d9e",
        "d9b",
        "d9ge",
        "cwX<Esc>",
        "c2wX<Esc>",
        "c9wX<Esc>",
        "ceX<Esc>",
        "cbX<Esc>",
        "cgeX<Esc>",
        "yw$p",
        "y2eP",
        "ybp",
        "ygeP",
        "dw.",
        "de.",
        "db.",
        "dge.",
        "2dw.",
        "cw-<Esc>w.",
        "vw",
        "vee",
        "vbd",
        "vgey",
        "v$b",
        "v$e",
        "v$ge",
        "v9e",
        "v9w",
        "vjbd",
        "Vwd",
        "<C-v>ew",
        "wbege",
    ];
    let (mut words, mut parts) = (HeadlessVim::new("", (0, 0)), HeadlessVim::new("", (0, 0)));
    for text in PLAIN {
        let rows: Vec<&str> = text.split('\n').collect();
        for (row, line) in rows.iter().enumerate() {
            let columns = line.char_indices().map(|(column, _)| column);
            let columns: Vec<usize> = columns.chain(line.is_empty().then_some(0)).collect();
            for column in columns {
                let cursor = (row as u32, column);
                for keys in KEYS {
                    reset(&mut words, text, cursor, false);
                    reset(&mut parts, text, cursor, true);
                    let (words, parts) = (outcome(&mut words, keys), outcome(&mut parts, keys));
                    assert_eq!(parts, words, "{keys} at {cursor:?} in {text:?}");
                }
            }
        }
    }
}

#[test]
fn subword_motions_under_operators() {
    const CAMEL: &str = "fooBarBaz";
    assert_eq!(run(CAMEL, (0, 0), "dw"), ("BarBaz".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 0), "dw."), ("Baz".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 0), "d2w"), ("Baz".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 4), "dw"), ("fooBBaz".into(), (0, 4)));
    // `cw` is `ce`: `X` and `B` are subwords of `XBarBaz` (the `B` begins
    // `Bar`), so `.` changes `Bar`.
    assert_eq!(run(CAMEL, (0, 0), "cwX<Esc>"), ("XBarBaz".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 0), "cwX<Esc>w."), ("XXBaz".into(), (0, 1)));
    assert_eq!(run(CAMEL, (0, 0), "c2wX<Esc>"), ("XBaz".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 0), "d2e"), ("Baz".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 0), "de."), ("Baz".into(), (0, 0)));
    assert_eq!(run("fooBarBazQux", (0, 0), "d2e."), ("".into(), (0, 0)));
    assert_eq!(run(CAMEL, (0, 7), "dge"), ("fooBaz".into(), (0, 5)));
    assert_eq!(run(CAMEL, (0, 8), "dge"), ("fooBa".into(), (0, 4)));
    assert_eq!(run(CAMEL, (0, 8), "dge."), ("fo".into(), (0, 1)));
    assert_eq!(run(CAMEL, (0, 8), "db"), ("fooBarz".into(), (0, 6)));
    assert_eq!(run(CAMEL, (0, 8), "db."), ("fooz".into(), (0, 3)));
    let mut vim = subwords(CAMEL, (0, 3));
    vim.feed("ywP").unwrap();
    assert_eq!(vim.text(), "fooBarBarBaz");
    assert_eq!(vim.registers().get('"').unwrap().text, "Bar");

    const SNAKE: &str = "snake_case_name";
    // `w` passes over the `_` as over a blank, which `dw` takes.
    assert_eq!(run(SNAKE, (0, 0), "dw"), ("case_name".into(), (0, 0)));
    assert_eq!(run(SNAKE, (0, 0), "dw."), ("name".into(), (0, 0)));
    assert_eq!(
        run(SNAKE, (0, 0), "cwX<Esc>"),
        ("X_case_name".into(), (0, 0))
    );
    assert_eq!(
        run(SNAKE, (0, 0), "cwX<Esc>w."),
        ("X_X_name".into(), (0, 2))
    );
    assert_eq!(run(SNAKE, (0, 0), "d2e"), ("_name".into(), (0, 0)));
    assert_eq!(run(SNAKE, (0, 6), "dge"), ("snakase_name".into(), (0, 4)));
    assert_eq!(run(SNAKE, (0, 11), "dge"), ("snake_casame".into(), (0, 9)));
    assert_eq!(run(SNAKE, (0, 11), "dge."), ("snakme".into(), (0, 4)));
    assert_eq!(run(SNAKE, (0, 14), "db"), ("snake_case_e".into(), (0, 11)));
    // On the `_`, as on a blank: `dw` takes it, `cw` changes it alone.
    assert_eq!(run(SNAKE, (0, 5), "dw"), ("snakecase_name".into(), (0, 5)));
    assert_eq!(
        run(SNAKE, (0, 5), "cw-<Esc>"),
        ("snake-case_name".into(), (0, 5))
    );

    // The visual modes take the same spans.
    assert_eq!(run(CAMEL, (0, 0), "ved"), ("BarBaz".into(), (0, 0)));
    assert_eq!(run(SNAKE, (0, 0), "vwd"), ("ase_name".into(), (0, 0)));
    assert_eq!(run(SNAKE, (0, 14), "vbd"), ("snake_case_".into(), (0, 10)));
    assert_eq!(run(SNAKE, (0, 14), "vged"), ("snake_cas".into(), (0, 8)));
    assert_eq!(run(CAMEL, (0, 7), "v$bd"), ("fooBarz".into(), (0, 6)));
}

#[test]
fn subword_motions_off_by_default() {
    const TEXT: &str = "fooBar baz_Qux utf8";
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    for (keys, column) in [
        ("w", 7),
        ("e", 13),
        ("b", 7),
        ("ge", 5),
        ("w", 7),
        ("w", 15),
    ] {
        vim.feed(keys).unwrap();
        assert_eq!(vim.cursor(), (0, column), "{keys}");
    }
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("dw").unwrap();
    assert_eq!(vim.text(), "baz_Qux utf8");

    // `W B E gE` are WORD motions on or off.
    for on in [false, true] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.set_subword_motions(on);
        for (keys, column) in [("W", 7), ("E", 13), ("B", 7), ("gE", 5), ("2W", 15)] {
            vim.feed(keys).unwrap();
            assert_eq!(vim.cursor(), (0, column), "{keys} with subwords {on}");
        }
        vim.feed("0dW").unwrap();
        assert_eq!(vim.text(), "baz_Qux utf8");
    }

    // Switched on and off again, they're word motions once more.
    let mut vim = subwords(TEXT, (0, 0));
    vim.feed("w").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    vim.set_subword_motions(false);
    vim.feed("w").unwrap();
    assert_eq!(vim.cursor(), (0, 7));

    // `vim.move` takes a subword motion either way.
    for on in [false, true] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.set_subword_motions(on);
        for (motion, column) in [
            ("subword_forward", 3),
            ("subword_end", 5),
            ("subword_forward", 7),
            ("subword_backward", 3),
            ("subword_end_backward", 2),
        ] {
            let invocation = Invocation::of("vim.move")
                .unwrap()
                .args(serde_json::json!({ "motion": motion }))
                .context(fe_vim::context(vim.state()));
            vim.dispatch(&invocation).unwrap();
            assert_eq!(vim.cursor(), (0, column), "{motion} with subwords {on}");
        }
    }
}
