use std::time::{Duration, Instant};

use fe_command::Effect;
use serde_json::json;

use super::{invocation, registers};
use crate::{
    Ex, ExCommand, ExError, Flags, HeadlessVim, Substitute, context, search::ROWS_SEARCHED,
};

/// `buffer` after `keys` from `cursor`.
fn after(buffer: &str, cursor: (u32, usize), keys: &str) -> String {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    vim.text()
}

/// The `/` register's text.
fn search(vim: &HeadlessVim) -> Option<String> {
    vim.registers()
        .get('/')
        .map(|register| register.text.clone())
}

#[test]
fn flags_specials_and_bounds() {
    // The first match on the row, or with `g` every one, the caret on the
    // row's first non-blank.
    assert_eq!(after("a a a\nxa", (0, 2), ":s/a/b/<CR>"), "b a a\nxa");
    assert_eq!(after("a a a\nxa", (0, 2), ":s/a/b/g<CR>"), "b b b\nxa");
    assert_eq!(after("a a a\nxa a", (0, 2), ":%s/a/b/<CR>"), "b a a\nxb a");
    // `i` and `I` override the case, the last given winning; `\c` and `\C`
    // in the pattern beat them.
    assert_eq!(after("a A a", (0, 0), ":s/a/b/gi<CR>"), "b b b");
    assert_eq!(after("a A a", (0, 0), ":s/A/b/gI<CR>"), "a b a");
    assert_eq!(after("a A a", (0, 0), ":s/a/b/giI<CR>"), "b A b");
    assert_eq!(after("a A a", (0, 0), ":s/\\Ca/b/gi<CR>"), "b A b");
    assert_eq!(after("a A a", (0, 0), ":s/\\ca/b/gI<CR>"), "b b b");
    // Each `g` toggles.
    assert_eq!(after("a a a", (0, 0), ":s/a/b/gg<CR>"), "b a a");

    // `n` counts without changing, the caret to its row's first non-blank.
    let mut vim = HeadlessVim::new("  xa a\n  ab\n  b c", (2, 4));
    vim.feed(":%s/a/X/gn<CR>").unwrap();
    assert_eq!(vim.text(), "  xa a\n  ab\n  b c");
    assert_eq!(vim.cursor(), (2, 2));
    assert_eq!(vim.state().message(), Some("3 matches on 2 lines"));
    assert!(!vim.model().is_dirty());

    // `e` suppresses E486; the pattern is still the last search.
    let mut vim = HeadlessVim::new("  xa\n  ab", (1, 3));
    vim.feed(":%s/z/X/<CR>").unwrap();
    assert_eq!(vim.state().message(), Some("E486: Pattern not found: z"));
    vim.feed(":%s/zz/X/e<CR>").unwrap();
    assert_eq!(vim.state().message(), None);
    assert_eq!((vim.text().as_str(), vim.cursor()), ("  xa\n  ab", (1, 3)));
    assert_eq!(search(&vim).as_deref(), Some("zz"));
    // Each `e` toggles, and `&e` toggles the kept one.
    for (keys, error) in [
        (":s/z/X/ee<CR>", true),
        (":s/z/X/e<CR>:s/z/X/&e<CR>", true),
        (":s/z/X/e<CR>:s/z/X/&<CR>", false),
        (":s/z/X/<CR>:s/z/X/&e<CR>", false),
    ] {
        let mut vim = HeadlessVim::new("a", (0, 0));
        vim.feed(keys).unwrap();
        let want = error.then_some("E486: Pattern not found: z");
        assert_eq!(vim.state().message(), want, "{keys}");
    }

    // Counts from 2147483647 are too large; blanks may follow `:&`, before
    // its `&` and flags, but not `:&&`.
    assert_eq!(
        Ex::parse("s/a/b/ 2147483646").unwrap().command,
        ExCommand::Substitute(Substitute {
            with: Some(("a".into(), "b".into())),
            flags: Flags::default(),
            count: Some(2_147_483_646),
        })
    );
    for count in ["2147483647", "4294967296", "99999999999999999999"] {
        assert_eq!(
            Ex::parse(&format!("s/a/b/ {count}")),
            Err(ExError::ValueTooLarge(count.into()))
        );
    }
    assert_eq!(
        after("a a\na a\na a", (0, 0), ":s/a/X/<CR>:& g<CR>"),
        "X X\na a\na a"
    );
    assert_eq!(
        after("a a\na a\na a", (0, 0), ":s/a/X/g<CR>j:& &<CR>"),
        "X X\nX X\na a"
    );
    assert_eq!(Ex::parse("&& g"), Err(ExError::Trailing("g".into())));
    // Repeating before any substitute is E33, from each of `:s`, `:&`,
    // `:&&` and `&`.
    for keys in [":s<CR>", ":&<CR>", ":&&<CR>", "&"] {
        let mut vim = HeadlessVim::new("a", (0, 0));
        vim.feed("/a<CR>").unwrap();
        vim.feed(keys).unwrap();
        assert_eq!(
            vim.state().message(),
            Some("E33: No previous substitute regular expression"),
            "{keys}"
        );
    }

    // `&` keeps the previous flags; `:&` drops them, `:&&` and `&` keep
    // them, and `:s` alone is `:&`.
    let three = "a a\na a\na a";
    assert_eq!(
        after(three, (0, 0), ":s/a/X/g<CR>j:s/a/Y/&<CR>"),
        "X X\nY Y\na a"
    );
    assert_eq!(after(three, (0, 0), ":s/a/X/g<CR>j:&<CR>"), "X X\nX a\na a");
    assert_eq!(after(three, (0, 0), ":s/a/X/g<CR>j:s<CR>"), "X X\nX a\na a");
    assert_eq!(
        after(three, (0, 0), ":s/a/X/g<CR>j:&&<CR>"),
        "X X\nX X\na a"
    );
    assert_eq!(after(three, (0, 0), ":s/a/X/g<CR>j&"), "X X\nX X\na a");
    assert_eq!(after(three, (0, 0), ":s/a/X/<CR>j2&"), "X a\nX a\nX a");
    assert_eq!(
        Ex::parse("&&g 2").unwrap().command,
        ExCommand::Substitute(Substitute {
            with: None,
            flags: Flags {
                keep: true,
                global: true,
                ..Flags::default()
            },
            count: Some(2),
        })
    );

    // The specials, as nvim makes them.
    let foo = "foo bar baz";
    for (replacement, want) in [
        ("[&,\\&]", "foo [bar,&] baz"),
        ("\\0-\\3\\2\\1-\\9", "foo bar-rab- baz"),
        ("\\t,\\\\,\\x", "foo \t,\\,x baz"),
        ("\\u&-\\l\\U&", "foo Bar-bAR baz"),
        ("\\U&x\\Ey-\\L&AB\\eC", "foo BARXy-barabC baz"),
        ("\\U\\lABC", "foo aBC baz"),
        ("\\u\\E&", "foo bar baz"),
        ("a\\rb", "foo a\nb baz"),
    ] {
        let keys = format!(":s/\\(b\\)\\(a\\)\\(.\\)/{replacement}/<CR>");
        assert_eq!(after(foo, (0, 0), &keys), want, "{replacement}");
    }
    // `~` is the previous replacement, as typed (`\~` is itself).
    assert_eq!(
        after(three, (0, 0), ":s/a/X/<CR>j:s/a/~Y/<CR>j:s/a/~/<CR>"),
        "X a\nXY a\nXY a"
    );
    assert_eq!(
        after(three, (0, 0), ":s/a/X/<CR>j:s/a/\\~~/<CR>"),
        "X a\n~X a\na a"
    );
    // A repeat expands the typed `~` again, each time (nvim: "X XY XYY
    // XYYY" from each of `:s`, `:&`, `:&&` and `&`).
    for repeat in [":s<CR>", ":&<CR>", ":&&<CR>", "&"] {
        let keys = format!(":s/a/X/<CR>:s/a/~Y/<CR>{repeat}{repeat}");
        assert_eq!(after("a a a a", (0, 0), &keys), "X XY XYY XYYY", "{repeat}");
    }
    // An invalid pattern leaves `~` as it was; a valid one finding nothing
    // doesn't (nvim).
    assert_eq!(
        after("ab", (0, 0), ":s/a/X/<CR>:s/\\(/Y/<CR>:s/b/~/<CR>"),
        "XX"
    );
    assert_eq!(
        after("aa", (0, 0), ":s/a/X/<CR>:s/zzz/~Y/e<CR>:s/a/~/<CR>"),
        "XXY"
    );
    // `\U` and `\u` case one character at a time, as nvim's tables do.
    assert_eq!(after("straße ǆ", (0, 0), ":s/.*/\\U&/<CR>"), "STRAẞE Ǆ");
    // `\r` breaks the row; the caret goes to the last row it made.
    let mut vim = HeadlessVim::new("one two three", (0, 4));
    vim.feed(":s/ /\\r/g<CR>").unwrap();
    assert_eq!(
        (vim.text().as_str(), vim.cursor()),
        ("one\ntwo\nthree", (2, 0))
    );

    // `\n` in the replacement and the `c` flag are errors that change
    // nothing: not the text, the caret, the last search or the last
    // substitute.
    for (line, error) in [
        ("s/z/x\\ny/", ExError::NulInReplacement),
        (
            "s/z/x/gc",
            ExError::Unsupported("the c flag (confirm)".into()),
        ),
    ] {
        assert_eq!(Ex::parse(line), Err(error.clone()), "{line}");
        let mut vim = HeadlessVim::new("a b\na b", (0, 2));
        vim.feed(":s/b/B/<CR>/a<CR>").unwrap();
        let (text, cursor) = (vim.text(), vim.cursor());
        let substitute = vim.registers().clone();
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(vim.state().message(), Some(error.to_string().as_str()));
        assert_eq!((vim.text(), vim.cursor()), (text, cursor), "{line}");
        assert_eq!(vim.registers(), &substitute, "{line}");
        // `:&` still repeats the substitute before.
        vim.feed(":&<CR>").unwrap();
        assert_eq!(vim.text(), "a B\na B", "{line}");
    }

    // `.` after `:s` repeats the change before it, not the `:s`.
    let mut vim = HeadlessVim::new("xab\n  ab\n  ba", (0, 0));
    vim.feed("x:s/b/X/<CR>j.").unwrap();
    assert_eq!(vim.text(), "aX\n ab\n  ba");

    // The pattern becomes the last search and the `/` register (keeping the
    // search's direction); registers `" 0-9 - a-z` stay empty.
    let mut vim = HeadlessVim::new("a\na\na\na\na", (2, 0));
    vim.feed("?a<CR>:s/a/A/<CR>").unwrap();
    assert_eq!(search(&vim).as_deref(), Some("a"));
    assert_eq!(vim.registers().last_search(), Some(("a", false)));
    assert!(vim.state().highlight().is_some());
    assert!(registers(&vim).is_empty());
    // An empty pattern is the last search's; a repeat leaves `/` alone.
    let mut vim = HeadlessVim::new("a A a a", (0, 0));
    vim.feed("/A<CR>:s//x/<CR>").unwrap();
    assert_eq!(
        (vim.text().as_str(), search(&vim).as_deref()),
        ("a x a a", Some("A"))
    );
    vim.feed(":s/a/X/<CR>:s//Y/<CR>").unwrap();
    assert_eq!(
        (vim.text().as_str(), search(&vim).as_deref()),
        ("X x Y a", Some("a"))
    );
    vim.feed("/x<CR>:s<CR>").unwrap();
    assert_eq!(
        (vim.text().as_str(), search(&vim).as_deref()),
        ("X x Y Y", Some("x"))
    );

    // `:%s` over 200,000 rows: one transaction, undone by one `u`, each row
    // read once.
    let rows = 200_000;
    let text: String = (0..rows).map(|row| format!("x{row} x\n")).collect();
    let text = text.trim_end();
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed(":").unwrap();
    let submit = invocation("vim.command_line_submit", json!({"text": "%s/x/y/"}))
        .context(context(vim.state()));
    // Two hundred thousand rows in a debug build can pass the default limit.
    crate::search::set_time_limit(Duration::from_secs(600));
    ROWS_SEARCHED.set(0);
    let dispatched = vim.dispatch(&submit).unwrap();
    crate::search::set_time_limit(crate::search::DEFAULT_TIME_LIMIT);
    assert_eq!(ROWS_SEARCHED.get(), rows);
    assert_eq!(dispatched.effect, Effect::Changed);
    assert_eq!(dispatched.transactions.len(), 1);
    assert!(vim.text().starts_with("y0 x\ny1 x\n"));
    assert_eq!(vim.cursor(), (rows as u32 - 1, 0));
    assert_eq!(
        vim.state().message(),
        Some("200000 substitutions on 200000 lines")
    );
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), text);
    assert_eq!(vim.cursor(), (0, 0));

    // A `g` substitute on a row of 100,000 matches, linear in the row.
    let row = "ab".repeat(100_000);
    let mut vim = HeadlessVim::new(&row, (0, 0));
    // A hundred thousand matches under load can pass the default limit.
    crate::search::set_time_limit(Duration::from_secs(600));
    let started = Instant::now();
    vim.feed(":s/\\(a\\)b/\\u\\1-/g<CR>").unwrap();
    let elapsed = started.elapsed();
    crate::search::set_time_limit(crate::search::DEFAULT_TIME_LIMIT);
    assert!(elapsed < Duration::from_secs(10));
    assert_eq!(vim.text(), "A-".repeat(100_000));
}
