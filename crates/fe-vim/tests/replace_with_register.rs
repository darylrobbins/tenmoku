//! `gr{motion}`, `grr` and visual `gr` as the ReplaceWithRegister plugin
//! (v1.42) documents and implements them (`doc/ReplaceWithRegister.txt`,
//! `autoload/ReplaceWithRegister.vim`). nvim has no ReplaceWithRegister to
//! record an oracle against: each case's expectation is worked out from the
//! plugin's code, which selects what the operator covered (`` g`[vg`] ``,
//! or `V` over rows) and runs `"xp` with `"` and 'clipboard' restored, the
//! register's text first corrected by `s:CorrectForRegtype`. Where no
//! correction applies, the case also runs the plugin's equivalent built-in
//! keys, visual `"xP` (which writes no register), on a twin, whose
//! built-ins the oracle corpora check against nvim.

use fe_command::{ContextFrame, Invocation};
use fe_vim::{
    HeadlessVim, HostCommand, OperateArgs, Operator, Register, Target, TextObject, Write,
};

const TEXT: &str = "alpha beta gamma\n  delta epsilon\nzeta\n\neta theta";

/// The registers a replace must leave alone: `"`, `-`, `0`-`9`, `a`, `b`
/// and the clipboard (`+`).
#[derive(Debug, PartialEq)]
struct Held {
    named: Vec<(char, Option<Register>)>,
    clipboard: Option<String>,
}

fn held(vim: &HeadlessVim) -> Held {
    let names = [
        '"', '-', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b',
    ];
    Held {
        named: names
            .into_iter()
            .map(|name| (name, vim.registers().get(name).cloned()))
            .collect(),
        clipboard: vim.host().clipboard.clone(),
    }
}

fn write(vim: &mut HeadlessVim, name: Option<char>, how: Write, value: Register) {
    let mut host = vim.host().clone();
    vim.registers_mut()
        .write(name, how, value, &mut host)
        .expect("a writable register");
    *vim.host_mut() = host;
}

/// What a case's register holds.
#[derive(Clone, Copy)]
enum Reg {
    Chars(&'static str),
    Lines(&'static str),
}

impl Reg {
    fn value(self) -> Register {
        match self {
            Reg::Chars(text) => Register::new(text, false),
            Reg::Lines(text) => Register::new(text, true),
        }
    }
}

/// A vim on `text` at `cursor` with ReplaceWithRegister on and every
/// register `held` reads set: `a` to `"AA"`, `b` to `"BB"`, `-` and `1` to
/// deletes, and `"` and `0` to `unnamed`; with `clipboard`, as
/// `clipboard=unnamedplus` (so `"` reads `+`, holding `unnamed`). A jump to
/// the last row and back leaves the jumplist with entries.
fn vim(text: &str, cursor: (u32, usize), unnamed: Reg, clipboard: bool) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.set_replace_with_register(true);
    vim.feed("G``").unwrap();
    assert_eq!(vim.cursor(), cursor, "the jump setup returns");
    write(&mut vim, Some('a'), Write::Yank, Register::new("AA", false));
    write(&mut vim, Some('b'), Write::Yank, Register::new("BB", false));
    let small = Write::Delete {
        small: true,
        numbered: false,
    };
    write(&mut vim, None, small, Register::new("sm", false));
    let big = Write::Delete {
        small: false,
        numbered: true,
    };
    write(&mut vim, None, big, Register::new("big\n", true));
    vim.set_clipboard_unnamed(clipboard);
    write(&mut vim, None, Write::Yank, unnamed.value());
    vim
}

/// The jumplist as `:jumps` lists it, each entry's row and column.
fn jumps(vim: &mut HeadlessVim) -> Vec<Vec<String>> {
    vim.feed(":jumps<CR>").unwrap();
    let Some(HostCommand::Pick(pick)) = vim.state_mut().take_host_command() else {
        panic!(":jumps lists nothing");
    };
    pick.rows
        .iter()
        .map(|row| row.cells.iter().take(3).cloned().collect())
        .collect()
}

/// One `gr` case: keys on `text` at `cursor` with `"` holding `reg`.
struct Case {
    /// The plugin's equivalent keys, as worked out from its code.
    plugin: &'static str,
    text: &'static str,
    cursor: (u32, usize),
    reg: Reg,
    keys: &'static str,
    after: &'static str,
    caret: (u32, usize),
    /// The equivalent built-in keys on a twin, when no correction applies
    /// (`None`: the case is corrected).
    twin: Option<&'static str>,
    clipboard: bool,
}

const fn case(
    plugin: &'static str,
    (text, cursor): (&'static str, (u32, usize)),
    reg: Reg,
    keys: &'static str,
    (after, caret): (&'static str, (u32, usize)),
    twin: Option<&'static str>,
) -> Case {
    Case {
        plugin,
        text,
        cursor,
        reg,
        keys,
        after,
        caret,
        twin,
        clipboard: false,
    }
}

/// Runs `case`: its text, caret, registers and jumplist after the keys;
/// one `u` putting the text back; `.` after that doing it again with the
/// same register; and its twin's keys leaving the same text, caret and
/// registers.
fn check(case: &Case) {
    let what = format!("{} ({})", case.keys, case.plugin);
    let mut vim = vim(case.text, case.cursor, case.reg, case.clipboard);
    let before = held(&vim);
    let jumped = jumps(&mut vim);
    vim.feed(case.keys).unwrap();
    assert_eq!(
        (vim.text(), vim.cursor()),
        (case.after.into(), case.caret),
        "{what}"
    );
    assert_eq!(vim.mode(), "normal", "{what}");
    assert_eq!(held(&vim), before, "{what}: the registers");
    // No jump is added (the entries' rows follow the text's edits).
    let jumps_after = jumps(&mut vim);
    assert_eq!(jumps_after.len(), jumped.len(), "{what}: the jumplist");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), case.text, "{what}: u");
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), case.after, "{what}: .");
    assert_eq!(held(&vim), before, "{what}: the registers after .");
    if let Some(twin_keys) = case.twin {
        let mut twin = self::vim(case.text, case.cursor, case.reg, case.clipboard);
        twin.feed(twin_keys).unwrap();
        let mut vim = self::vim(case.text, case.cursor, case.reg, case.clipboard);
        vim.feed(case.keys).unwrap();
        assert_eq!(
            (vim.text(), vim.cursor(), held(&vim)),
            (twin.text(), twin.cursor(), held(&twin)),
            "{what}: against {twin_keys}"
        );
        assert_eq!(jumps(&mut vim), jumps(&mut twin), "{what}: the jumplist");
    }
}

