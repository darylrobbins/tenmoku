use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, Origin, Text};
use fe_command::{Actor, Effect, Invocation, Value};
use fe_display::CharWidth;
use serde_json::json;

use crate::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
    completion::{CompletionItem, CompletionList, CompletionSession},
    tests::{parse, render},
};

/// Ten rows, the caret on the third.
const ROWS: &str = "r0\nr1\nr|2\nr3\nr4\nr5\nr6\nr7\nr8\nr9";

fn run_as(model: &mut EditorModel, actor: Actor, command: &str, args: Value) -> Effect {
    let invocation = Invocation::of(command)
        .unwrap()
        .args(args)
        .context(commands::context())
        .actor(actor);
    commands::registry()
        .dispatch(
            &mut EditorTarget::new(model, &mut HeadlessHost::new()),
            &invocation,
        )
        .unwrap_or_else(|error| panic!("{command}: {error}"))
        .effect
}

fn go(model: &mut EditorModel) -> Effect {
    let command = "editor.go_to_last_edit_location";
    run_as(model, Actor::User, command, Value::Null)
}

/// Puts the caret at the start of `row`.
fn caret_to(model: &mut EditorModel, row: u32) {
    let offset = model.text().row_start(row);
    model.set_cursor(offset);
}

fn ai() -> Actor {
    Actor::Ai {
        agent: "agent".into(),
        session: "s".into(),
    }
}

#[test]
fn goes_to_the_last_change() {
    let mut model = parse(ROWS);
    model.insert("xy");
    let edited = "r0\nr1\nrxy|2\nr3\nr4\nr5\nr6\nr7\nr8\nr9";
    assert_eq!(render(&model), edited);
    caret_to(&mut model, 9);
    model.fold_rows(1..=3);

    assert_eq!(go(&mut model), Effect::Changed);
    assert_eq!(render(&model), edited);
    assert!(model.display().folds().is_empty(), "the fold opened");
    assert_eq!(go(&mut model), Effect::Nothing);
    assert_eq!(render(&model), edited);

    // A row command's edit is the user's too.
    caret_to(&mut model, 5);
    run_as(&mut model, Actor::User, "editor.indent", Value::Null);
    caret_to(&mut model, 0);
    go(&mut model);
    assert_eq!(
        render(&model),
        "r0\nr1\nrxy2\nr3\nr4\n    |r5\nr6\nr7\nr8\nr9"
    );
}

#[test]
fn follows_later_edits() {
    let mut model = parse(ROWS);
    model.insert("xy");
    caret_to(&mut model, 9);
    // Not the user's, so the place stays the typing's.
    model.buffer_mut().edit([(0..0, "new\n")], Origin::Lsp);
    go(&mut model);
    assert_eq!(
        render(&model),
        "new\nr0\nr1\nrxy|2\nr3\nr4\nr5\nr6\nr7\nr8\nr9"
    );

    caret_to(&mut model, 9);
    let row = model.text().row_start(3);
    model
        .buffer_mut()
        .edit([(row..row + "rxy2\nr3\n".len(), "")], Origin::Lsp);
    go(&mut model);
    assert_eq!(render(&model), "new\nr0\nr1\n|r4\nr5\nr6\nr7\nr8\nr9");
}

