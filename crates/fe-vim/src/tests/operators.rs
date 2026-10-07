use std::time::{Duration, Instant};

use fe_buffer::{Origin, Point, ResolvedSelection};
use fe_command::{Actor, DispatchError};

use super::{commit, invocation, registers};
use crate::{
    HeadlessVim, Motion, OperateArgs, Operator, PutArgs, Register, Target, VimState, Write, context,
};

fn ai() -> Actor {
    Actor::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// Every caret as a row and a byte column.
fn carets(vim: &HeadlessVim) -> Vec<(u32, u32)> {
    let text = vim.model().text();
    vim.model()
        .selections()
        .iter()
        .map(|selection| {
            let point = text.offset_to_point(selection.head());
            (point.row, point.column)
        })
        .collect()
}

/// The text, the carets, the registers and the dirty flag.
type Snapshot = (String, Vec<(u32, u32)>, Vec<(char, String, bool)>, bool);

/// What a test compares to see that nothing happened.
fn snapshot(vim: &HeadlessVim) -> Snapshot {
    (
        vim.text(),
        carets(vim),
        registers(vim),
        vim.model().is_dirty(),
    )
}

#[test]
fn transactions_origins_and_failed_targets() {
    // `d3w` as an agent: one transaction, undone by the agent's undo.
    let text = "one two three four\nnext";
    let mut vim = HeadlessVim::new(text, (0, 0));
    let d3w = commit("d3w").context(context(&VimState::new())).actor(ai());
    let dispatched = vim.dispatch(&d3w).unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    assert_eq!(vim.text(), "four\nnext");
    let undone = vim.model_mut().buffer_mut().undo(Origin::is_ai).unwrap();
    assert_eq!(undone, Some(dispatched.transactions[0]));
    vim.model_mut().sync_display();
    assert_eq!(vim.text(), text);

    // A yank changes no text and makes no transaction.
    let mut vim = HeadlessVim::new(text, (0, 4));
    let yy = commit("yy").context(context(&VimState::new()));
    let dispatched = vim.dispatch(&yy).unwrap();
    assert!(dispatched.transactions.is_empty());
    assert_eq!(vim.text(), text);
    assert!(!vim.model().is_dirty());
    assert_eq!(
        registers(&vim)[0],
        ('"', "one two three four\n".into(), true)
    );

    // A failed motion or a missing object cancels the operator: no edit, no
    // register, no move.
    for keys in ["dfz", "d%", "di("] {
        let mut vim = HeadlessVim::new("abc def\nghi", (0, 2));
        vim.feed("yl").unwrap();
        let before = snapshot(&vim);
        let dispatched = vim
            .dispatch(&commit(keys).context(context(&VimState::new())))
            .unwrap();
        assert!(dispatched.transactions.is_empty(), "{keys}");
        assert_eq!(snapshot(&vim), before, "{keys}");
    }

    // Two carets: both words go in one transaction, a caret stays at each,
    // and the registers hold the last caret's word.
    let mut vim = HeadlessVim::new("foo bar\nbaz qux", (0, 0));
    let model = vim.model_mut();
    let carets_at = [(0, 4), (1, 4)].map(|(row, column)| {
        let offset = model.text().point_to_offset(Point::new(row, column));
        ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }
    });
    model.set_selections(carets_at);
    let dispatched = vim
        .dispatch(&commit("dw").context(context(&VimState::new())))
        .unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    assert_eq!(vim.text(), "foo \nbaz ");
    assert_eq!(carets(&vim), [(0, 3), (1, 3)]);
    assert_eq!(registers(&vim)[0], ('"', "qux".into(), false));

    // The registers follow the primary (last) caret: its `di(` fails, so
    // they're left alone while the first caret's object still goes.
    let mut vim = HeadlessVim::new("f(one)\nplain", (1, 0));
    vim.feed("yl").unwrap();
    let model = vim.model_mut();
    let both = [(0, 3), (1, 2)].map(|(row, column)| {
        let offset = model.text().point_to_offset(Point::new(row, column));
        ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }
    });
    model.set_selections(both);
    let kept = registers(&vim);
    let dispatched = vim
        .dispatch(&commit("di(").context(context(&VimState::new())))
        .unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    assert_eq!(vim.text(), "f()\nplain");
    assert_eq!(registers(&vim), kept);
    // The same for a yank.
    let mut vim = HeadlessVim::new("f(one)\nplain", (0, 3));
    let model = vim.model_mut();
    let both = [(0, 3), (1, 2)].map(|(row, column)| {
        let offset = model.text().point_to_offset(Point::new(row, column));
        ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }
    });
    model.set_selections(both);
    vim.dispatch(&commit("yi(").context(context(&VimState::new())))
        .unwrap();
    assert_eq!(registers(&vim), []);

    // Both last rows go, with the line break before them.
    let mut vim = HeadlessVim::new("a\nb\nc", (0, 0));
    let model = vim.model_mut();
    let rows = [1, 2].map(|row| {
        let offset = model.text().row_start(row);
        ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }
    });
    model.set_selections(rows);
    let dispatched = vim
        .dispatch(&commit("dd").context(context(&VimState::new())))
        .unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    assert_eq!(vim.text(), "a");
    assert_eq!(registers(&vim)[0], ('"', "c\n".into(), true));

    // `change` waits for insert mode.
    let mut vim = HeadlessVim::new(text, (0, 0));
    let change = invocation(
        "vim.operate",
        OperateArgs {
            operator: Operator::Change,
            target: Target::Motion(Motion::WordForward),
            count: None,
            register: None,
        },
    );
    let before = snapshot(&vim);
    assert!(matches!(
        vim.dispatch(&change),
        Err(DispatchError::Failed { .. })
    ));
    assert_eq!(snapshot(&vim), before);
}

