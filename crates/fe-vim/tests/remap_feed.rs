//! Remaps run by `VimState::feed`, in front of the reducer
//! (`docs/decisions/vim-remaps.md` §3–§5), through `HeadlessVim`: where a
//! key is matched, prefixes and their timeout, recursion, and a set
//! replaced while keys are held.

use std::sync::Arc;

use fe_command::ContextFrame;
use fe_editor::commands::HeadlessHost;
use fe_vim::{HeadlessVim, Pending, Register, Remap, RemapMode, RemapSet, VimKey, Write};

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;
const O: RemapMode = RemapMode::OperatorPending;

fn noremap(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

fn map(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        recursive: true,
        ..noremap(modes, from, to)
    }
}

fn set(remaps: &[Remap]) -> Arc<RemapSet> {
    let (set, issues) = RemapSet::new(remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    Arc::new(set)
}

fn vim(text: &str, cursor: (u32, usize), remaps: &[Remap]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.state_mut().set_remaps(set(remaps));
    vim
}

/// `key` in vim notation.
fn notation(key: VimKey) -> String {
    match key {
        VimKey::Char('<') => "<lt>".into(),
        VimKey::Char(ch) => ch.into(),
        VimKey::Ctrl(ch) => format!("<C-{ch}>"),
        VimKey::Esc => "<Esc>".into(),
        VimKey::Enter => "<CR>".into(),
        VimKey::Backspace => "<BS>".into(),
        VimKey::Tab => "<Tab>".into(),
        other => panic!("no notation for {other:?}"),
    }
}

/// What a host does once `CHORD_TIMEOUT` passes with no key: tells the
/// state, then feeds the keys it resolved to.
fn time_out(vim: &mut HeadlessVim) {
    vim.state_mut().remap_timeout();
    for key in vim.state_mut().take_replay() {
        vim.feed(&notation(key)).unwrap();
    }
}

/// The text and cursor after `keys`, from `cursor` in `text`.
fn after(text: &str, cursor: (u32, usize), remaps: &[Remap], keys: &str) -> (String, (u32, usize)) {
    let mut vim = vim(text, cursor, remaps);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

const INDENTED: &str = "  one two\n  three four\n  five six";

#[test]
fn remap_feed_matches_where_nvim_does() {
    let h = [noremap(&[N], "H", "^")];
    let moved = |keys| after(INDENTED, (1, 6), &h, keys);
    // After a count or a register too: `^` takes the count and ignores it.
    for keys in ["H", "3H", "\"aH"] {
        assert_eq!(moved(keys), (INDENTED.into(), (1, 2)), "{keys}");
    }
    // Unmapped, `3H` is the window's third row, the column kept.
    assert_eq!(after(INDENTED, (1, 6), &[], "3H").1, (2, 6));
    // After an operator, `nnoremap` doesn't apply: `dH` deletes to the
    // window's top row, linewise.
    assert_eq!(moved("dH"), ("  five six".into(), (0, 6)));
    // `onoremap` does.
    let o = [noremap(&[N], "H", "^"), noremap(&[O], "H", "^")];
    assert_eq!(
        after(INDENTED, (1, 6), &o, "dH"),
        ("  one two\n  e four\n  five six".into(), (1, 2))
    );

    // `xnoremap` applies in the visual modes only.
    let x = [noremap(&[X], "H", "^")];
    assert_eq!(after(INDENTED, (1, 6), &x, "H").1, (0, 6));
    for (keys, mode) in [
        ("v", "visual"),
        ("V", "visual_line"),
        ("<C-v>", "visual_block"),
    ] {
        let mut vim = vim(INDENTED, (1, 6), &x);
        vim.feed(keys).unwrap();
        vim.feed("H").unwrap();
        assert_eq!((vim.mode(), vim.cursor()), (mode, (1, 2)), "{keys}");
        assert_eq!(vim.visual_anchor(), Some((1, 6)), "{keys}");
    }

    // A prefix's argument is read unmapped, after an operator too.
    let dd = [noremap(&[N, O], "x", "dd")];
    let text = "ax bx cx\nline two";
    for (keys, want) in [
        ("fx", (text, (0, 1))),
        ("2tx", (text, (0, 3))),
        ("$Fx", (text, (0, 4))),
        ("$Tx", (text, (0, 5))),
        ("dfx", (" bx cx\nline two", (0, 0))),
        ("rx", ("xx bx cx\nline two", (0, 0))),
        ("jmxk'x", (text, (1, 0))),
        ("jlmxk`x", (text, (1, 1))),
        ("\"xyl", (text, (0, 0))),
        ("qxq", (text, (0, 0))),
    ] {
        assert_eq!(
            after(text, (0, 0), &dd, keys),
            (want.0.into(), want.1),
            "{keys}"
        );
    }
    let mut played = vim(text, (0, 0), &dd);
    played.feed("\"xyl").unwrap();
    assert_eq!(played.registers().get('x').unwrap().text, "a");
    played.feed("@x").unwrap();
    // The register's `a` appended: the `x` after `@` wasn't `dd`.
    assert_eq!((played.text(), played.mode()), (text.into(), "insert"));

    // The second key of `g`, `z`, `Z`, `[` and `]` is read unmapped.
    let seconds = [
        noremap(&[N], "j", "dd"),
        noremap(&[N], "t", "dd"),
        noremap(&[N], "Q", "dd"),
        noremap(&[N], "c", "dd"),
    ];
    let lines = "a (b\nc\nd) e\nf";
    assert_eq!(after(lines, (0, 0), &seconds, "gj"), (lines.into(), (1, 0)));
    // `[c` and `]c` (no diff here to move through): mapped, `c`'s `dd`
    // would leave `d` pending.
    for keys in ["[c", "]c"] {
        let mut vim = vim(lines, (1, 0), &seconds);
        vim.feed(keys).unwrap();
        assert_eq!((vim.text(), vim.mode()), (lines.into(), "normal"), "{keys}");
        assert_eq!(*vim.state().pending(), Pending::new(), "{keys}");
    }
    let mut scrolled = vim(lines, (2, 0), &seconds);
    scrolled.feed("zt").unwrap();
    assert_eq!((scrolled.text(), scrolled.top_row()), (lines.into(), 2));
    let mut quit = vim(lines, (0, 0), &seconds);
    quit.feed("ZQ").unwrap();
    assert_eq!(quit.text(), lines);
    assert!(quit.state_mut().take_host_command().is_some());
}

#[test]
fn remap_feed_resolves_prefixes() {
    let jk = [noremap(&[N], "jk", "x"), noremap(&[N], "h", "l")];
    let text = "abc\ndef\nghi";
    // `j` breaks off `jk`: built-in `j`, then `h`, matched again, is `l`.
    assert_eq!(after(text, (0, 1), &jk, "jh"), (text.into(), (1, 2)));
    assert_eq!(
        after(text, (0, 1), &jk, "jk"),
        ("ac\ndef\nghi".into(), (0, 1))
    );

    // An exact map that is also a prefix waits.
    let q = [
        noremap(&[N], "Q", "x"),
        noremap(&[N], "Qw", "dw"),
        noremap(&[N], "jk", "x"),
    ];
    let words = "one two\nthree four\nfive";
    let mut vim = vim(words, (0, 0), &q);
    vim.feed("Q").unwrap();
    assert!(vim.state().remap_waiting());
    assert_eq!((vim.text(), vim.mode()), (words.into(), "normal"));
    assert_eq!(vim.state().mode_name(), "normal");
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    assert!(frame.has("vim.pending"));
    assert_eq!(frame.get("vim.pending_kind"), Some("remap"));
    assert_eq!(frame.get("vim.mode"), Some("normal"));
    vim.feed("w").unwrap();
    assert!(!vim.state().remap_waiting());
    assert_eq!(vim.text(), "two\nthree four\nfive");
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    assert!(!frame.has("vim.pending"));

    // The timeout runs the longest complete map.
    let mut vim = self::vim(words, (0, 0), &q);
    vim.feed("Q").unwrap();
    time_out(&mut vim);
    assert!(!vim.state().remap_waiting());
    assert_eq!(vim.text(), "ne two\nthree four\nfive");
    // A key that breaks the longer match does too, and is matched again
    // from the start: `j` waits for `jk`.
    vim.feed("Qj").unwrap();
    assert_eq!(vim.text(), "e two\nthree four\nfive");
    assert!(vim.state().remap_waiting());
    vim.feed("k").unwrap();
    assert_eq!(vim.text(), " two\nthree four\nfive");

    // A timeout with nothing complete feeds only the first key as typed:
    // `a` appends, and `b` is matched again in insert mode, as typed.
    let abc = [noremap(&[N], "abc", "dd")];
    let mut vim = self::vim("xyz", (0, 0), &abc);
    vim.feed("ab").unwrap();
    assert!(vim.state().remap_waiting());
    assert_eq!(vim.text(), "xyz");
    time_out(&mut vim);
    assert_eq!((vim.text(), vim.mode()), ("xbyz".into(), "insert"));
    // Nothing held, nothing to resolve.
    time_out(&mut vim);
    assert_eq!(vim.text(), "xbyz");

    // A key fed before the host took a timeout's keys goes after them.
    let mut vim = self::vim(words, (0, 0), &q);
    vim.feed("Q").unwrap();
    vim.state_mut().remap_timeout();
    vim.feed("w").unwrap();
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("ne two\nthree four\nfive".into(), (0, 3))
    );

    // A `0` after a count goes on the count.
    let zero = [noremap(&[N], "0", "$")];
    let digits = "0123456789abc";
    assert_eq!(after(digits, (0, 0), &zero, "0").1, (0, 12));
    assert_eq!(after(digits, (0, 0), &zero, "10l").1, (0, 10));
}

#[test]
fn remap_feed_recursion() {
    let text = "abcd";
    // A noremap's keys aren't matched again; a map's are.
    let x_is_l = noremap(&[N], "x", "l");
    assert_eq!(
        after(
            text,
            (0, 0),
            &[x_is_l.clone(), noremap(&[N], "X", "x")],
            "X"
        ),
        ("bcd".into(), (0, 0))
    );
    assert_eq!(
        after(text, (0, 0), &[x_is_l, map(&[N], "X", "x")], "X"),
        (text.into(), (0, 1))
    );
    // A right side that starts with its left side runs that first key
    // unmapped, and matches the rest.
    assert_eq!(
        after(
            text,
            (0, 1),
            &[map(&[N], "x", "xl"), map(&[N], "l", "h")],
            "x"
        ),
        ("acd".into(), (0, 0))
    );

    // A cycle stops with E223, its queued keys dropped.
    let cycle = [map(&[N], "a", "b"), map(&[N], "b", "ax")];
    let mut vim = vim(text, (0, 0), &cycle);
    vim.feed("a").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.state().message(), Some("E223: recursive mapping"));
    assert_eq!((vim.text(), vim.mode()), (text.into(), "normal"));
    assert!(vim.state_mut().take_replay().is_empty());
    // The next key typed starts over.
    vim.feed("l").unwrap();
    assert!(!vim.state().failed());
    assert_eq!(vim.cursor(), (0, 1));

    // A map's keys aren't recorded: the keys typed are.
    let y = [noremap(&[N], "Y", "x")];
    let mut vim = self::vim(text, (0, 0), &y);
    vim.feed("qaYq").unwrap();
    assert_eq!(vim.registers().get('a').unwrap().text, "Y");
    assert_eq!(vim.text(), "bcd");
    // And replayed through the maps.
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "cd");
}

