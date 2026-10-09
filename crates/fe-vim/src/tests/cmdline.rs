use fe_command::{ContextFrame, ContextPredicate, DispatchError, Invocation};
use serde_json::json;

use super::registers;
use crate::{FeedError, HeadlessVim, Step, VimKey, VimState, context, parse_keys, registry};

/// What `keys` commit through a [`VimState`], from nothing pending.
fn commits(keys: &str) -> Invocation {
    let mut state = VimState::new();
    let mut last = Step::Cancel;
    for key in parse_keys(keys).unwrap() {
        last = state.feed(key);
    }
    match last {
        Step::Commit(invocation) => invocation,
        other => panic!("{keys} gave {other:?}, not Commit"),
    }
}

#[test]
fn line_mode_and_routing() {
    // `/` and `?` open a line, carrying what was pending.
    for (keys, args) in [
        ("/", json!({"kind": "search_forward"})),
        ("?", json!({"kind": "search_backward"})),
        (
            "d2/",
            json!({"kind": "search_forward", "operator": "delete", "count": 2}),
        ),
        (
            "2\"ay?",
            json!({"kind": "search_backward", "operator": "yank", "count": 2, "register": "a"}),
        ),
    ] {
        let invocation = commits(keys);
        assert_eq!(invocation.command.as_str(), "vim.command_line", "{keys}");
        assert_eq!(invocation.args, args, "{keys}");
    }
    // As a prefix's key they're its: `f/` finds a slash.
    assert_eq!(commits("f/").command.as_str(), "vim.move");

    let registry = registry();
    for id in ["vim.command_line_submit", "vim.command_line_cancel"] {
        let meta = registry.get(&id.parse().unwrap()).unwrap();
        assert_eq!(
            meta.context,
            Some(ContextPredicate::parse("editor && vim.mode == cmdline").unwrap()),
            "{id}"
        );
    }

    // In `cmdline` mode the keys go on the line, not to `step`: `dw` and
    // `x` change nothing until `<CR>` submits them as a pattern.
    let mut vim = HeadlessVim::new("abc dw x dw", (0, 0));
    vim.feed("d/dw x").unwrap();
    assert_eq!(vim.mode(), "cmdline");
    assert!(vim.state().pending().kind().is_none());
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    assert_eq!(frame.get("vim.mode"), Some("cmdline"));
    assert_eq!(frame.get("vim.cmdline_kind"), Some("search_forward"));
    assert_eq!((vim.text().as_str(), vim.cursor()), ("abc dw x dw", (0, 0)));
    // A key the line doesn't take is forwarded.
    assert_eq!(vim.feed("<Up>"), Err(FeedError::Forwarded(VimKey::Up)));
    // `<BS>` deletes the last character; `?`'s line is `search_backward`.
    vim.feed("<BS><BS><CR>").unwrap();
    assert_eq!((vim.text().as_str(), vim.mode()), ("dw x dw", "normal"));
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    assert_eq!(frame.get("vim.cmdline_kind"), None);
    vim.feed("$?").unwrap();
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    assert_eq!(frame.get("vim.cmdline_kind"), Some("search_backward"));

    // Cancelling applies no operator, back in normal mode: `<Esc>`, or
    // `<BS>` on an empty line.
    for keys in ["d/x<Esc>", "d/x<BS><BS>", "d/<BS>"] {
        let mut vim = HeadlessVim::new("abc x", (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            ("abc x", (0, 0)),
            "{keys}"
        );
        assert!(registers(&vim).is_empty(), "{keys}");
        // Nothing carried over: the next key is a fresh command.
        vim.feed("x").unwrap();
        assert_eq!(vim.text(), "bc x", "{keys}");
    }

    // From a visual mode, back to it with the selection kept.
    let mut vim = HeadlessVim::new("abc xyz", (0, 1));
    vim.feed("vl").unwrap();
    let selection = vim.model().selections();
    vim.feed("/xy").unwrap();
    assert_eq!(vim.mode(), "cmdline");
    assert_eq!(vim.model().selections(), selection);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.mode(), "visual");
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 2), Some((0, 1))));
    assert_eq!(vim.model().selections(), selection);
    vim.feed("V/<BS>").unwrap();
    assert_eq!(vim.mode(), "visual_line");

    // Normal-mode commands don't run in `cmdline` mode, and the line's
    // commands don't run outside it.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(json!({"text": "c"}))
        .context(context(vim.state()));
    assert!(matches!(
        vim.dispatch(&submit),
        Err(DispatchError::NotInContext { .. })
    ));
    vim.feed("/").unwrap();
    let mut state = vim.state().clone();
    let Step::Commit(invocation) = state.feed(VimKey::Esc) else {
        panic!("a VimState's <Esc> in cmdline mode cancels");
    };
    assert_eq!(invocation.command.as_str(), "vim.command_line_cancel");
    assert_eq!(state.feed(VimKey::Char('x')), Step::Forward);
    let mut normal = HeadlessVim::new("abc", (0, 0));
    let delete = commits("x").context(context(vim.state()));
    assert!(matches!(
        normal.dispatch(&delete),
        Err(DispatchError::NotInContext { .. })
    ));
}