#[test]
fn replace_with_register_operator() {
    let x = Reg::Chars("X");
    let quotes = "say \"\" now";
    let cases = [
        // `w` covers the word and the blank after it.
        case(
            "g`[vg`]p over \"beta \"",
            (TEXT, (0, 6)),
            x,
            "grw",
            ("alpha Xgamma\n  delta epsilon\nzeta\n\neta theta", (0, 6)),
            Some("v4lP"),
        ),
        case(
            "g`[vg`]p over \"beta gamma\"",
            (TEXT, (0, 6)),
            x,
            "gr$",
            ("alpha X\n  delta epsilon\nzeta\n\neta theta", (0, 6)),
            Some("veeP"),
        ),
        // The count goes to the motion: the register goes in once.
        case(
            "g`[vg`]p over \"alpha beta \"",
            (TEXT, (0, 0)),
            x,
            "2grw",
            ("Xgamma\n  delta epsilon\nzeta\n\neta theta", (0, 0)),
            Some("v10lP"),
        ),
        case(
            "g`[vg`]p over \"alpha beta \"",
            (TEXT, (0, 0)),
            x,
            "gr2w",
            ("Xgamma\n  delta epsilon\nzeta\n\neta theta", (0, 0)),
            Some("v10lP"),
        ),
        case(
            "g`[vg`]p over \"beta\"",
            (TEXT, (0, 7)),
            x,
            "griw",
            ("alpha X gamma\n  delta epsilon\nzeta\n\neta theta", (0, 6)),
            Some("viwP"),
        ),
        // An empty object: `'[` after `']`, so `P` at the caret, which
        // `g@` left at `'[`.
        case(
            "P with '[ after ']",
            (quotes, (0, 4)),
            x,
            "gri\"",
            ("say \"X\" now", (0, 5)),
            Some("lP"),
        ),
        // Over rows (`g@` linewise): `V`, characters going on a row of
        // their own.
        case(
            "g`[Vg`]p over rows 1-3",
            (TEXT, (0, 3)),
            x,
            "grip",
            ("X\n\neta theta", (0, 0)),
            Some("VjjP"),
        ),
        // A named register.
        case(
            "g`[vg`]\"ap over \"beta \"",
            (TEXT, (0, 6)),
            x,
            "\"agrw",
            ("alpha AAgamma\n  delta epsilon\nzeta\n\neta theta", (0, 7)),
            Some("v4l\"aP"),
        ),
        // A linewise register over characters goes in characterwise,
        // without its line break: no row is added.
        case(
            "corrected to v \"LINE\", g`[vg`]p over \"beta \"",
            (TEXT, (0, 6)),
            Reg::Lines("LINE\n"),
            "grw",
            (
                "alpha LINEgamma\n  delta epsilon\nzeta\n\neta theta",
                (0, 9),
            ),
            None,
        ),
        case(
            "corrected to v \"one\\ntwo\", g`[vg`]p over \"beta \"",
            (TEXT, (0, 6)),
            Reg::Lines("one\ntwo\n"),
            "grw",
            (
                "alpha one\ntwogamma\n  delta epsilon\nzeta\n\neta theta",
                (0, 6),
            ),
            None,
        ),
        // `grr`: `V{count}_` then `gvp`, characters on a row of their own.
        case(
            "V_ then gvp",
            (TEXT, (1, 5)),
            x,
            "grr",
            ("alpha beta gamma\nX\nzeta\n\neta theta", (1, 0)),
            Some("VP"),
        ),
        case(
            "V3_ then gvp",
            (TEXT, (0, 2)),
            x,
            "3grr",
            ("X\n\neta theta", (0, 0)),
            Some("VjjP"),
        ),
        case(
            "V_ then gvp",
            (TEXT, (1, 5)),
            x,
            "grgr",
            ("alpha beta gamma\nX\nzeta\n\neta theta", (1, 0)),
            Some("VP"),
        ),
        // A linewise register over rows is corrected too, its lines then
        // taking the rows.
        case(
            "corrected to v \"one\\ntwo\", V_ then gvp",
            (TEXT, (1, 5)),
            Reg::Lines("one\ntwo\n"),
            "grr",
            ("alpha beta gamma\none\ntwo\nzeta\n\neta theta", (1, 0)),
            None,
        ),
    ];
    for case in &cases {
        check(case);
    }
    // With `clipboard=unnamedplus`, `"` is the clipboard, which stays.
    let mut clipboard = case(
        "g`[vg`]\"+p over \"beta \" with 'clipboard' emptied",
        (TEXT, (0, 6)),
        x,
        "grw",
        ("alpha Xgamma\n  delta epsilon\nzeta\n\neta theta", (0, 6)),
        Some("v4lP"),
    );
    clipboard.clipboard = true;
    check(&clipboard);
    // From a numbered register, `.` reads the same one (repeat.vim's), not
    // the next as a put's `.` does: over characters and over rows.
    for (keys, after) in [
        (
            "\"1grwu.",
            "alpha N1gamma\n  delta epsilon\nzeta\n\neta theta",
        ),
        ("\"1grru.", "N1\n  delta epsilon\nzeta\n\neta theta"),
        ("\"1grrj.", "N1\nN1\nzeta\n\neta theta"),
    ] {
        let mut vim = numbered(TEXT, (0, 6));
        let before = held(&vim);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), after, "{keys}");
        assert_eq!(held(&vim), before, "{keys}: the registers");
    }
}