#[test]
fn remap_feed_set_replaced() {
    let text = "abc\ndef";
    let jk = [noremap(&[N], "jk", "x")];
    let mut vim = vim(text, (0, 0), &jk);
    vim.feed("j").unwrap();
    assert!(vim.state().remap_waiting());
    // The held `j` goes on as typed, not as the new set's `j`; the next
    // key is read with the new set, after it.
    let new = set(&[noremap(&[N], "j", "dd"), noremap(&[N], "l", "x")]);
    vim.state_mut().set_remaps(Arc::clone(&new));
    assert!(!vim.state().remap_waiting());
    assert_eq!(*vim.state().remaps(), *new);
    vim.feed("l").unwrap();
    assert_eq!((vim.text(), vim.cursor()), ("abc\nef".into(), (1, 0)));

    // A key of a map that fails drops the rest of it.
    let q = [noremap(&[N], "Q", "jx")];
    let mut vim = self::vim("ab", (0, 0), &q);
    vim.feed("Q").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.text(), "ab");
    vim.feed("Ql").unwrap();
    assert_eq!((vim.text(), vim.cursor()), ("ab".into(), (0, 1)));
}

#[test]
fn remap_set_hides_builtin_jump_prefixes() {
    for mode in [N, X, O] {
        let remaps = [
            noremap(&[mode], "gs", "x"),
            noremap(&[mode], "gSa", "x"),
            noremap(&[mode], "gsab", "x"),
            noremap(&[mode], "gSab", "x"),
        ];
        let (set, _) = RemapSet::new(&remaps, "\\");
        assert_eq!(set.hides_builtin(mode), vec!["gs", "gSa"], "{mode:?}");
    }
}

