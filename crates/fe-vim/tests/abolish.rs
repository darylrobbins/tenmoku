//! `cr{char}`, vim-abolish's case coercion, as its `doc/abolish.txt` and
//! its `g:Abolish.Coercions` transforms describe it (nvim has no abolish, so
//! there's no oracle corpus).

use fe_buffer::ResolvedSelection;
use fe_command::ContextFrame;
use fe_vim::HeadlessVim;

fn run(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    vim
}

fn pending_kind(vim: &HeadlessVim) -> Option<String> {
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    frame.get("vim.pending_kind").map(str::to_string)
}

/// Two carets, at `offsets`.
fn with_carets(text: &str, offsets: [usize; 2]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.model_mut()
        .set_selections(offsets.map(|offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
    vim
}

fn heads(vim: &HeadlessVim) -> Vec<usize> {
    vim.model().selections().iter().map(|s| s.head()).collect()
}

/// The coercion keys, in the order of each word's expectations below.
const KEYS: [&str; 12] = [
    "s", "_", "m", "p", "c", "u", "U", "-", "k", ".", "<Space>", "t",
];

#[test]
fn coercions_match_abolish() {
    let words: [(&str, [&str; 12]); 7] = [
        (
            "fooBar",
            [
                "foo_bar", "foo_bar", "FooBar", "FooBar", "fooBar", "FOO_BAR", "FOO_BAR",
                "foo-bar", "foo-bar", "foo.bar", "foo bar", "Foo Bar",
            ],
        ),
        (
            "HTTPServer",
            [
                "http_server",
                "http_server",
                "HTTPServer",
                "HTTPServer",
                // No `_` and a lowercase letter: only the first char goes
                // down, as abolish's `crc` leaves it.
                "hTTPServer",
                "HTTP_SERVER",
                "HTTP_SERVER",
                "http-server",
                "http-server",
                "http.server",
                "http server",
                "Http Server",
            ],
        ),
        (
            "foo_bar",
            [
                "foo_bar", "foo_bar", "FooBar", "FooBar", "fooBar", "FOO_BAR", "FOO_BAR",
                "foo-bar", "foo-bar", "foo.bar", "foo bar", "Foo Bar",
            ],
        ),
        (
            // `iw` takes `foo` only: `-` isn't a word char.
            "foo-bar",
            [
                "foo-bar", "foo-bar", "Foo-bar", "Foo-bar", "foo-bar", "FOO-bar", "FOO-bar",
                "foo-bar", "foo-bar", "foo-bar", "foo-bar", "Foo-bar",
            ],
        ),
        (
            "utf8Value",
            [
                "utf8_value",
                "utf8_value",
                "Utf8Value",
                "Utf8Value",
                "utf8Value",
                "UTF8_VALUE",
                "UTF8_VALUE",
                "utf8-value",
                "utf8-value",
                "utf8.value",
                "utf8 value",
                "Utf8 Value",
            ],
        ),
        (
            // Case mapping is Unicode's; abolish's `\u` `\l` and `\w` are
            // ASCII, so `É` starts no title word.
            "ÉcoleNormale",
            [
                "école_normale",
                "école_normale",
                "ÉcoleNormale",
                "ÉcoleNormale",
                "écoleNormale",
                "ÉCOLE_NORMALE",
                "ÉCOLE_NORMALE",
                "école-normale",
                "école-normale",
                "école.normale",
                "école normale",
                "école Normale",
            ],
        ),
        (
            "straße_über",
            [
                "straße_über",
                "straße_über",
                "StraßeÜber",
                "StraßeÜber",
                "straßeÜber",
                "STRAẞE_ÜBER",
                "STRAẞE_ÜBER",
                "straße-über",
                "straße-über",
                "straße.über",
                "straße über",
                "Straße über",
            ],
        ),
    ];
    for (word, expected) in words {
        let text = format!("x {word} y");
        for (keys, after) in KEYS.iter().zip(expected) {
            let keys = format!("cr{keys}");
            // From inside the word, the caret going to its start.
            let vim = run(&text, (0, 4), &keys);
            assert_eq!(vim.text(), format!("x {after} y"), "{keys} on {word}");
            assert_eq!(vim.cursor(), (0, 2), "{keys} on {word}: the caret");
            assert_eq!(vim.mode(), "normal", "{keys} on {word}: the mode");
        }
    }
    // `'[` and `']` mark the new text.
    let vim = run("x fooBar y", (0, 5), "crs");
    assert_eq!(vim.text(), "x foo_bar y");
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((0, 2)), Some((0, 8))));
}

