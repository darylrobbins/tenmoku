use serde_json::json;

use super::{commit, registers};
use crate::{
    HeadlessVim, PendingKind, Step, VimKey, VimState, context,
    macros::{decode, encode},
    parse_keys,
};

/// `vim.recording` on the editor frame.
fn recording(vim: &HeadlessVim) -> Option<String> {
    let stack = context(vim.state());
    let frame = stack.frames().last().unwrap().clone();
    frame.values.get("vim.recording").cloned()
}

/// Register `name`'s text.
fn text(vim: &HeadlessVim, name: char) -> String {
    vim.registers().get(name).map(|r| r.text.clone()).unwrap()
}

/// A buffer of `rows` rows `r0`, `r1`, ….
fn rows(rows: usize) -> String {
    (0..rows)
        .map(|row| format!("r{row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn record_and_play() {
    // `qa` commits `vim.record`, `@a` and `@@` `vim.play`, with a count.
    let record = commit("qa");
    assert_eq!(record.command.as_str(), "vim.record");
    assert_eq!(record.args, json!({"register": "a"}));
    assert_eq!(commit("qA").args, json!({"register": "A"}));
    let play = commit("3@a");
    assert_eq!(play.command.as_str(), "vim.play");
    assert_eq!(play.args, json!({"register": "a", "count": 3}));
    assert_eq!(commit("@@").args, json!({"register": "@"}));
    let mut state = VimState::new();
    for key in ["q", "@"] {
        let key = parse_keys(key).unwrap()[0];
        match state.feed(key) {
            Step::Wait(pending) => assert_eq!(pending.kind(), Some(PendingKind::Register)),
            other => panic!("{other:?}"),
        }
        state.feed(VimKey::Esc);
    }

    // While it records, `vim.recording` is the register; the last `q`
    // isn't recorded, nor does recording touch `"`.
    let mut vim = HeadlessVim::new("abc\ndef\nghi", (0, 0));
    assert_eq!(recording(&vim), None);
    vim.feed("qa").unwrap();
    assert_eq!(recording(&vim).as_deref(), Some("a"));
    assert_eq!(vim.state().recording(), Some('a'));
    vim.feed("xjq").unwrap();
    assert_eq!(recording(&vim), None);
    assert_eq!(text(&vim, 'a'), "xj");
    assert!(!vim.registers().get('a').unwrap().linewise);
    assert_eq!(text(&vim, '"'), "a");

    // Insert-mode keys too, `<Esc>` as nvim stores it.
    let mut vim = HeadlessVim::new("a1\nb2\nc3\nd4\ne5\nf6", (0, 0));
    vim.feed("qaA;<Esc>jq").unwrap();
    assert_eq!(text(&vim, 'a'), "A;\x1bj");
    vim.feed("3@a").unwrap();
    assert_eq!(vim.text(), "a1;\nb2;\nc3;\nd4;\ne5\nf6");
    assert_eq!(vim.cursor(), (4, 1));
    // `@@` plays the last register played.
    vim.feed("@@").unwrap();
    assert_eq!(vim.text(), "a1;\nb2;\nc3;\nd4;\ne5;\nf6");
    // `qA` appends; `"ap` puts the text.
    vim.feed("qAxq").unwrap();
    assert_eq!(text(&vim, 'a'), "A;\x1bjx");
    vim.feed("\"ap").unwrap();
    assert_eq!(vim.text(), "a1;\nb2;\nc3;\nd4;\ne5;\nfA;\x1bjx");

    // The register's text is what plays: a yank into it changes the macro.
    let mut vim = HeadlessVim::new("dd\nb\nc\nd", (0, 0));
    vim.feed("\"ayiwj@a").unwrap();
    assert_eq!(vim.text(), "dd\nc\nd");

    // A macro stops at the key that fails (`j` on the last row), as
    // nvim's error stops it; feeding goes on after it.
    let mut vim = HeadlessVim::new("abc\ndef", (0, 0));
    vim.feed("qaxjxq").unwrap();
    assert_eq!(vim.text(), "bc\nef");
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "bc\nf");
    assert!(vim.state().failed());
    vim.feed("x").unwrap();
    assert_eq!(vim.text(), "bc\n");

    // A recursive macro, set by a yank, ends when `j` fails on the last
    // row, on 1,000 rows too.
    for count in [4, 1000] {
        let mut text = rows(count);
        text.insert_str(0, "xj@a\n");
        let mut vim = HeadlessVim::new(&text, (0, 0));
        vim.feed("\"ay$dd@a").unwrap();
        let expected = (0..count)
            .map(|row| format!("{row}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(vim.text(), expected);
        assert_eq!(vim.cursor(), (count as u32 - 1, 0));
    }

    // A replay that would feed more than 1,000,000 keys stops there, with a
    // message. Each of these plays 600,004: an ex line of blanks, which does
    // nothing, then `@a` again.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    let line = format!(":{}\r@a", " ".repeat(600_000));
    vim.registers_mut().record('a', line);
    vim.feed("@a").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("macro stopped: it would replay more than 1000000 keys")
    );
    assert_eq!(vim.mode(), "normal");
    vim.feed("x").unwrap();
    assert_eq!(vim.text(), "bc");

    // `take_replay` hands over the keys, once; `HeadlessVim` feeds them.
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    vim.feed("qaxlq").unwrap();
    let invocation = commit("2@a").context(context(vim.state()));
    vim.dispatch(&invocation).unwrap();
    assert_eq!(vim.text(), "bcdef");
    let keys = vim.state_mut().take_replay();
    assert_eq!(keys, parse_keys("xlxl").unwrap());
    assert!(vim.state_mut().take_replay().is_empty());
    vim.state_mut().stop_replay();
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "bdef");
}

#[test]
fn recording_every_mode() {
    // Visual and operator-pending keys, the command line, and in insert
    // mode `<CR>`, and `<BS>` in vim notation.
    let mut vim = HeadlessVim::new("abc def\nabc", (0, 0));
    vim.feed("qavlddw:s/e/X/<CR>Ax<CR>y<BS><Esc>q").unwrap();
    assert_eq!(text(&vim, 'a'), "vlddw:s/e/X/\rAx\ry<BS>\x1b");
    assert_eq!(vim.text(), "dXfx\n\nabc");
    // `q` in a visual mode stops it there, which stays on.
    vim.feed("qbvq").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!(text(&vim, 'b'), "v");
    // A replay's keys aren't recorded: `@a` is.
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    vim.feed("qaxqqb@aq").unwrap();
    assert_eq!(text(&vim, 'b'), "@a");
    assert_eq!(vim.text(), "cdef");
    // `qA` onto lines appends to the last; onto a block, it's charwise.
    let mut vim = HeadlessVim::new("ab\ncd", (0, 0));
    vim.feed("\"ayyqAxq").unwrap();
    let a = vim.registers().get('a').unwrap();
    assert_eq!((a.text.as_str(), a.linewise), ("abx\n", true));
    let mut vim = HeadlessVim::new("ab\ncd", (0, 0));
    vim.feed("<C-v>j\"ayqAxq").unwrap();
    let a = vim.registers().get('a').unwrap();
    assert_eq!((a.text.as_str(), a.block_width), ("a\ncx", None));
    // `qaq` empties `a`; playing it fails, doing nothing.
    vim.feed("qaq@a").unwrap();
    assert_eq!(text(&vim, 'a'), "");
    assert!(vim.state().failed());
    assert_eq!(vim.text(), "b\ncd");
    // `@@` before any `@`: E748.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("@@").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("E748: No previously used register")
    );
}