/// Puts `text` in register `name`, as a yank would.
fn set_register(vim: &mut HeadlessVim, name: char, text: &str) {
    let register = Register::new(text, false);
    let mut host = HeadlessHost::new();
    vim.registers_mut()
        .write(Some(name), Write::Yank, register, &mut host)
        .unwrap();
}

/// Register `b`'s text and the buffer's after `keys` with `register_a` in
/// register `a`.
fn recording(text: &str, remaps: &[Remap], register_a: &str, keys: &str) -> (String, String) {
    let mut vim = vim(text, (0, 0), remaps);
    set_register(&mut vim, 'a', register_a);
    vim.feed(keys).unwrap();
    let b = vim
        .registers()
        .get('b')
        .map(|r| r.text.clone())
        .unwrap_or_default();
    (b, vim.text())
}

#[test]
fn remap_feed_records_typed_keys_only() {
    let text = "abc\ndef\nghi";
    let jk = [noremap(&[N], "jk", "x")];
    // A replay's key held for a longer map, the replay over before it
    // goes on, isn't recorded: by a timeout, in insert mode, or as part of
    // a map's left side.
    let mut vim = vim(text, (0, 0), &jk);
    set_register(&mut vim, 'a', "j");
    vim.feed("qb@a").unwrap();
    assert!(vim.state().remap_waiting());
    time_out(&mut vim);
    vim.feed("q").unwrap();
    assert_eq!(vim.registers().get('b').unwrap().text, "@a");
    assert_eq!(vim.cursor(), (1, 0));

    let az = [noremap(&[N], "abc", "dd")];
    assert_eq!(
        recording(text, &az, "az", "qb@a<Esc>q"),
        recording(text, &[], "az", "qb@a<Esc>q")
    );
    assert_eq!(recording(text, &az, "az", "qb@a<Esc>q").1, "azbc\ndef\nghi");

    assert_eq!(
        recording(text, &jk, "j", "qb@akq"),
        ("@ak".into(), "bc\ndef\nghi".into())
    );

    // Nor a map's answer to `:s`'s question, or a jump it makes.
    let s = [noremap(&[N], "Q", ":s/a/b/c<CR>y")];
    assert_eq!(recording("a", &s, "", "qbQq"), ("Q".into(), "b".into()));
    let jump = [noremap(&[N], "Q", "gsab")];
    let mut vim = self::vim("x ab", (0, 0), &jump);
    vim.feed("xqbQq").unwrap();
    assert_eq!(vim.registers().get('b').unwrap().text, "Q");
    assert_eq!(vim.cursor(), (0, 1));
}

