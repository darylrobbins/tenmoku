use fe_command::{Actor, Dispatched, Effect, Invocation, Value};
use serde_json::json;

use super::*;
use crate::commands::{self, EditorTarget, HeadlessHost};

/// The GPUI view's action names, in snake_case.
const ACTIONS: [&str; 41] = [
    "move_left",
    "move_right",
    "move_up",
    "move_down",
    "move_word_left",
    "move_word_right",
    "move_line_start",
    "move_line_end",
    "move_document_start",
    "move_document_end",
    "page_up",
    "page_down",
    "select_left",
    "select_right",
    "select_up",
    "select_down",
    "select_word_left",
    "select_word_right",
    "select_line_start",
    "select_line_end",
    "select_document_start",
    "select_document_end",
    "select_page_up",
    "select_page_down",
    "select_all",
    "backspace",
    "delete",
    "delete_word_left",
    "newline",
    "tab",
    "fold",
    "unfold",
    "fold_all",
    "unfold_all",
    "toggle_soft_wrap",
    "copy",
    "cut",
    "paste",
    "undo",
    "redo",
    "save",
];

fn invocation(command: &str, args: Value) -> Invocation {
    Invocation::of(command)
        .unwrap()
        .args(args)
        .context(commands::context())
}

fn dispatch(
    model: &mut EditorModel,
    host: &mut HeadlessHost,
    command: &str,
    args: Value,
) -> Dispatched<TransactionId> {
    commands::registry()
        .dispatch(
            &mut EditorTarget::new(model, host),
            &invocation(command, args),
        )
        .unwrap_or_else(|error| panic!("{command}: {error}"))
}

/// The text, the selections, the display rows (so folds and wraps count)
/// and whether soft wrap is on.
fn state(model: &EditorModel) -> (String, u32, bool) {
    (
        render(model),
        model.display().row_count(),
        model.soft_wrap(),
    )
}

#[test]
fn every_editor_command_is_registered() {
    let registry = commands::registry();
    let ids: Vec<_> = registry
        .commands()
        .map(|meta| meta.id.to_string())
        .collect();
    let mut expected: Vec<_> = ACTIONS
        .iter()
        .map(|name| format!("editor.{name}"))
        .collect();
    expected.extend(["editor.insert".to_string(), "editor.move".to_string()]);
    assert_eq!(ids, expected);
    let mutating = [
        "backspace",
        "delete",
        "delete_word_left",
        "newline",
        "tab",
        "cut",
        "paste",
        "undo",
        "redo",
        "insert",
    ];
    for meta in registry.commands() {
        let name = meta.id.as_str().strip_prefix("editor.").unwrap();
        assert_eq!(
            meta.context.as_ref().map(ToString::to_string).as_deref(),
            Some("editor")
        );
        assert!(!meta.title.is_empty(), "{name}");
        assert_eq!(meta.mutates_buffer, mutating.contains(&name), "{name}");
        let typed = matches!(name, "insert" | "move");
        assert_eq!(meta.args_schema.is_some(), typed, "{name}");
    }
}

