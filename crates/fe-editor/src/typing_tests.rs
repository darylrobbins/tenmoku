use std::{cell::Cell, rc::Rc};

use fe_buffer::Origin;
use fe_command::Value;
use serde_json::json;

use super::*;
use crate::{
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

/// A clock the test moves by hand.
fn clock(model: &mut EditorModel) -> Rc<Cell<Instant>> {
    let now = Rc::new(Cell::new(Instant::now()));
    let read = Rc::clone(&now);
    model.set_clock(Rc::new(move || read.get()));
    now
}

fn wait(now: &Cell<Instant>, ms: u64) {
    now.set(now.get() + Duration::from_millis(ms));
}

fn run(model: &mut EditorModel, command: &str, args: Value) {
    let invocation = fe_command::Invocation::of(command)
        .unwrap()
        .args(args)
        .context(commands::context());
    let mut host = HeadlessHost::new();
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap_or_else(|error| panic!("{command}: {error}"));
}

/// Types `text` a char at a time, as the user, 100 ms apart.
fn type_text(model: &mut EditorModel, now: &Cell<Instant>, text: &str) {
    for ch in text.chars() {
        wait(now, 100);
        run(model, "editor.insert", json!({ "text": ch.to_string() }));
    }
}

fn undo(model: &mut EditorModel) {
    run(model, "editor.undo", Value::Null);
}

fn redo(model: &mut EditorModel) {
    run(model, "editor.redo", Value::Null);
}

#[test]
fn one_step() {
    let mut model = parse("say |!");
    let now = clock(&mut model);
    for ch in ["h", "e", "l", "l", "o"] {
        wait(&now, 150);
        model.insert(ch);
    }
    assert_eq!(render(&model), "say hello|!");
    model.undo().unwrap();
    assert_eq!(
        render(&model),
        "say |!",
        "one step, caret where typing began"
    );
    assert_eq!(model.undo().unwrap(), None, "nothing else to undo");
    model.redo().unwrap();
    assert_eq!(render(&model), "say hello|!");
    model.undo().unwrap();
    assert_eq!(render(&model), "say |!");

    // Through the registry, as the host dispatches keystrokes.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "hello");
    undo(&mut model);
    assert_eq!(render(&model), "|");
    redo(&mut model);
    assert_eq!(render(&model), "hello|");
}

#[test]
fn breaks() {
    // A caret move: away and back again still ends the run.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    run(&mut model, "editor.move_left", Value::Null);
    run(&mut model, "editor.move_right", Value::Null);
    type_text(&mut model, &now, "cd");
    undo(&mut model);
    assert_eq!(render(&model), "ab|");
    undo(&mut model);
    assert_eq!(render(&model), "|");

    // A pause of a second.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    wait(&now, 900);
    type_text(&mut model, &now, "cd");
    undo(&mut model);
    assert_eq!(render(&model), "ab|");
    undo(&mut model);
    assert_eq!(render(&model), "|");

    // Just under a second keeps it.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    wait(&now, 899);
    type_text(&mut model, &now, "cd");
    undo(&mut model);
    assert_eq!(render(&model), "|");

    // A newline is its own step, and the typing after it another.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    run(&mut model, "editor.newline", Value::Null);
    type_text(&mut model, &now, "cd");
    assert_eq!(render(&model), "ab\ncd|");
    undo(&mut model);
    assert_eq!(render(&model), "ab\n|");
    undo(&mut model);
    assert_eq!(render(&model), "ab|");
    undo(&mut model);
    assert_eq!(render(&model), "|");
    // So is a line break typed as text.
    type_text(&mut model, &now, "ab\ncd");
    undo(&mut model);
    assert_eq!(render(&model), "ab\n|");
    undo(&mut model);
    assert_eq!(render(&model), "ab|");

    // A new word: "foo " then "bar".
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "foo bar");
    undo(&mut model);
    assert_eq!(render(&model), "foo |");
    undo(&mut model);
    assert_eq!(render(&model), "|");

    // A paste is its own step, and ends the run.
    let mut model = parse("|");
    let now = clock(&mut model);
    let mut host = HeadlessHost {
        clipboard: Some("XY".to_string()),
        ..HeadlessHost::new()
    };
    type_text(&mut model, &now, "ab");
    let invocation = fe_command::Invocation::of("editor.paste")
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap();
    type_text(&mut model, &now, "cd");
    assert_eq!(render(&model), "abXYcd|");
    undo(&mut model);
    assert_eq!(render(&model), "abXY|");
    undo(&mut model);
    assert_eq!(render(&model), "ab|");
    undo(&mut model);
    assert_eq!(render(&model), "|");
}