#[test]
fn remap_feed_bounds_a_macro_its_map_plays_again() {
    // `Q` plays `a`, whose last key is `Q`: each `@a` the map runs would
    // start the macros' count again. The keys fed since one was typed stop
    // at the replay cap. Each play is 600,003 keys: an ex line of blanks,
    // which does nothing, then `Q`.
    let mut vim = vim("abc", (0, 0), &[noremap(&[N], "Q", "@a")]);
    let line = format!(":{}\rQ", " ".repeat(600_000));
    set_register(&mut vim, 'a', &line);
    vim.feed("@a").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("macro stopped: it would replay more than 1000000 keys")
    );
    assert_eq!(vim.mode(), "normal");
    vim.feed("x").unwrap();
    assert_eq!(vim.text(), "bc");
}

#[test]
fn remap_feed_held_expansion_fails_as_the_maps() {
    // `Q`'s `jx` waits for `jxy`; `h` breaks it, and `j` goes on as the
    // map's key: failing on the one row, it drops the rest, so `x` doesn't
    // delete.
    let remaps = [map(&[N], "Q", "jx"), noremap(&[N], "jxy", "l")];
    let mut vim = vim("abc", (0, 0), &remaps);
    vim.feed("Qh").unwrap();
    assert!(vim.state().failed());
    assert_eq!((vim.text(), vim.cursor()), ("abc".into(), (0, 0)));
    vim.feed("l").unwrap();
    assert_eq!(vim.cursor(), (0, 1));
}
