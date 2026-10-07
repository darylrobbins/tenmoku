use fe_buffer::Origin;
use fe_command::{Effect, Invocation};

use crate::{HeadlessVim, context, history::KEPT};

/// docs/decisions/vim-engine.md §6's buffer.
const SPOT: &str = "fn foo(a, b) {\n    let x = foo(a);\n    x + b\n}\nfoo end";

#[test]
fn carets_after_undo_and_redo() {
    for (keys, from, text) in [
        (
            "ddu<C-r>",
            (0, 4),
            "    let x = foo(a);\n    x + b\n}\nfoo end",
        ),
        (
            "dwu<C-r>",
            (1, 8),
            "fn foo(a, b) {\n    let = foo(a);\n    x + b\n}\nfoo end",
        ),
        ("x$u", (2, 4), SPOT),
        (
            "ciwzz<Esc>0u<C-r>",
            (1, 8),
            "fn foo(a, b) {\n    let zz = foo(a);\n    x + b\n}\nfoo end",
        ),
    ] {
        let mut vim = HeadlessVim::new(SPOT, from);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), text, "{keys}");
        assert_eq!(vim.cursor(), from, "{keys}");
    }

    // `2u` undoes two changes, `2<C-r>` redoes them.
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    vim.feed("xxx2u").unwrap();
    assert_eq!(vim.text(), "bcdef");
    vim.feed("2<C-r>").unwrap();
    assert_eq!(vim.text(), "def");
    // Past the history it stops, having done what there was.
    vim.feed("9u").unwrap();
    assert_eq!(vim.text(), "abcdef");

    // Nothing to undo or redo.
    let undo = |vim: &HeadlessVim, command: &str| {
        Invocation::of(command)
            .unwrap()
            .context(context(vim.state()))
    };
    let invocation = undo(&vim, "vim.undo");
    let dispatched = vim.dispatch(&invocation).unwrap();
    assert_eq!(dispatched.effect, Effect::Nothing);
    assert!(dispatched.transactions.is_empty());
    vim.feed("3<C-r>").unwrap();
    let invocation = undo(&vim, "vim.redo");
    assert_eq!(vim.dispatch(&invocation).unwrap().effect, Effect::Nothing);

    // At most 1,000 transactions are remembered.
    let mut vim = HeadlessVim::new("a", (0, 0));
    vim.feed(&"~".repeat(KEPT + 11)).unwrap();
    assert_eq!(vim.text(), "A");
    assert_eq!(vim.state().carets.len(), KEPT);
    vim.feed("2u").unwrap();
    assert_eq!((vim.text().as_str(), vim.cursor()), ("A", (0, 0)));

    // A remembered row past the end goes to the last row: another origin
    // deleted rows above meanwhile.
    let mut vim = HeadlessVim::new("a\nb\nc\nd\nefg", (4, 2));
    vim.feed("x").unwrap();
    let ai = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    vim.model_mut().buffer_mut().edit([(0..6, "")], ai);
    assert_eq!(vim.text(), "d\nef");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "d\nefg");
    assert_eq!(vim.cursor(), (1, 2));
}

#[test]
fn empty_sessions_leave_no_record() {
    let mut vim = HeadlessVim::new("abcdef", (0, 1));
    vim.feed("xl").unwrap();
    vim.feed(&"a<Esc>".repeat(KEPT + 5)).unwrap();
    assert_eq!(vim.state().carets.len(), 1);
    vim.feed("u").unwrap();
    assert_eq!((vim.text().as_str(), vim.cursor()), ("abcdef", (0, 1)));
    // Nor does a `.` that changed nothing.
    vim.feed("<C-r>a<Esc>u.").unwrap();
    assert_eq!(vim.state().carets.len(), 1);
}

#[test]
fn empty_transactions_keep_the_full_record() {
    // 1,000 real changes, then an insert, a change and a `.` that change
    // nothing: the oldest real record is still there.
    let mut vim = HeadlessVim::new("ab", (0, 1));
    vim.feed(&"~".repeat(KEPT)).unwrap();
    assert_eq!(vim.state().carets.len(), KEPT);
    let oldest = vim.state().carets.oldest().cloned();
    vim.feed("i<Esc>ch<Esc>.").unwrap();
    assert_eq!(vim.text(), "ab");
    assert_eq!(vim.state().carets.len(), KEPT);
    assert_eq!(vim.state().carets.oldest().cloned(), oldest);
    assert_eq!(oldest.map(|(_, carets)| carets), Some(vec![(0, 1)]));
}