#[test]
fn replays_stop_where_nvim_stops() {
    // `<Esc>` cancelling, alone or after an operator, isn't an error: the
    // macro goes on. Other cancels and failed searches stop it.
    for (macro_keys, expected) in [
        ("x<Esc>x", "cdef"),
        ("xd<Esc>x", "cdef"),
        ("xdJx", "bcdef"),
        ("xZXx", "bcdef"),
        ("x/zzz<CR>x", "bcdef"),
        // Nothing to redo, repeat or reselect, as nvim fails them.
        ("<C-r>x", "abcdef"),
        ("l.x", "abcdef"),
        ("gvx", "abcdef"),
        // A `w` that can't move fails; `t` onto the next character doesn't.
        ("$wx", "abcdef"),
        ("tbx", "bcdef"),
        // A visual text object that selects nothing, or runs out part way.
        ("vi(x", "abcdef"),
        ("v2iwx", "abcdef"),
        // Running out of undo, even part way.
        ("x2ux", "abcdef"),
        // From a row's end, `b` moves.
        ("v$bd", "bcdef"),
        // `aw` back from the text's start selects nothing more.
        ("lvhawx", "abcdef"),
        ("lvhaWx", "abcdef"),
    ] {
        let mut vim = HeadlessVim::new("abcdef", (0, 0));
        let mut text = String::new();
        for key in parse_keys(macro_keys).unwrap() {
            encode(key, &mut text);
        }
        vim.registers_mut().record('a', text);
        vim.feed("@a").unwrap();
        assert_eq!(vim.text(), expected, "{macro_keys}");
    }
    // `~` on an empty row fails.
    let mut vim = HeadlessVim::new("\nab", (0, 0));
    vim.registers_mut().record('a', "~jx".into());
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "\nab");
    // A replay's `<Esc>` on the command line runs it, as in nvim.
    let mut vim = HeadlessVim::new("abc\nabc", (0, 0));
    vim.registers_mut().record('a', ":s/a/X/\x1bj".into());
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "Xbc\nabc");
    assert_eq!(vim.cursor(), (1, 0));
    // `.` after `@a` repeats the macro's last change.
    let mut vim = HeadlessVim::new("a\nb\nc", (0, 0));
    vim.feed("qaA;<Esc>jq@aj.").unwrap();
    assert_eq!(vim.text(), "a;\nb;\nc;");
}

