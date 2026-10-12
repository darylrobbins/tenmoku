use fe_command::{Actor, DispatchError, Dispatched, Effect, Invocation, Value};
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
    // The multi-cursor commands, which no GPUI action has yet; none edits.
    expected.extend(
        [
            "add_cursor_above",
            "add_cursor_below",
            "select_next_occurrence",
            "select_all_occurrences",
            "split_selection_into_lines",
            "single_selection",
        ]
        .map(|name| format!("editor.{name}")),
    );
    // The row commands, which no key binds yet; each edits.
    let rows = [
        "indent",
        "outdent",
        "move_rows_up",
        "move_rows_down",
        "duplicate_rows",
        "delete_rows",
    ];
    expected.extend(rows.map(|name| format!("editor.{name}")));
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
        "indent",
        "outdent",
        "move_rows_up",
        "move_rows_down",
        "duplicate_rows",
        "delete_rows",
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
    // The largest count stops once the carets stop moving.
    check(
        "editor.move",
        json!({"motion": "right", "count": commands::MAX_MOVE_COUNT}),
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

/// A host whose clipboard panics, as a buggy embedding might.
struct PanickingHost;

impl commands::EditorHost for PanickingHost {
    fn page_rows(&self) -> u32 {
        20
    }

    fn read_clipboard(&mut self) -> Option<String> {
        panic!("clipboard unavailable")
    }

    fn write_clipboard(&mut self, _: String) {}

    fn fold_range_at(&self, _: u32) -> Option<std::ops::RangeInclusive<u32>> {
        None
    }

    fn fold_ranges(&self, _: std::ops::Range<u32>) -> Vec<std::ops::RangeInclusive<u32>> {
        Vec::new()
    }
}

#[test]
fn a_caught_panic_leaves_the_model_acting_as_the_user() {
    let registry = commands::registry();
    let ai = Actor::Ai {
        agent: "claude".into(),
        session: "s1".into(),
    };
    let mut model = parse("|");
    model.insert("U");
    let by_ai = registry
        .dispatch(
            &mut EditorTarget::new(&mut model, &mut HeadlessHost::new()),
            &invocation("editor.insert", json!({"text": "X"})).actor(ai.clone()),
        )
        .unwrap();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        registry.dispatch(
            &mut EditorTarget::new(&mut model, &mut PanickingHost),
            &invocation("editor.paste", Value::Null).actor(ai.clone()),
        )
    }));
    assert!(panicked.is_err());

    // A direct edit is the user's, so the agent's undo passes over it.
    model.insert("u");
    let undone = registry
        .dispatch(
            &mut EditorTarget::new(&mut model, &mut HeadlessHost::new()),
            &invocation("editor.undo", Value::Null).actor(ai),
        )
        .unwrap();
    assert_eq!(undone.transactions, by_ai.transactions);
    assert_eq!(model.text().to_string(), "Uu");
    // And a direct undo undoes the user's edits, after that dispatch too.
    assert!(model.undo().unwrap().is_some());
    assert_eq!(model.text().to_string(), "U");
    assert!(model.undo().unwrap().is_some());
    assert_eq!(model.text().to_string(), "");
}

#[test]
fn move_counts_past_the_limit_are_bad_args() {
    let registry = commands::registry();
    let schema = registry
        .commands()
        .find(|meta| meta.id.as_str() == "editor.move")
        .and_then(|meta| meta.args_schema.clone())
        .unwrap();
    assert_eq!(
        schema["properties"]["count"]["maximum"],
        json!(commands::MAX_MOVE_COUNT),
        "{schema}"
    );

    // Text long enough that every move moves, in short rows to keep each
    // move cheap.
    let long = format!("|{}", "xxxxxxxxx\n".repeat(1_001));
    let mut model = parse(&long);
    let mut host = HeadlessHost::new();
    let most = commands::MAX_MOVE_COUNT;
    dispatch(
        &mut model,
        &mut host,
        "editor.move",
        json!({"motion": "right", "count": most}),
    );
    assert_eq!(model.primary_selection().head(), most as usize);

    let mut model = parse(&long);
    for count in [most + 1, u32::MAX] {
        let too_many = invocation("editor.move", json!({"motion": "right", "count": count}));
        assert!(matches!(
            registry.validate(&too_many),
            Err(DispatchError::BadArgs { .. })
        ));
        assert!(matches!(
            registry.dispatch(&mut EditorTarget::new(&mut model, &mut host), &too_many),
            Err(DispatchError::BadArgs { .. })
        ));
    }
    assert_eq!(render(&model), long);
}

