use std::{
    cell::Cell,
    ops::{Range, RangeInclusive},
    rc::Rc,
    time::{Duration, Instant},
};

use fe_buffer::Origin;
use fe_command::Invocation;
use serde_json::json;

use super::*;
use crate::{
    Movement,
    commands::{self, EditorHost, EditorTarget},
    tests::{parse, render},
};

/// Brackets and double quotes, as Rust has them.
const BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
/// Those and single quotes, as Python has them.
const QUOTES: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];

/// Types `text` a char at a time into `marked`, closing from `pairs`.
fn typed(marked: &str, text: &str, pairs: &[(char, char)]) -> String {
    let mut model = parse(marked);
    type_chars(&mut model, text, pairs);
    render(&model)
}

fn type_chars(model: &mut EditorModel, text: &str, pairs: &[(char, char)]) {
    for ch in text.chars() {
        model.insert_typed(ch, pairs);
    }
}

#[test]
fn open_and_type_over() {
    // Before a space, the row's end or a closing char, an opener closes.
    assert_eq!(typed("a| b", "(", BRACKETS), "a(|) b");
    assert_eq!(typed("foo|", "(", BRACKETS), "foo(|)");
    assert_eq!(typed("x = |;", "[", BRACKETS), "x = [|];");
    assert_eq!(typed("(|)", "{", BRACKETS), "({|})");
    // And typing its closer moves over it, after typing inside or not.
    assert_eq!(typed("foo|", "()", BRACKETS), "foo()|");
    assert_eq!(typed("foo|", "(x, y)", BRACKETS), "foo(x, y)|");
    assert_eq!(typed("|", "([{}])", BRACKETS), "([{}])|");
    // Before a word char, only the opener goes in.
    assert_eq!(typed("|foo", "(", BRACKETS), "(|foo");
    // A closer that was already there is typed, not moved over.
    assert_eq!(typed("(|)", ")", BRACKETS), "()|)");
    assert_eq!(typed("|foo", "()", BRACKETS), "()|foo");
    // Nor is one whose caret left its row and came back.
    let mut model = parse("|\nb");
    type_chars(&mut model, "(", BRACKETS);
    model.move_cursors(Movement::Down, false);
    model.move_cursors(Movement::Up, false);
    assert_eq!(render(&model), "(|)\nb");
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "()|)\nb");
    // Nor one typed over already.
    let mut model = parse("|");
    type_chars(&mut model, "()", BRACKETS);
    model.move_cursors(Movement::Left, false);
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "()|)");
    // Each caret pairs, or not, on its own, in one edit.
    assert_eq!(typed("a| b|", "(", BRACKETS), "a(|) b(|)");
    let mut model = parse("|x\n|");
    type_chars(&mut model, "(", BRACKETS);
    assert_eq!(render(&model), "(|x\n(|)");
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "()|x\n()|");
}

#[test]
fn quotes_wrap_and_backspace() {
    // A quote after a word char is an apostrophe.
    assert_eq!(typed("don|", "'", QUOTES), "don'|");
    assert_eq!(typed("don|", "'t", QUOTES), "don't|");
    // At a space it pairs, and typing it again moves over.
    assert_eq!(typed("x = |", "\"", QUOTES), "x = \"|\"");
    assert_eq!(typed("x = |", "\"a\"", QUOTES), "x = \"a\"|");
    assert_eq!(typed("x = |", "'a'", QUOTES), "x = 'a'|");
    // Right after the same quote, it doesn't.
    assert_eq!(typed("\"|", "\"", QUOTES), "\"\"|");
    // Not paired where the language doesn't pair it.
    assert_eq!(typed("x = |", "'", BRACKETS), "x = '|");

    // Backspace in an untouched pair deletes both halves.
    let mut model = parse("f|");
    type_chars(&mut model, "(", BRACKETS);
    model.backspace();
    assert_eq!(render(&model), "f|");
    let mut model = parse("x = |");
    type_chars(&mut model, "\"", QUOTES);
    model.backspace();
    assert_eq!(render(&model), "x = |");
    // Emptied again, it's still the pair typed.
    let mut model = parse("|");
    type_chars(&mut model, "(a", BRACKETS);
    model.backspace();
    assert_eq!(render(&model), "(|)");
    model.backspace();
    assert_eq!(render(&model), "|");
    // A pair typing didn't insert is a char at a time.
    let mut model = parse("(|)");
    model.backspace();
    assert_eq!(render(&model), "|)");

    // An opener with a selection surrounds it, keeping it selected.
    let mut model = parse("x [foo] y");
    type_chars(&mut model, "[", BRACKETS);
    assert_eq!(model.text().to_string(), "x [foo] y");
    let selections = model.selections();
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].range, 3..6);
    assert_eq!(typed("[foo]", "(", BRACKETS), "([foo])");
    assert_eq!(typed("a [b] c [d]", "'", QUOTES), "a '[b]' c '[d]'");
    // A reversed selection stays reversed.
    let mut model = parse("ab|");
    model.move_cursors(Movement::Left, true);
    model.move_cursors(Movement::Left, true);
    type_chars(&mut model, "{", BRACKETS);
    assert_eq!(model.text().to_string(), "{ab}");
    let selection = model.selections()[0].clone();
    assert_eq!((selection.range, selection.reversed), (1..3, true));
    // A closer with a selection replaces it.
    assert_eq!(typed("[ab]", ")", BRACKETS), ")|");
}