#[test]
fn keys_as_register_text() {
    // Bytes where nvim stores one, vim notation where it doesn't; a line
    // break plays as `<CR>`.
    let keys = parse_keys("a<Esc><CR><Tab><C-v><C-r><BS><Left><Right><Up><Down><lt>é").unwrap();
    let mut text = String::new();
    for &key in &keys {
        encode(key, &mut text);
    }
    assert_eq!(text, "a\x1b\r\t\x16\x12<BS><Left><Right><Up><Down><é");
    assert_eq!(decode(&text, usize::MAX), Some(keys));
    assert_eq!(
        decode("dd\n<<<Esc>", 10),
        Some(parse_keys("dd<C-j><lt><lt><lt>Esc>").unwrap())
    );
    assert_eq!(decode("dd\n<<<Esc>", 9), None);
}

#[test]
fn replays_end_cleanly() {
    // A replay's `q` does nothing: it neither starts nor stops recording.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.registers_mut().record('a', "qx".into());
    vim.feed("@a").unwrap();
    assert_eq!((vim.text().as_str(), vim.state().recording()), ("bc", None));
    vim.feed("qb@axq").unwrap();
    assert_eq!(vim.text(), "");
    assert_eq!(text(&vim, 'b'), "@ax");
    // After a count or a register too, which it drops; recording or not.
    for recording in ["", "qc"] {
        for played in ["2qx", "\"bqx"] {
            let mut vim = HeadlessVim::new("abc", (0, 1));
            vim.registers_mut().record('a', played.into());
            vim.feed(&format!("{recording}@a")).unwrap();
            assert_eq!(vim.text(), "ac", "{recording}{played}");
            assert_eq!(vim.state().recording().is_some(), !recording.is_empty());
            assert!(vim.state().pending().kind().is_none());
        }
    }

    // Once the host has fed a replay, a `@` dispatched directly starts a
    // new one, with the whole budget; so does one after `stop_replay`.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.registers_mut()
        .record('a', format!(":{}\r", " ".repeat(600_000)));
    for _ in 0..2 {
        let play = commit("@a").context(context(vim.state()));
        vim.dispatch(&play).unwrap();
        let keys = vim.state_mut().take_replay();
        assert_eq!(keys.len(), 600_002);
        for key in keys {
            let typed: String = match key {
                VimKey::Char(ch) => ch.into(),
                _ => "<CR>".into(),
            };
            vim.feed(&typed).unwrap();
        }
        assert!(vim.state_mut().take_replay().is_empty());
        assert_eq!(vim.state().message(), None);
    }
    let play = commit("@a").context(context(vim.state()));
    vim.dispatch(&play).unwrap();
    assert_eq!(vim.state_mut().take_replay().len(), 600_002);
    vim.state_mut().stop_replay();
    vim.dispatch(&play).unwrap();
    assert_eq!(vim.state_mut().take_replay().len(), 600_002);
    vim.state_mut().stop_replay();

    // A register far past the cap is refused without decoding it all.
    vim.registers_mut().record('b', "x".repeat(50_000_000));
    vim.feed("@b").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("macro stopped: it would replay more than 1000000 keys")
    );
    assert_eq!(vim.text(), "abc");

    // A linewise register's line breaks play as `<NL>`, which moves down.
    let mut vim = HeadlessVim::new("dd\none\ntwo\nthree\nfour", (0, 0));
    vim.feed("\"ayyj2@a").unwrap();
    assert_eq!(vim.text(), "dd\ntwo\nfour");
}

#[test]
fn bounded_bookkeeping() {
    // 100,000 keys recorded are one register's text, and nothing else: no
    // key is kept apart from it.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    // As a host records the text it types itself.
    vim.feed("qai").unwrap();
    for _ in 0..100_000 {
        vim.state_mut().record_key(VimKey::Char('x'));
    }
    vim.feed("<Esc>q").unwrap();
    let a = vim.registers().get('a').unwrap();
    assert_eq!(a.text.len(), 100_002);
    assert!(a.text.capacity() < 2 * 100_002, "{}", a.text.capacity());
    assert_eq!(registers(&vim).len(), 1);
    assert_eq!(vim.state().recording(), None);

    // A count of 4294967295 is stopped by the cap at once.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("qaxq4294967295@a").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("macro stopped: it would replay more than 1000000 keys")
    );
    assert_eq!(vim.text(), "bc");
    assert!(vim.state_mut().take_replay().is_empty());
}