#[test]
fn ignores_non_user_edits() {
    // With no edit, nothing happens.
    let mut model = parse(ROWS);
    assert_eq!(go(&mut model), Effect::Nothing);
    assert_eq!(render(&model), ROWS);

    // Typing on rows 2 and 5; the place is row 5's.
    model.insert("a");
    caret_to(&mut model, 5);
    model.insert("b");
    let typed = "r0\nr1\nra2\nr3\nr4\nb|r5\nr6\nr7\nr8\nr9";
    assert_eq!(render(&model), typed);

    // Undoing row 2's typing and redoing it don't move it there.
    model.undo().unwrap();
    model.undo().unwrap();
    caret_to(&mut model, 9);
    go(&mut model);
    assert_eq!(render(&model), "r0\nr1\nr2\nr3\nr4\n|r5\nr6\nr7\nr8\nr9");
    model.redo().unwrap();
    caret_to(&mut model, 9);
    go(&mut model);
    assert_eq!(render(&model), "r0\nr1\nra2\nr3\nr4\n|r5\nr6\nr7\nr8\nr9");
    // Redoing row 5's typing puts back the place it made.
    model.redo().unwrap();
    assert_eq!(render(&model), typed);

    // Formatting, the AI and a reload don't either.
    caret_to(&mut model, 7);
    run_as(&mut model, Actor::Format, "editor.indent", Value::Null);
    caret_to(&mut model, 8);
    run_as(&mut model, ai(), "editor.insert", json!({"text": "c"}));
    run_as(&mut model, ai(), "editor.move_rows_up", Value::Null);
    let reloaded = "r0\nr1\nra2\nr3\nr4\nbr5\nr6\ncr8\n    r7\nr9 reloaded";
    let file = Text::from_reader_detecting(reloaded.as_bytes()).unwrap();
    model.reload(file, None);
    caret_to(&mut model, 0);
    go(&mut model);
    let after_b = "r0\nr1\nra2\nr3\nr4\nb|r5\nr6\ncr8\n    r7\nr9 reloaded";
    assert_eq!(render(&model), after_b);

    // A split of the buffer shares the place, and sets it for both.
    let mut split = model.split();
    caret_to(&mut split, 9);
    go(&mut split);
    assert_eq!(render(&split), after_b);
    caret_to(&mut split, 1);
    split.insert("d");
    caret_to(&mut model, 9);
    go(&mut model);
    assert_eq!(
        render(&model),
        "r0\nd|r1\nra2\nr3\nr4\nbr5\nr6\ncr8\n    r7\nr9 reloaded"
    );

    // Another buffer's model has none.
    let mut other = EditorModel::new(Buffer::new("x"), None);
    assert_eq!(go(&mut other), Effect::Nothing);
}

#[test]
fn accepting_a_completion_sets_it() {
    // With no edit yet, and over one.
    for edited_before in [false, true] {
        let mut model = parse("r0\nr1\nfo|");
        if edited_before {
            caret_to(&mut model, 0);
            model.insert("x");
            let end = model.text().len();
            model.set_cursor(end);
        }
        let start = model.text().len() - "fo".len();
        let item = CompletionItem {
            label: "foo".into(),
            edit: (start..start + 2, "foo".into()),
            ..CompletionItem::default()
        };
        let list = CompletionList {
            items: vec![item],
            is_incomplete: false,
        };
        let head = model.primary_selection().head();
        let session = CompletionSession::new(model.snapshot().clone(), head, list);
        session.accept(&mut model).unwrap();
        caret_to(&mut model, 1);
        go(&mut model);
        let row0 = if edited_before { "xr0" } else { "r0" };
        assert_eq!(render(&model), format!("{row0}\nr1\nfoo|"));
    }
}

/// An edit for [`EditsOnce`] to make.
type Hook = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

/// Widths that make the hooked edit when first measuring a non-ASCII char.
struct EditsOnce(Hook);

impl CharWidth for EditsOnce {
    fn char_width(&mut self, ch: char) -> f32 {
        let edit = if ch.is_ascii() {
            None
        } else {
            self.0.borrow_mut().take()
        };
        if let Some(edit) = edit {
            edit();
        }
        1.
    }
}

#[test]
fn an_edit_made_while_catching_up_is_the_last() {
    for row_command in [false, true] {
        let a = Rc::new(RefCell::new(parse("|foo\nbąr")));
        let mut b = a.borrow().split();
        let hook = Hook::default();
        b.set_char_widths(Box::new(EditsOnce(hook.clone())));
        // Wrapping is what measures; nothing is long enough to wrap.
        b.set_soft_wrap(true);
        caret_to(&mut b, 1);
        // Measuring b's edited row as it catches up after the edit puts
        // "Z" at a's caret: that's the newer edit.
        let hooked = a.clone();
        *hook.borrow_mut() = Some(Box::new(move || hooked.borrow_mut().insert("Z")));
        match row_command {
            false => b.insert("X"),
            true => _ = run_as(&mut b, Actor::User, "editor.indent", Value::Null),
        }
        assert!(hook.borrow().is_none(), "the hook ran");
        go(&mut b);
        let bar = if row_command { "    bąr" } else { "Xbąr" };
        assert_eq!(render(&b), format!("Z|foo\n{bar}"));
    }
}