/// A host whose language pairs `pairs`.
struct PairsHost(&'static [(char, char)]);

impl EditorHost for PairsHost {
    fn page_rows(&self) -> u32 {
        20
    }
    fn read_clipboard(&mut self) -> Option<String> {
        Some("(".to_owned())
    }
    fn write_clipboard(&mut self, _: String) {}
    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }
    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        Vec::new()
    }
    fn auto_pairs(&self) -> &[(char, char)] {
        self.0
    }
}

fn run(model: &mut EditorModel, command: &str, args: serde_json::Value) {
    let invocation = Invocation::of(command)
        .unwrap()
        .args(args)
        .context(commands::context());
    let mut host = PairsHost(BRACKETS);
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap_or_else(|error| panic!("{command}: {error}"));
}

fn insert(model: &mut EditorModel, text: &str) {
    run(model, "editor.insert", json!({ "text": text }));
}

#[test]
fn off_and_other_inserts() {
    // Typed through the command, a char closes from the host's pairs.
    let mut model = parse("|");
    insert(&mut model, "(");
    assert_eq!(render(&model), "(|)");
    // Turned off, typing is as typed.
    let mut model = parse("|");
    assert!(model.auto_close());
    model.set_auto_close(false);
    assert!(!model.auto_close());
    insert(&mut model, "(");
    assert_eq!(render(&model), "(|");
    type_chars(&mut model, "\"[", BRACKETS);
    assert_eq!(render(&model), "(\"[|");
    // Turning it off forgets the closers typed so far.
    let mut model = parse("|");
    type_chars(&mut model, "(", BRACKETS);
    model.set_auto_close(false);
    model.set_auto_close(true);
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "()|)");
    // So with no pairs.
    assert_eq!(typed("|", "(", &[]), "(|");
    // Several chars at once, a paste, or the model's own insert are as given.
    let mut model = parse("|");
    insert(&mut model, "((");
    assert_eq!(render(&model), "((|");
    let mut model = parse("|");
    run(&mut model, "editor.paste", serde_json::Value::Null);
    assert_eq!(render(&model), "(|");
    let mut model = parse("|");
    model.insert("[");
    assert_eq!(render(&model), "[|");

    // One undo takes a typed pair away with the typing around it, typed
    // over or not.
    let mut model = parse("|");
    let now = Rc::new(Cell::new(Instant::now()));
    let read = Rc::clone(&now);
    model.set_clock(Rc::new(move || read.get()));
    for ch in "f(x".chars() {
        now.set(now.get() + Duration::from_millis(100));
        insert(&mut model, &ch.to_string());
    }
    assert_eq!(render(&model), "f(x|)");
    run(&mut model, "editor.undo", serde_json::Value::Null);
    assert_eq!(render(&model), "|");
    run(&mut model, "editor.redo", serde_json::Value::Null);
    assert_eq!(model.text().to_string(), "f(x)");
    let mut model = parse("|");
    let now = Rc::new(Cell::new(Instant::now()));
    let read = Rc::clone(&now);
    model.set_clock(Rc::new(move || read.get()));
    for ch in "g(y)".chars() {
        now.set(now.get() + Duration::from_millis(100));
        insert(&mut model, &ch.to_string());
    }
    assert_eq!(render(&model), "g(y)|");
    run(&mut model, "editor.undo", serde_json::Value::Null);
    assert_eq!(render(&model), "|");
}

