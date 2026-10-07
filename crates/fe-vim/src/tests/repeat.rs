use fe_buffer::{BufferEvent, Origin};
use fe_command::{Actor, Invocation};

use super::registers;
use crate::{FeedError, HeadlessVim, Register, Write, context};

#[test]
fn last_change_rules() {
    let text = "one two three four five six seven\nsecond";

    // A yank isn't a change: `.` repeats the `d3w`.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("d3w\"ayy.").unwrap();
    assert_eq!(vim.text(), "seven\nsecond");
    assert_eq!(registers(&vim)[0], ('"', "four five six ".into(), false));

    // With only motions and yanks, there's nothing to repeat.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("wyiwj$yy.").unwrap();
    assert_eq!(vim.text(), text);
    assert!(vim.state().last_change().is_none());

    // A new count replaces the old.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("d2w3.").unwrap();
    assert_eq!(vim.text(), "six seven\nsecond");

    // An insert session's change and text, as one transaction.
    let mut vim = HeadlessVim::new("one two\nthree four", (0, 0));
    vim.feed("ciwXY<Esc>j0").unwrap();
    let repeat = Invocation::of("vim.repeat")
        .unwrap()
        .context(context(vim.state()));
    let dispatched = vim.dispatch(&repeat).unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    assert_eq!(vim.text(), "XY two\nXY four");
    assert_eq!(vim.mode(), "normal");
    assert_eq!(vim.cursor(), (1, 1));
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "XY two\nthree four");

    // A put from `1` repeats from `2`, then `3`.
    let mut vim = HeadlessVim::new("a\nb\nc\nd", (0, 0));
    vim.feed("dddddd\"1pu.").unwrap();
    assert_eq!(vim.text(), "d\nb");
    vim.feed("u.").unwrap();
    assert_eq!(vim.text(), "d\na");

    // The repeating actor's origin, not the original's.
    let mut vim = HeadlessVim::new("one two three", (0, 0));
    vim.feed("dw").unwrap();
    let events = vim.model_mut().buffer_mut().subscribe();
    let ai = Actor::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    let repeat = Invocation::of("vim.repeat")
        .unwrap()
        .actor(ai)
        .context(context(vim.state()));
    vim.dispatch(&repeat).unwrap();
    assert_eq!(vim.text(), "three");
    let origins: Vec<Origin> = events
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            BufferEvent::Edited { origin, .. } => Some(origin),
            _ => None,
        })
        .collect();
    assert_eq!(
        origins,
        [Origin::Ai {
            agent: "agent".into(),
            session: "session".into()
        }]
    );
    // The user's `u` undoes the user's `dw` and leaves the AI's.
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "one three");
}

#[test]
fn a_failed_command_keeps_the_last_change() {
    let mut vim = HeadlessVim::new("abc def", (0, 0));
    vim.feed("x").unwrap();
    let big = Register::new("y".repeat(1 << 20), false);
    let mut registers = vim.registers().clone();
    registers
        .write(Some('a'), Write::Yank, big, vim.host_mut())
        .unwrap();
    *vim.registers_mut() = registers;
    assert!(matches!(vim.feed("100\"ap"), Err(FeedError::Dispatch(_))));
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "c def");
}
