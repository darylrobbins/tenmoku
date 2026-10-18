use fe_command::{Effect, Invocation};

use super::{invocation, registers};
use crate::{
    HeadlessVim, Motion, OperateArgs, Operator, Search, SearchPattern, Target,
    search::{Pattern, ROWS_SEARCHED, split_search},
};

/// The `/` register's text.
fn slash(vim: &HeadlessVim) -> Option<&str> {
    Some(vim.registers().get('/')?.text.as_str())
}

#[test]
fn motion_register_and_visual() {
    // `/` goes to the next match and makes it the last search, `?` to the
    // previous.
    let mut vim = HeadlessVim::new("foo abc foo\nfoo", (0, 0));
    vim.feed("/foo<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 8));
    assert_eq!(slash(&vim), Some("foo"));
    assert_eq!(vim.registers().last_search(), Some(("foo", true)));
    assert_eq!(vim.state().message(), None);
    vim.feed("?abc<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 4));
    assert_eq!(vim.registers().last_search(), Some(("abc", false)));

    // An empty line reuses the last pattern, in its own direction.
    vim.feed("/<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 4));
    assert_eq!(vim.registers().last_search(), Some(("abc", true)));
    vim.feed("/foo<CR>/<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));

    // `"/p` puts the last pattern; a yank or delete into `/` is refused.
    let mut vim = HeadlessVim::new("ab foo", (0, 0));
    vim.feed("\"/p").unwrap();
    assert_eq!(vim.text(), "ab foo");
    assert_eq!(
        vim.state().message(),
        Some("E35: No previous regular expression")
    );
    vim.feed("/foo<CR>0\"/p").unwrap();
    assert_eq!((vim.text().as_str(), vim.cursor()), ("afoob foo", (0, 3)));
    let before = vim.registers().clone();
    for keys in ["\"/yw", "\"/x", "\"/dd", "v\"/y"] {
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), "afoob foo", "{keys}");
        assert_eq!(*vim.registers(), before, "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(vim.state().message(), Some("register / is read-only"));
    }
    let yank = OperateArgs {
        operator: Operator::Yank,
        target: Target::Motion(Motion::Line),
        count: None,
        register: Some('/'),
    };
    let dispatched = vim.dispatch(&invocation("vim.operate", yank)).unwrap();
    assert_eq!(dispatched.effect, Effect::Notify);
    assert_eq!(*vim.registers(), before);

    // A search that fails changes nothing but the last search (nvim keeps
    // the pattern it couldn't find), and says why until the next key.
    let mut vim = HeadlessVim::new("abc x", (0, 1));
    vim.feed("yl/x<CR>").unwrap();
    let before = registers(&vim);
    for (keys, message, last) in [
        ("/zzz<CR>", "E486: Pattern not found: zzz", "zzz"),
        ("d/zzz<CR>", "E486: Pattern not found: zzz", "zzz"),
        // A pattern fe-vim can't search isn't kept.
        ("/\\zy<CR>", "pattern not supported: \\zy", "zzz"),
        ("y?\\zy<CR>", "pattern not supported: \\zy", "zzz"),
    ] {
        vim.feed(keys).unwrap();
        assert_eq!(vim.state().message(), Some(message), "{keys}");
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            ("abc x", (0, 4)),
            "{keys}"
        );
        assert_eq!(registers(&vim), before, "{keys}");
        assert_eq!(slash(&vim), Some(last), "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert!(vim.state().pending().kind().is_none(), "{keys}");
        vim.feed("0").unwrap();
        assert_eq!(vim.state().message(), None, "{keys}");
        vim.feed("$").unwrap();
    }

    // In a visual mode the search moves the vim cursor, keeping the anchor.
    let mut vim = HeadlessVim::new("abc x def x", (0, 1));
    vim.feed("vl/x<CR>").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 4), Some((0, 1))));
    vim.feed("?b<CR>").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 1), Some((0, 1))));
    vim.feed("/zzz<CR>").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 1), Some((0, 1))));

    // `d/x` is one change: one `u` puts it all back.
    let mut vim = HeadlessVim::new("abc\nde x f", (0, 1));
    vim.feed("d/x<CR>").unwrap();
    assert_eq!(vim.text(), "ax f");
    assert_eq!(
        registers(&vim),
        [
            ('"', "bc\nde ".to_string(), false),
            ('1', "bc\nde ".to_string(), false),
        ]
    );
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "abc\nde x f");
    assert_eq!(vim.cursor(), (0, 1));

    // The motion's args carry the pattern, and serialize it as its text.
    let search = Motion::Search(Search {
        pattern: SearchPattern::new("a\\|b"),
        forward: false,
        offset: crate::SearchOffset::None,
        again: false,
    });
    assert_eq!(
        serde_json::to_value(search).unwrap(),
        serde_json::json!({"search": {"pattern": "a\\|b", "forward": false}})
    );
    assert_eq!(
        serde_json::from_value::<Motion>(serde_json::to_value(search).unwrap()).unwrap(),
        search
    );
}