#[test]
fn wrapped_pair_types_over_and_backspaces() {
    // Wrapped, the closer is moved over like a typed pair's.
    let mut model = parse("x [foo] y");
    type_chars(&mut model, "[", BRACKETS);
    model.move_cursors(Movement::Right, false);
    assert_eq!(render(&model), "x [foo|] y");
    type_chars(&mut model, "]", BRACKETS);
    assert_eq!(render(&model), "x [foo]| y");
    // And emptied, it goes with one backspace.
    let mut model = parse("x [foo] y");
    type_chars(&mut model, "(", BRACKETS);
    model.backspace();
    assert_eq!(render(&model), "x (|) y");
    model.backspace();
    assert_eq!(render(&model), "x | y");
}

/// Asserts that after `edit` to a typed `(|)`, leaving it as it was,
/// typing `)` types it and backspace deletes one char.
fn assert_forgotten(edit: impl Fn(&mut EditorModel)) {
    for backspace in [false, true] {
        let mut model = parse("|");
        type_chars(&mut model, "(", BRACKETS);
        edit(&mut model);
        assert_eq!(render(&model), "(|)");
        if backspace {
            model.backspace();
            assert_eq!(render(&model), "|)");
        } else {
            type_chars(&mut model, ")", BRACKETS);
            assert_eq!(render(&model), "()|)");
        }
    }
}

#[test]
fn edited_half_is_not_typed_over() {
    // A closer replaced by the same char through the model.
    assert_forgotten(|model| {
        model.select_range(1..2, false);
        model.insert(")");
        model.set_cursor(1);
    });
    // So through another model, while the caret stays inside.
    assert_forgotten(|model| {
        let mut other = model.split();
        other.select_range(1..2, false);
        other.insert(")");
        model.sync_display();
    });
    // Or a half changed and restored before a sync, opener or closer.
    for (half, ch) in [(0..1, "("), (1..2, ")")] {
        assert_forgotten(|model| {
            let mut buffer = model.buffer_mut();
            buffer.edit([(half.clone(), "x")], Origin::User);
            buffer.edit([(half.clone(), ch)], Origin::User);
        });
    }
    // Text put between or around the halves leaves the pair alone.
    let mut model = parse("|");
    type_chars(&mut model, "(", BRACKETS);
    model
        .buffer_mut()
        .edit([(0..0, "a"), (1..1, "b"), (2..2, "c")], Origin::User);
    model.set_cursor(3);
    assert_eq!(render(&model), "a(b|)c");
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "a(b)|c");
}

#[test]
fn leaving_the_pair_forgets_it() {
    // A caret that leaves the row and comes back, with no sync between.
    let mut model = parse("|\nb");
    type_chars(&mut model, "(", BRACKETS);
    model.set_cursor(3);
    model.set_cursor(1);
    assert_eq!(render(&model), "(|)\nb");
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "()|)\nb");
    // Or leaves the pair on its row, while another caret stays on it.
    let mut model = parse("a| b|");
    type_chars(&mut model, "(", BRACKETS);
    assert_eq!(render(&model), "a(|) b(|)");
    model.set_selections([caret(0), caret(6)]);
    model.set_selections([caret(2), caret(6)]);
    type_chars(&mut model, ")", BRACKETS);
    assert_eq!(render(&model), "a()|) b()|");
}

#[test]
fn more_than_64_carets_keep_their_pairs() {
    let marked = "|\n".repeat(65);
    let mut model = parse(&marked);
    type_chars(&mut model, "()", BRACKETS);
    assert_eq!(render(&model), "()|\n".repeat(65));
    let mut model = parse(&marked);
    type_chars(&mut model, "(", BRACKETS);
    model.backspace();
    assert_eq!(render(&model), marked);
}