/// A visual `gr` case: the keys, then the text and caret.
fn visual_case(
    (text, cursor): (&str, (u32, usize)),
    reg: Reg,
    keys: &str,
    (after, caret): (&str, (u32, usize)),
) -> HeadlessVim {
    let mut vim = vim(text, cursor, reg, false);
    let before = held(&vim);
    vim.feed(keys).unwrap();
    assert_eq!((vim.text(), vim.cursor()), (after.into(), caret), "{keys}");
    assert_eq!(vim.mode(), "normal", "{keys}");
    assert_eq!(held(&vim), before, "{keys}: the registers");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), text, "{keys}: u");
    vim
}

#[test]
fn replace_with_register_visual() {
    let x = Reg::Chars("X");
    // `gvp` over the selection.
    let viw = ("alpha X gamma\n  delta epsilon\nzeta\n\neta theta", (0, 6));
    visual_case((TEXT, (0, 7)), x, "viwgr", viw);
    let rows = ("alpha beta gamma\nX\nzeta\n\neta theta", (1, 0));
    visual_case((TEXT, (1, 5)), x, "Vgr", rows);
    let two = ("alpha beta gamma\nX\n\neta theta", (1, 0));
    visual_case((TEXT, (1, 5)), x, "Vjgr", two);
    // The same as the built-in visual `P`.
    for (keys, twin) in [("viwgr", "viwP"), ("Vgr", "VP"), ("Vjgr", "VjP")] {
        let mut vim = vim(TEXT, (1, 5), x, false);
        vim.feed(keys).unwrap();
        let mut built_in = self::vim(TEXT, (1, 5), x, false);
        built_in.feed(twin).unwrap();
        assert_eq!(
            (vim.text(), vim.cursor(), held(&vim)),
            (built_in.text(), built_in.cursor(), held(&built_in)),
            "{keys} against {twin}"
        );
    }
    // A linewise register over characters, corrected.
    visual_case(
        (TEXT, (0, 7)),
        Reg::Lines("LINE\n"),
        "viwgr",
        (
            "alpha LINE gamma\n  delta epsilon\nzeta\n\neta theta",
            (0, 9),
        ),
    );
    // A block: a one-row register goes in on each row, as a block of that
    // row repeated; a linewise one of several rows as a block.
    let grid = "abcd\nefgh\nijkl";
    visual_case((grid, (0, 1)), x, "<C-v>jlgr", ("aXd\neXh\nijkl", (0, 1)));
    visual_case(
        (grid, (0, 1)),
        Reg::Lines("XY\n"),
        "<C-v>jjgr",
        ("aXYcd\neXYgh\niXYkl", (0, 1)),
    );
    visual_case(
        (grid, (0, 1)),
        Reg::Lines("one\ntwo\n"),
        "<C-v>jlgr",
        ("aoned\netwoh\nijkl", (0, 1)),
    );
    // As the plugin's `setreg(x, '', "a\<C-v>")` leaves them (each case
    // as nvim 0.12.4 runs the plugin's correction and `gv"xp`): the
    // register's rows, an empty one after them, 0 columns wide, so no copy
    // is padded, and past the text's end the empty row is a new one.
    visual_case(
        (grid, (0, 1)),
        Reg::Lines("on\ntwo\n"),
        "<C-v>jlgr",
        ("aond\netwoh\nijkl", (0, 1)),
    );
    visual_case(
        ("abcd\nefgh", (0, 1)),
        Reg::Lines("one\ntwo\n"),
        "<C-v>jlgr",
        ("aoned\netwoh\n ", (0, 1)),
    );
    // Blank rows within and at the ends of a linewise register stay.
    visual_case(
        (grid, (0, 1)),
        Reg::Lines("\none\n\ntwo\n"),
        "<C-v>jlgr",
        ("ad\neoneh\nijkl\n two\n ", (0, 1)),
    );
    // Characters repeated on each row: Vim's `split()` leaves out an empty
    // first or last line, not one within.
    visual_case(
        (grid, (0, 1)),
        Reg::Chars("a\n\nb"),
        "<C-v>jlgr",
        ("aad\ne h\nibjkl\n a\n \n b", (0, 1)),
    );
    visual_case(
        (grid, (0, 1)),
        Reg::Chars("\nX\n"),
        "<C-v>jlgr",
        ("aXd\neXh\nijkl", (0, 1)),
    );
    // `.` after a block `gr`: as many columns from the caret's, however
    // short the rows it reaches, a tab (4 columns here) split at its edge.
    for (text, keys, after) in [
        ("abcd\nefgh\nijkl\nm", "<C-v>jlgrjj.", "aXd\neXh\niXl\nmX"),
        (
            "abcdefghijk\nefgh\nab\tijkl\nmnopqrstuv",
            "<C-v>jlgrjj.",
            "aXdefghijk\neXh\naX ijkl\nmXpqrstuv",
        ),
        ("abcd\nefgh\nijkl\nm", "<C-v>j$grjj.", "aX\neX\niX\nmX"),
    ] {
        let mut vim = vim(text, (0, 1), x, false);
        let before = held(&vim);
        vim.feed(keys).unwrap();
        assert_eq!((vim.text(), vim.cursor()), (after.into(), (2, 1)), "{keys}");
        assert_eq!(held(&vim), before, "{keys}: the registers");
    }
    // A register named before `v` is dropped as nvim drops it, so the
    // plugin's `v:register` is `"`; named in the visual mode, it's read.
    visual_case((TEXT, (0, 7)), x, "\"bviwgr", viw);
    visual_case(
        (TEXT, (0, 7)),
        x,
        "viw\"bgr",
        ("alpha BB gamma\n  delta epsilon\nzeta\n\neta theta", (0, 7)),
    );
    // `.` replaces as much text again at the caret, with the same register.
    let mut vim = vim("one two six ten", (0, 0), x, false);
    let before = held(&vim);
    vim.feed("viw\"bgrw.").unwrap();
    assert_eq!(vim.text(), "BB BB six ten");
    assert_eq!(held(&vim), before, ". after visual gr: the registers");
    vim.feed("w.").unwrap();
    assert_eq!(vim.text(), "BB BB BB ten");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "BB BB six ten");
    // From a numbered register, `.` reads the same one (repeat.vim's), not
    // the next as a put's `.` does.
    let mut vim = numbered("one two six ten", (0, 0));
    vim.feed("viw\"1grw.").unwrap();
    assert_eq!(vim.text(), "N1 N1 six ten", ". after visual \"1gr");
}