/// `vim.move` and `vim.operate` with a search, dispatched directly (as a
/// keymap or `.` would), check it as `/` does.
#[test]
fn search_commands_check_their_pattern() {
    let search = |pattern: &str| {
        Motion::Search(Search {
            pattern: SearchPattern::new(pattern),
            forward: true,
            offset: crate::SearchOffset::None,
            again: false,
        })
    };
    let moving = |pattern: &str| {
        invocation(
            "vim.move",
            crate::MoveArgs {
                motion: search(pattern),
                extend: false,
                count: None,
            },
        )
    };
    let mut vim = HeadlessVim::new("abc x", (0, 0));
    vim.feed("/x<CR>0").unwrap();
    for (pattern, message, last) in [
        ("\\zy", "pattern not supported: \\zy", "x"),
        ("zzz", "E486: Pattern not found: zzz", "zzz"),
        ("a\\(", "invalid pattern: unmatched \\(", "zzz"),
    ] {
        let dispatched = vim.dispatch(&moving(pattern)).unwrap();
        assert_eq!(dispatched.effect, Effect::Notify, "{pattern}");
        assert_eq!(vim.state().message(), Some(message), "{pattern}");
        assert_eq!(slash(&vim), Some(last), "{pattern}");
        assert_eq!(vim.cursor(), (0, 0), "{pattern}");
        let delete = OperateArgs {
            operator: Operator::Delete,
            target: Target::Motion(search(pattern)),
            count: None,
            register: None,
        };
        let dispatched = vim.dispatch(&invocation("vim.operate", delete)).unwrap();
        assert_eq!(dispatched.effect, Effect::Notify, "{pattern}");
        assert_eq!(vim.state().message(), Some(message), "{pattern}");
        assert_eq!((vim.text().as_str(), registers(&vim)), ("abc x", vec![]));
    }
    let dispatched = vim.dispatch(&moving("x")).unwrap();
    assert_eq!(dispatched.effect, Effect::Changed);
    assert_eq!((vim.cursor(), slash(&vim)), ((0, 4), Some("x")));
}

/// What closing the line or failing changes is reported: the mode and the
/// message are at least a `Notify`, a selection dropped `Changed`.
#[test]
fn search_effects() {
    let submit = |vim: &HeadlessVim, text: &str| {
        Invocation::of("vim.command_line_submit")
            .unwrap()
            .args(serde_json::json!({ "text": text }))
            .context(crate::context(vim.state()))
    };
    // A failed search, and one whose only match is at the cursor.
    for (text, message) in [("zzz", true), ("x", false)] {
        let mut vim = HeadlessVim::new("abc x", (0, 4));
        vim.feed("/").unwrap();
        let dispatched = vim.dispatch(&submit(&vim, text)).unwrap();
        assert_eq!(dispatched.effect, Effect::Notify, "{text}");
        assert_eq!(vim.mode(), "normal");
        assert_eq!(vim.state().message().is_some(), message, "{text}");
    }
    let mut vim = HeadlessVim::new("abc x", (0, 0));
    vim.feed("vl").unwrap();
    let yank = OperateArgs {
        operator: Operator::Yank,
        target: Target::Selection,
        count: None,
        register: Some('/'),
    };
    let visual = invocation("vim.operate", yank).context(crate::context(vim.state()));
    assert_eq!(vim.dispatch(&visual).unwrap().effect, Effect::Changed);
    assert_eq!(vim.mode(), "normal");
    let put = crate::PutArgs {
        before: false,
        cursor_after: false,
        count: None,
        register: Some('/'),
    };
    let put = vim.dispatch(&invocation("vim.put", put)).unwrap();
    assert_eq!(put.effect, Effect::Notify);
    assert_eq!(
        vim.state().message(),
        Some("E35: No previous regular expression")
    );
}