#[test]
fn headless_dispatch_matches_model_calls() {
    let marked = "fn a() {\n    le|t x = 1;\n    [foo]\n}\n\nfn b() {\n    x|\n}\n";
    let check = |command: &str, args: Value, direct: &dyn Fn(&mut EditorModel)| {
        let mut expected = parse(marked);
        direct(&mut expected);
        let mut model = parse(marked);
        let mut host = HeadlessHost::new();
        let dispatched = dispatch(&mut model, &mut host, command, args);
        assert_eq!(state(&model), state(&expected), "{command}");
        dispatched
    };
    let movements = [
        ("left", Movement::Left),
        ("right", Movement::Right),
        ("up", Movement::Up),
        ("down", Movement::Down),
        ("word_left", Movement::WordLeft),
        ("word_right", Movement::WordRight),
        ("line_start", Movement::LineStart),
        ("line_end", Movement::LineEnd),
        ("document_start", Movement::DocumentStart),
        ("document_end", Movement::DocumentEnd),
        ("page_up", Movement::PageUp(20)),
        ("page_down", Movement::PageDown(20)),
    ];
    for (name, movement) in movements {
        for extend in [false, true] {
            let command = match (extend, name.starts_with("page_")) {
                (false, true) => format!("editor.{name}"),
                (false, false) => format!("editor.move_{name}"),
                (true, _) => format!("editor.select_{name}"),
            };
            let dispatched = check(&command, Value::Null, &|model| {
                model.move_cursors(movement, extend)
            });
            assert_eq!(dispatched, Dispatched::new(Effect::Changed));
        }
    }
    check("editor.select_all", Value::Null, &|model| {
        model.select_all()
    });

    // Paging moves the host's page of rows.
    let tall = format!("|{}", "row\n".repeat(10));
    let mut model = parse(&tall);
    let mut host = HeadlessHost {
        page_rows: 3,
        ..HeadlessHost::new()
    };
    dispatch(&mut model, &mut host, "editor.page_down", Value::Null);
    let mut expected = parse(&tall);
    expected.move_cursors(Movement::PageDown(3), false);
    assert_eq!(render(&model), render(&expected));

    check(
        "editor.move",
        json!({"motion": "word_right", "count": 3}),
        &|model| {
            for _ in 0..3 {
                model.move_cursors(Movement::WordRight, false);
            }
        },
    );
    // A huge count stops once the carets stop moving.
    check(
        "editor.move",
        json!({"motion": "right", "count": u32::MAX}),
        &|model| model.move_cursors(Movement::DocumentEnd, false),
    );
    check(
        "editor.move",
        json!({"motion": "down", "extend": true}),
        &|model| model.move_cursors(Movement::Down, true),
    );
    check("editor.insert", json!({"text": "zz"}), &|model| {
        model.insert("zz")
    });
    for (name, direct) in [
        ("backspace", EditorModel::backspace as fn(&mut EditorModel)),
        ("delete", EditorModel::delete_forward),
        ("delete_word_left", EditorModel::delete_word_left),
        ("newline", EditorModel::newline),
        ("tab", EditorModel::tab),
    ] {
        check(&format!("editor.{name}"), Value::Null, &direct);
    }

    // The clipboard is the host's.
    let mut model = parse(marked);
    let mut host = HeadlessHost::new();
    let copied = dispatch(&mut model, &mut host, "editor.copy", Value::Null);
    assert_eq!(copied, Dispatched::new(Effect::Nothing));
    assert_eq!(host.clipboard, Some(parse(marked).copy_text().0));
    assert_eq!(
        dispatch(&mut model, &mut host, "editor.paste", Value::Null).effect,
        Effect::Changed
    );
    let mut expected = parse(marked);
    expected.paste(&expected.copy_text().0);
    assert_eq!(state(&model), state(&expected));
    let cut = dispatch(&mut model, &mut host, "editor.cut", Value::Null);
    assert_eq!(cut.effect, Effect::Changed);
    assert_eq!(host.clipboard, Some(expected.cut()));
    assert_eq!(state(&model), state(&expected));
    dispatch(
        &mut model,
        &mut host,
        "editor.move_document_end",
        Value::Null,
    );
    dispatch(&mut model, &mut host, "editor.paste", Value::Null);
    expected.move_cursors(Movement::DocumentEnd, false);
    expected.paste(&host.clipboard.clone().unwrap());
    assert_eq!(state(&model), state(&expected));
    // An empty clipboard pastes nothing.
    let mut empty = HeadlessHost::new();
    assert_eq!(
        dispatch(&mut model, &mut empty, "editor.paste", Value::Null).effect,
        Effect::Nothing
    );
    assert_eq!(state(&model), state(&expected));

    // Folds: with no syntax, indented blocks.
    let no_candidates = |_: u32| None;
    check("editor.fold", Value::Null, &|model| {
        model.fold(&no_candidates)
    });
    check("editor.fold_all", Value::Null, &|model| model.fold_all([]));
    let folded = |model: &mut EditorModel| {
        model.fold_all([]);
        model.set_cursor(model.text().len());
    };
    for (command, direct) in [
        ("editor.unfold", EditorModel::unfold as fn(&mut EditorModel)),
        ("editor.unfold_all", EditorModel::unfold_all),
    ] {
        let mut model = parse(marked);
        folded(&mut model);
        model.set_cursor(0);
        let mut expected = parse(marked);
        folded(&mut expected);
        expected.set_cursor(0);
        let rows = model.display().row_count();
        dispatch(&mut model, &mut HeadlessHost::new(), command, Value::Null);
        direct(&mut expected);
        assert!(model.display().row_count() > rows, "{command}");
        assert_eq!(state(&model), state(&expected), "{command}");
    }

    let wrapped = check("editor.toggle_soft_wrap", Value::Null, &|model| {
        model.toggle_soft_wrap()
    });
    assert_eq!(wrapped.effect, Effect::Changed);

    // Undo and redo.
    let mut model = parse(marked);
    let mut expected = parse(marked);
    let mut host = HeadlessHost::new();
    dispatch(&mut model, &mut host, "editor.insert", json!({"text": "1"}));
    dispatch(&mut model, &mut host, "editor.newline", Value::Null);
    expected.insert("1");
    expected.newline();
    for (command, direct) in [
        (
            "editor.undo",
            (|model: &mut EditorModel| drop(model.undo())) as fn(&mut EditorModel),
        ),
        ("editor.undo", |model| drop(model.undo())),
        ("editor.redo", |model| drop(model.redo())),
    ] {
        assert_eq!(
            dispatch(&mut model, &mut host, command, Value::Null).effect,
            Effect::Changed
        );
        direct(&mut expected);
        assert_eq!(state(&model), state(&expected), "{command}");
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.txt");
    std::fs::write(&path, "old").unwrap();
    let mut model = EditorModel::open(&path).unwrap();
    let mut host = HeadlessHost::new();
    dispatch(
        &mut model,
        &mut host,
        "editor.insert",
        json!({"text": "new "}),
    );
    let saved = dispatch(&mut model, &mut host, "editor.save", Value::Null);
    assert_eq!(saved, Dispatched::new(Effect::Notify));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new old");
    assert!(!model.is_dirty());
}

#[test]
fn actors_stamp_and_undo_their_own_transactions() {
    let registry = commands::registry();
    let ai = Actor::Ai {
        agent: "claude".into(),
        session: "s1".into(),
    };
    let mut model = parse("a|b");
    let mut host = HeadlessHost::new();
    let mut run = |command: &str, args: Value, actor: &Actor| {
        let invocation = invocation(command, args).actor(actor.clone());
        registry
            .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
            .unwrap()
    };

    let user = run("editor.insert", json!({"text": "U"}), &Actor::User);
    assert_eq!(user.transactions.len(), 1);
    let moved = run("editor.move", json!({"motion": "document_end"}), &ai);
    assert_eq!(moved.transactions, []);
    let by_ai = run("editor.insert", json!({"text": "X"}), &ai);
    assert_eq!(by_ai.transactions.len(), 1);
    assert_ne!(by_ai.transactions, user.transactions);

    let undone = run("editor.undo", Value::Null, &Actor::User);
    assert_eq!(undone.transactions, user.transactions);
    assert_eq!(model.text().to_string(), "abX");
    let other_ai = Actor::Ai {
        agent: "claude".into(),
        session: "s2".into(),
    };
    let mut run = |command: &str, actor: &Actor| {
        let invocation = invocation(command, Value::Null).actor(actor.clone());
        registry
            .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
            .unwrap()
    };
    // Another session has nothing to undo.
    assert_eq!(run("editor.undo", &other_ai).transactions, []);
    let undone = run("editor.undo", &ai);
    assert_eq!(undone.transactions, by_ai.transactions);
    assert_eq!(model.text().to_string(), "ab");

    // Outside a command the model edits and undoes as the user, as before.
    model.insert("u");
    assert_eq!(model.text().to_string(), "abu");
    assert!(model.undo().unwrap().is_some());
    assert_eq!(model.text().to_string(), "ab");
}

#[test]
fn recorded_invocations_replay() {
    let marked = "one |two\nthree four\n";
    let mut registry = commands::registry();
    let recorded = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    registry.observe({
        let recorded = recorded.clone();
        move |invocation| {
            let mut recorded = recorded.borrow_mut();
            recorded.push_str(&serde_json::to_string(invocation).unwrap());
            recorded.push('\n');
        }
    });
    let mut a = parse(marked);
    let mut host = HeadlessHost::new();
    for (command, args) in [
        ("editor.insert", json!({"text": "big "})),
        ("editor.move", json!({"motion": "word_right"})),
        ("editor.select_word_left", Value::Null),
        ("editor.cut", Value::Null),
        (
            "editor.move",
            json!({"motion": "down", "extend": false, "count": 1}),
        ),
        ("editor.move_line_end", Value::Null),
        ("editor.paste", Value::Null),
        (
            "editor.move",
            json!({"motion": "line_start", "extend": true}),
        ),
        ("editor.insert", json!({"text": "x"})),
        ("editor.undo", Value::Null),
    ] {
        registry
            .dispatch(
                &mut EditorTarget::new(&mut a, &mut host),
                &invocation(command, args),
            )
            .unwrap();
    }
    let recorded = recorded.borrow().clone();
    assert_eq!(recorded.lines().count(), 10);

    let replay = commands::registry();
    let mut b = parse(marked);
    let mut host = HeadlessHost::new();
    for line in recorded.lines() {
        let invocation: Invocation = serde_json::from_str(line).unwrap();
        replay
            .dispatch(&mut EditorTarget::new(&mut b, &mut host), &invocation)
            .unwrap();
    }
    assert_eq!(render(&b), render(&a));
    assert_ne!(render(&b), marked);
}