/// A vim whose `1` and `2` hold `N1` and `N2`, charwise.
fn numbered(text: &str, cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = vim(text, cursor, Reg::Chars("X"), false);
    let big = Write::Delete {
        small: false,
        numbered: true,
    };
    write(&mut vim, None, big, Register::new("N2", false));
    write(&mut vim, None, big, Register::new("N1", false));
    vim
}

#[test]
fn replace_with_register_refuses_a_block_too_big() {
    // One megabyte on each of 100 rows is past what a put may insert: the
    // correction is refused before it's built, and nothing changes.
    let text = vec!["ab"; 100].join("\n");
    let mut vim = vim(&text, (0, 0), Reg::Chars("X"), false);
    write(
        &mut vim,
        Some('a'),
        Write::Yank,
        Register::new("x".repeat(1 << 20), false),
    );
    let before = held(&vim);
    let _ = vim.feed("<C-v>99j\"agr");
    assert_eq!(vim.text(), text);
    assert_eq!(vim.mode(), "visual_block");
    assert_eq!(held(&vim), before);
}

fn flagged(vim: &HeadlessVim) -> bool {
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    frame.flags.contains("vim.replace_with_register")
}

fn operate(vim: &mut HeadlessVim, target: Target) {
    let args = OperateArgs {
        operator: Operator::ReplaceWithRegister,
        target,
        count: None,
        register: None,
    };
    let invocation = Invocation::of("vim.operate")
        .unwrap()
        .args(serde_json::to_value(args).unwrap())
        .context(fe_vim::context(vim.state()));
    vim.dispatch(&invocation).unwrap();
}