/// A line ended by a dispatch rather than a key leaves nothing behind.
#[test]
fn headless_line_starts_empty() {
    let cancel = |vim: &HeadlessVim| {
        Invocation::of("vim.command_line_cancel")
            .unwrap()
            .context(crate::context(vim.state()))
    };
    let mut vim = HeadlessVim::new("foo bar foobar", (0, 0));
    vim.feed("/foo").unwrap();
    vim.dispatch(&cancel(&vim)).unwrap();
    vim.feed("/bar<CR>").unwrap();
    assert_eq!((vim.cursor(), slash(&vim)), ((0, 4), Some("bar")));
    vim.feed("/foo").unwrap();
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({"text": "oo"}))
        .context(crate::context(vim.state()));
    vim.dispatch(&submit).unwrap();
    assert_eq!(vim.cursor(), (0, 9));
    vim.feed("?b<CR>").unwrap();
    assert_eq!((vim.cursor(), slash(&vim)), ((0, 4), Some("b")));
}

/// A counted search steps through each row's matches once listed: its
/// work doesn't grow with the count, or with the matches before each.
#[test]
fn counted_search_is_bounded() {
    // The bound is on rows searched, not time: a loaded machine mustn't
    // trip the default limit and turn a match into `None`.
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::search::set_time_limit(crate::search::DEFAULT_TIME_LIMIT);
        }
    }
    crate::search::set_time_limit(std::time::Duration::from_secs(600));
    let _restore = Restore;
    let row = "x ".repeat(20_000);
    let text = fe_text::Text::new(&format!("{row}\nx\n{row}"));
    let pattern = Pattern::parse("x").unwrap();
    // The matches in order: 20,000 on each long row, one between.
    let matches = 40_001;
    let start = |ix: usize| match ix {
        0..20_000 => 2 * ix,
        20_000 => 40_001,
        _ => 40_003 + 2 * (ix - 20_001),
    };
    for count in [1, 2, 3, 40_000, 40_001, 40_002, 1_000_000, usize::MAX] {
        for forward in [true, false] {
            ROWS_SEARCHED.set(0);
            let found = pattern.nth(&text, 0, forward, count);
            let ix = match forward {
                true => count % matches,
                false => (matches - count % matches) % matches,
            };
            assert_eq!(found, Some(start(ix)), "{count} {forward}");
            // Each row listed once, after the first search's rows.
            assert!(ROWS_SEARCHED.get() <= 6, "{count}: {}", ROWS_SEARCHED.get());
            if count <= 3 {
                assert_eq!(found, nth_slowly(&text, &pattern, 0, forward, count));
            }
        }
    }
}

fn nth_slowly(
    text: &fe_text::Text,
    pattern: &Pattern,
    from: usize,
    forward: bool,
    count: usize,
) -> Option<usize> {
    let mut at = from;
    for _ in 0..count {
        at = pattern.find(text, at, forward)?.start;
    }
    Some(at)
}

#[test]
fn split_search_reads_collections_by_magic() {
    for (line, delimiter, pattern, rest) in [
        ("foo/e", '/', "foo", Some("e")),
        ("a[/]b", '/', "a[/]b", None),
        ("a[/b", '/', "a[/b", None),
        ("\\M[f/]oo", '/', "\\M[f/]oo", None),
        ("\\M\\[foo/", '/', "\\M\\[foo", Some("")),
        ("\\V[f/]oo", '/', "\\V[f", Some("]oo")),
        ("\\V\\[f/]oo", '/', "\\V\\[f/]oo", None),
        ("\\V\\[foo/", '/', "\\V\\[foo/", None),
        ("\\V\\m[f/]", '/', "\\V\\m[f/]", None),
        ("a\\?b?", '?', "a?b", Some("")),
        ("\\M\\[f?]oo", '?', "\\M\\[f", Some("]oo")),
        ("a/b", '?', "a/b", None),
    ] {
        assert_eq!(
            split_search(line, delimiter),
            (pattern.to_string(), rest),
            "{line}"
        );
    }
}

/// A pattern Vim rejects keeps the last search and its direction, whether
/// the line is submitted or the motion dispatched directly.
#[test]
fn invalid_patterns_keep_the_last_search() {
    let mut vim = HeadlessVim::new("foo abc foo", (0, 0));
    vim.feed("/foo<CR>?a\\(<CR>").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("invalid pattern: unmatched \\(")
    );
    assert_eq!(vim.registers().last_search(), Some(("foo", true)));
    assert_eq!(vim.cursor(), (0, 8));
    vim.feed("?<CR>").unwrap();
    assert_eq!(vim.cursor(), (0, 0));

    let backward = Motion::Search(Search {
        pattern: SearchPattern::new("a\\("),
        forward: false,
        offset: crate::SearchOffset::None,
        again: false,
    });
    let args = crate::MoveArgs {
        motion: backward,
        extend: false,
        count: None,
    };
    vim.dispatch(&invocation("vim.move", args)).unwrap();
    assert_eq!(vim.registers().last_search(), Some(("foo", false)));
}