#[test]
fn coercion_counts_repeat_and_undo() {
    // `2crs` takes the word and its blank, as `2iw` does.
    let vim = run("fooBar bazQux", (0, 3), "2crs");
    assert_eq!(vim.text(), "foo_bar bazQux");
    assert_eq!(vim.cursor(), (0, 0));
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((0, 0)), Some((0, 7))));

    // `.` repeats the coercion and its count at the caret then.
    let mut vim = run("aB cD eF gH iJ", (0, 0), "3crs");
    assert_eq!(vim.text(), "a_b c_d eF gH iJ");
    vim.feed("ww.").unwrap();
    assert_eq!(vim.text(), "a_b c_d e_f g_h iJ");
    assert_eq!(vim.cursor(), (0, 8));
    // One `u` undoes it.
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "a_b c_d eF gH iJ");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "aB cD eF gH iJ");
    let mut vim = run("fooBar bazQux", (0, 0), "crcw");
    vim.feed("crm").unwrap();
    assert_eq!(vim.text(), "fooBar BazQux");
    vim.feed("0.").unwrap();
    assert_eq!(vim.text(), "FooBar BazQux");

    // Two carets change both words, in one undo step.
    let text = "fooBar = bazQux;";
    let mut vim = with_carets(text, [3, 12]);
    vim.feed("cru").unwrap();
    assert_eq!(vim.text(), "FOO_BAR = BAZ_QUX;");
    assert_eq!(heads(&vim), [0, 10]);
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), text);
    // A caret on a word the coercion leaves as it was moves only.
    let mut vim = with_carets("foo_bar bazQux", [4, 10]);
    vim.feed("crs").unwrap();
    assert_eq!(vim.text(), "foo_bar baz_qux");
    assert_eq!(heads(&vim), [0, 8]);
    // Two carets in one word change it once.
    let mut vim = with_carets("x fooBar y", [3, 6]);
    vim.feed("cr-").unwrap();
    assert_eq!(vim.text(), "x foo-bar y");
    assert_eq!(heads(&vim), [2]);
}

#[test]
fn coercion_cancels_and_keeps_registers() {
    // `cr` waits on the prefix; `<Esc>` or a char naming no coercion
    // cancels.
    let mut vim = HeadlessVim::new("fooBar baz", (0, 2));
    vim.feed("cr").unwrap();
    assert_eq!(pending_kind(&vim).as_deref(), Some("abolish"));
    for keys in ["cr<Esc>", "crz", "cr<CR>"] {
        let vim = run("fooBar baz", (0, 2), keys);
        assert_eq!(vim.text(), "fooBar baz", "{keys}");
        assert_eq!(vim.cursor(), (0, 2), "{keys}: the caret");
        assert_eq!(vim.mode(), "normal", "{keys}: the mode");
        assert_eq!(pending_kind(&vim), None, "{keys}");
    }
    // After the cancel, keys start afresh.
    let vim = run("fooBar baz", (0, 2), "crzx");
    assert_eq!(vim.text(), "foBar baz");

    // An already snake_case word: no edit, so no undo step (`u` undoes the
    // `x` before it).
    let mut vim = run("xfoo_bar baz", (0, 0), "x");
    vim.feed("crs").unwrap();
    assert_eq!(vim.text(), "foo_bar baz");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "xfoo_bar baz");

    // No register changes.
    let names = ['"', '-', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
    let held = |vim: &HeadlessVim| -> Vec<_> {
        names
            .iter()
            .map(|&name| (name, vim.registers().get(name).cloned()))
            .collect()
    };
    let mut vim = run("one two\nline\nfooBar end", (0, 0), "yiwjddkxj0");
    let before = held(&vim);
    assert!(before.iter().filter(|(_, held)| held.is_some()).count() >= 4);
    for keys in ["crs", "crc", "2cru", "crt", "$."] {
        vim.feed(keys).unwrap();
        assert_eq!(held(&vim), before, "{keys}");
    }
    assert_eq!(vim.text(), "ne two\nFoo Bar End");
}