#[test]
fn replace_with_register_is_opt_in() {
    let x = Reg::Chars("X");
    for keys in ["grw", "grr", "viwgr"] {
        let mut vim = vim(TEXT, (0, 7), x, false);
        vim.set_replace_with_register(false);
        assert!(!flagged(&vim), "{keys}: off, no flag");
        // An `x` before, which one `u` then undoes: `gr` left no step.
        vim.feed("x").unwrap();
        let deleted = vim.text();
        let before = held(&vim);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), deleted, "{keys} off");
        assert_eq!(held(&vim), before, "{keys} off: the registers");
        vim.feed("<Esc>u").unwrap();
        assert_eq!(vim.text(), TEXT, "{keys} off: u undoes the x");
    }
    // Off by default.
    let fresh = HeadlessVim::new(TEXT, (0, 0));
    assert!(!flagged(&fresh), "off by default");
    let mut on = vim(TEXT, (0, 7), x, false);
    assert!(flagged(&on), "on: the flag");
    on.set_replace_with_register(false);
    assert!(!flagged(&on), "off again: no flag");
    // `vim.operate` replaces either way.
    for setting in [false, true] {
        let mut vim = vim(TEXT, (0, 7), x, false);
        vim.set_replace_with_register(setting);
        operate(&mut vim, Target::TextObject(TextObject::InnerWord));
        assert_eq!(
            vim.text(),
            "alpha X gamma\n  delta epsilon\nzeta\n\neta theta",
            "vim.operate, setting {setting}"
        );
        vim.feed("viw").unwrap();
        vim.set_replace_with_register(setting);
        operate(&mut vim, Target::Selection);
        assert_eq!(
            vim.text(),
            "alpha X gamma\n  delta epsilon\nzeta\n\neta theta",
            "vim.operate over a selection, setting {setting}"
        );
    }
}