#[test]
fn commands_that_change_nothing_report_nothing() {
    let no_ops: [(&str, &[&str]); 3] = [
        (
            "|ab\ncd",
            &[
                "move_left",
                "move_up",
                "move_word_left",
                "move_line_start",
                "move_document_start",
                "page_up",
                "select_left",
                "select_up",
                "select_word_left",
                "select_document_start",
                "select_page_up",
                "backspace",
                "delete_word_left",
                "fold",
                "fold_all",
                "unfold",
                "unfold_all",
                "undo",
                "redo",
            ],
        ),
        (
            "ab\ncd|",
            &[
                "move_right",
                "move_down",
                "move_word_right",
                "move_line_end",
                "move_document_end",
                "page_down",
                "select_right",
                "select_down",
                "select_document_end",
                "delete",
            ],
        ),
        ("[ab\ncd]", &["select_all", "select_document_end"]),
    ];
    let mut cases: Vec<(&str, String, Value)> = no_ops
        .iter()
        .flat_map(|(marked, names)| {
            names
                .iter()
                .map(|name| (*marked, format!("editor.{name}"), Value::Null))
        })
        .collect();
    cases.extend([
        (
            "|ab\ncd",
            "editor.move".to_string(),
            json!({"motion": "left", "count": 3}),
        ),
        (
            "a|b\ncd",
            "editor.move".to_string(),
            json!({"motion": "right", "count": 0}),
        ),
        ("a|b\nc|d", "editor.insert".to_string(), json!({"text": ""})),
        ("a|b\nc|d", "editor.paste".to_string(), Value::Null),
    ]);
    for (marked, command, args) in cases {
        let mut model = parse(marked);
        let before = state(&model);
        let mut host = HeadlessHost {
            clipboard: Some(String::new()),
            ..HeadlessHost::new()
        };
        let dispatched = dispatch(&mut model, &mut host, &command, args);
        assert_eq!(
            dispatched,
            Dispatched::new(Effect::Nothing),
            "{command} at {marked:?}"
        );
        assert_eq!(state(&model), before, "{command} at {marked:?}");
    }

    // The same commands report changes once there's something to change,
    // and nothing again once it's done.
    let mut model = parse("|a\n    b\n");
    let mut host = HeadlessHost::new();
    for (command, effect) in [
        ("editor.fold", Effect::Changed),
        ("editor.fold", Effect::Nothing),
        ("editor.unfold", Effect::Changed),
        ("editor.unfold", Effect::Nothing),
        ("editor.toggle_soft_wrap", Effect::Changed),
        ("editor.delete", Effect::Changed),
        ("editor.undo", Effect::Changed),
        ("editor.undo", Effect::Nothing),
        ("editor.redo", Effect::Changed),
        ("editor.redo", Effect::Nothing),
        ("editor.copy", Effect::Nothing),
    ] {
        let dispatched = dispatch(&mut model, &mut host, command, Value::Null);
        assert_eq!(dispatched.effect, effect, "{command}");
    }
}

#[test]
fn undoing_and_redoing_a_replacement_by_the_same_text_reports_nothing() {
    let mut model = parse("x|");
    model.buffer_mut().edit([(0..1, "x")], Origin::User);
    let before = state(&model);
    let mut host = HeadlessHost::new();
    for command in ["editor.undo", "editor.redo"] {
        let dispatched = dispatch(&mut model, &mut host, command, Value::Null);
        assert_eq!(dispatched.effect, Effect::Nothing, "{command}");
        // The transaction undone or redone is still reported.
        assert_eq!(dispatched.transactions.len(), 1, "{command}");
        assert_eq!(state(&model), before, "{command}");
    }
}