#[test]
fn counts_are_bounded() {
    let rows = "abc\ndef\nghi";
    let started = Instant::now();
    for (keys, cursor, left) in [
        ("99999999dd", (0, 0), ""),
        ("99999999x", (1, 1), "abc\nd\nghi"),
        ("4294967295X", (2, 2), "abc\ndef\ni"),
    ] {
        let mut vim = HeadlessVim::new(rows, cursor);
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), left, "{keys}");
    }
    assert!(started.elapsed() < Duration::from_secs(5));

    // A put that would insert gigabytes is refused whole.
    let mut vim = HeadlessVim::new(rows, (0, 0));
    vim.feed("yl").unwrap();
    let put = |count| {
        invocation(
            "vim.put",
            PutArgs {
                before: false,
                count: Some(count),
                register: None,
            },
        )
    };
    let before = snapshot(&vim);
    assert!(matches!(
        vim.dispatch(&put(u32::MAX)),
        Err(DispatchError::Failed { .. })
    ));
    assert_eq!(snapshot(&vim), before);
    vim.feed("3p").unwrap();
    assert_eq!(vim.text(), "aaaabc\ndef\nghi");

    // Lines a host wrote without their line break count it: 2 bytes, so
    // 33554433 copies are past 64 MiB.
    let mut vim = HeadlessVim::new(rows, (2, 0));
    let host = &mut fe_editor::commands::HeadlessHost::new();
    vim.registers_mut()
        .write(None, Write::Yank, Register::new("a", true), host)
        .unwrap();
    let before = snapshot(&vim);
    assert!(matches!(
        vim.dispatch(&put(33_554_433)),
        Err(DispatchError::Failed { .. })
    ));
    assert_eq!(snapshot(&vim), before);

    // `%` from a `*/` after many `/*` is one pass too.
    let openers = format!("{}*/", "/* ".repeat(100_000));
    let mut vim = HeadlessVim::new(&openers, (0, openers.len() - 1));
    let started = Instant::now();
    vim.feed("%").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    assert!(started.elapsed() < Duration::from_secs(5));

    // A counted bracket object is one pass however deep the nesting.
    let depth = 3000;
    let nested = format!("{}x{}", "(\n".repeat(depth), ")\n".repeat(depth));
    let mut vim = HeadlessVim::new(nested.trim_end(), (depth as u32, 0));
    let started = Instant::now();
    vim.feed("99999di(").unwrap();
    assert_eq!(vim.text(), nested.trim_end(), "no pair that far out");
    vim.feed("2999di(").unwrap();
    assert!(vim.text().starts_with("(\n(\n)"), "{:?}", &vim.text()[..10]);
    assert!(started.elapsed() < Duration::from_secs(5));
}