#[test]
fn deletes_group() {
    let mut model = parse("hello|");
    let now = clock(&mut model);
    for _ in 0..3 {
        wait(&now, 100);
        run(&mut model, "editor.backspace", Value::Null);
    }
    assert_eq!(render(&model), "he|");
    type_text(&mut model, &now, "y");
    assert_eq!(render(&model), "hey|");
    undo(&mut model);
    assert_eq!(render(&model), "he|", "typing after the deletes is a step");
    undo(&mut model);
    assert_eq!(render(&model), "hello|", "three backspaces, one step");
    undo(&mut model);
    assert_eq!(render(&model), "hello|", "nothing more to undo");

    // Deleting forward and by word group too.
    let mut model = parse("one two |three");
    let now = clock(&mut model);
    for _ in 0..2 {
        wait(&now, 100);
        run(&mut model, "editor.delete", Value::Null);
    }
    for _ in 0..2 {
        wait(&now, 100);
        run(&mut model, "editor.delete_word_left", Value::Null);
    }
    assert_eq!(render(&model), "|ree");
    undo(&mut model);
    assert_eq!(model.text().to_string(), "one two three");
    assert_eq!(model.undo().unwrap(), None, "one step");

    // Deleting a selection isn't typing: its own step.
    let mut model = parse("ab[cd]");
    let now = clock(&mut model);
    run(&mut model, "editor.backspace", Value::Null);
    wait(&now, 100);
    run(&mut model, "editor.backspace", Value::Null);
    undo(&mut model);
    assert_eq!(render(&model), "ab|");
    undo(&mut model);
    assert_eq!(render(&model), "abcd|");
}

#[test]
fn other_origins() {
    // An AI edit elsewhere between keystrokes keeps the run; undo takes
    // only the typing.
    let mut model = parse("top\n|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    let ai = Origin::Ai {
        agent: "agent".into(),
        session: "s".into(),
    };
    model
        .buffer_mut()
        .edit([(0..0, "AI ")], ai.clone())
        .unwrap();
    type_text(&mut model, &now, "cd");
    assert_eq!(render(&model), "AI top\nabcd|");
    undo(&mut model);
    assert_eq!(render(&model), "AI top\n|");
    undo(&mut model);
    assert_eq!(
        render(&model),
        "AI top\n|",
        "the AI's edit isn't the user's"
    );

    // A user edit through a split ends it.
    let mut model = parse("top\n|");
    let now = clock(&mut model);
    let mut split = model.split();
    type_text(&mut model, &now, "ab");
    split.set_cursor(0);
    split.insert("X");
    type_text(&mut model, &now, "cd");
    assert_eq!(render(&model), "Xtop\nabcd|");
    undo(&mut model);
    assert_eq!(render(&model), "Xtop\nab|");

    // So does a user edit made straight to the buffer.
    let mut model = parse("top\n|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    model
        .buffer_mut()
        .edit([(0..0, "X")], Origin::User)
        .unwrap();
    type_text(&mut model, &now, "cd");
    undo(&mut model);
    assert_eq!(render(&model), "Xtop\nab|");

    // An undo: typing after it is a new step.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab ");
    type_text(&mut model, &now, "cd");
    undo(&mut model);
    type_text(&mut model, &now, "ef");
    assert_eq!(render(&model), "ab ef|");
    undo(&mut model);
    assert_eq!(render(&model), "ab |");
    undo(&mut model);
    assert_eq!(render(&model), "|");

    // A redo.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab ");
    type_text(&mut model, &now, "cd");
    undo(&mut model);
    redo(&mut model);
    type_text(&mut model, &now, "ef");
    assert_eq!(render(&model), "ab cdef|");
    undo(&mut model);
    assert_eq!(render(&model), "ab cd|");

    // A save, from this model or a split.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("typed.txt");
    std::fs::write(&path, "").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    let now = clock(&mut model);
    let mut split = model.split();
    type_text(&mut model, &now, "ab");
    run(&mut model, "editor.save", Value::Null);
    type_text(&mut model, &now, "cd");
    split.save().unwrap();
    type_text(&mut model, &now, "ef");
    undo(&mut model);
    assert_eq!(render(&model), "abcd|");
    undo(&mut model);
    assert_eq!(render(&model), "ab|");
    undo(&mut model);
    assert_eq!(render(&model), "|");
}

#[test]
fn carets_and_transactions() {
    // Three carets: one step per run.
    let mut model = parse("|\n|\n|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab ");
    type_text(&mut model, &now, "cd");
    assert_eq!(render(&model), "ab cd|\nab cd|\nab cd|");
    undo(&mut model);
    assert_eq!(render(&model), "ab |\nab |\nab |");
    undo(&mut model);
    assert_eq!(render(&model), "|\n|\n|");

    // With a transaction open, typing joins it, and no run outlives it.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "x");
    model.buffer_mut().start_transaction(Origin::User);
    type_text(&mut model, &now, "ab cd");
    model.buffer_mut().end_transaction();
    type_text(&mut model, &now, "ef");
    assert_eq!(render(&model), "xab cdef|");
    undo(&mut model);
    assert_eq!(render(&model), "xab cd|");
    undo(&mut model);
    assert_eq!(render(&model), "x|");
    undo(&mut model);
    assert_eq!(render(&model), "|");

    // A tab is typing; it's blank, so the word after it starts a step.
    let mut model = parse("|");
    let now = clock(&mut model);
    type_text(&mut model, &now, "ab");
    wait(&now, 100);
    run(&mut model, "editor.tab", Value::Null);
    type_text(&mut model, &now, "cd");
    assert_eq!(render(&model), "ab  cd|");
    undo(&mut model);
    assert_eq!(render(&model), "ab  |");
    undo(&mut model);
    assert_eq!(render(&model), "|");
}
