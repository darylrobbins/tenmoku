//! Key bindings dispatch through the command registries: one table binds
//! them all, keystrokes become recorded invocations, and the recordings
//! replay headlessly.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::Buffer;
use fe_command::{CommandId, ContextStack, Invocation, Source, Value};
use fe_editor::{
    EditorModel, Movement,
    commands::{EditorTarget, HeadlessHost},
};
use gpui::{Entity, Focusable as _, Keystroke, TestAppContext, VisualTestContext};

use crate::{
    commands::{RunCommand, observe_invocations},
    default_bindings::DEFAULT_BINDINGS,
    *,
};

/// A focused editor showing `text` with the caret at its end, and every
/// invocation the registries dispatch from then on.
fn recording_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (
    Entity<EditorView>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
) {
    cx.update(init);
    let recorded = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let recorded = recorded.clone();
        observe_invocations(cx, move |invocation| {
            recorded.borrow_mut().push(invocation.clone())
        });
    });
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(text.len());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx, recorded)
}

fn text_and_selections(model: &EditorModel) -> (String, Vec<fe_buffer::ResolvedSelection>) {
    (model.text().to_string(), model.selections())
}

#[gpui::test]
fn table_is_the_only_binding_source(cx: &mut TestAppContext) {
    assert_eq!(DEFAULT_BINDINGS.len(), 47);
    let editor_rows: Vec<_> = DEFAULT_BINDINGS
        .iter()
        .filter(|row| row.context == Some("editor"))
        .collect();
    assert_eq!(editor_rows.len(), 46);
    let others: Vec<_> = DEFAULT_BINDINGS
        .iter()
        .filter(|row| row.context != Some("editor"))
        .map(|row| (row.keys, row.command, row.context))
        .collect();
    assert_eq!(others, [("cmd-q", "app.quit", None)]);

    let editor_commands = fe_editor::commands::registry();
    for row in DEFAULT_BINDINGS {
        let id = CommandId::parse(row.command).unwrap();
        let known = match id.namespace() {
            "editor" => editor_commands.get(&id).is_some(),
            "app" => id.as_str() == "app.quit",
            _ => false,
        };
        assert!(known, "{} isn't a registered command", row.command);
    }

    cx.update(init);
    let normalized = |keys: &str| {
        keys.split_whitespace()
            .map(|key| Keystroke::parse(key).unwrap().unparse())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut bound: Vec<(String, Option<String>, Invocation)> = cx.update(|cx| {
        cx.key_bindings()
            .borrow()
            .bindings()
            .map(|binding| {
                let RunCommand(invocation) = binding
                    .action()
                    .as_any()
                    .downcast_ref::<RunCommand>()
                    .expect("every binding runs a command");
                let keys = binding
                    .keystrokes()
                    .iter()
                    .map(|key| key.unparse())
                    .collect::<Vec<_>>()
                    .join(" ");
                let context = binding.predicate().map(|p| p.to_string());
                (keys, context, invocation.clone())
            })
            .collect()
    });
    let mut expected: Vec<_> = DEFAULT_BINDINGS
        .iter()
        .map(|row| {
            (
                normalized(row.keys),
                row.context.map(str::to_string),
                Invocation::of(row.command)
                    .unwrap()
                    .source(Source::Keyboard),
            )
        })
        .collect();
    // One binding per row, in any order: invocations aren't `Ord`.
    let key = |(keys, context, invocation): &(String, Option<String>, Invocation)| {
        (keys.clone(), context.clone(), invocation.command.clone())
    };
    bound.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(bound, expected);
}

#[gpui::test]
fn keystrokes_dispatch_through_the_registry(cx: &mut TestAppContext) {
    let (view, cx, recorded) = recording_editor(cx, "ab");
    cx.simulate_keystrokes("x y left cmd-z");

    let insert = |text: &str| {
        Invocation::of("editor.insert")
            .unwrap()
            .args(Value::from_iter([("text", text)]))
    };
    let expected: Vec<Invocation> = [
        insert("x"),
        insert("y"),
        Invocation::of("editor.move_left").unwrap(),
        Invocation::of("editor.undo").unwrap(),
    ]
    .into_iter()
    .map(|invocation| {
        invocation
            .source(Source::Keyboard)
            .context(fe_editor::commands::context())
    })
    .collect();
    assert_eq!(*recorded.borrow(), expected);
    assert_eq!(
        expected[0].context,
        ContextStack::new().with(fe_command::ContextFrame::new("editor"))
    );

    // What the view did before keys went through the registry: the same
    // calls on the model directly.
    let mut before = EditorModel::new(Buffer::new("ab"), None);
    before.set_cursor(2);
    before.insert("x");
    before.insert("y");
    before.move_cursors(Movement::Left, false);
    before.undo().unwrap();
    let shown = view.update(cx, |view, _| text_and_selections(view.model()));
    assert_eq!(shown, text_and_selections(&before));
}

#[gpui::test]
fn recorded_keystrokes_replay_headless(cx: &mut TestAppContext) {
    let start = "one two three";
    let (view, cx, recorded) = recording_editor(cx, start);
    cx.simulate_keystrokes("cmd-left h i space alt-right shift-left cmd-x cmd-v enter cmd-z");
    let shown = view.update(cx, |view, _| text_and_selections(view.model()));
    assert_eq!(shown.0, "hi one two three");

    let json = serde_json::to_string(&*recorded.borrow()).unwrap();
    let replayed: Vec<Invocation> = serde_json::from_str(&json).unwrap();
    assert_eq!(*recorded.borrow(), replayed);
    assert!(
        replayed
            .iter()
            .any(|i| i.command.as_str() == "editor.insert")
    );
    assert_eq!(replayed.len(), 10);

    let registry = fe_editor::commands::registry();
    let mut model = EditorModel::new(Buffer::new(start), None);
    model.set_cursor(start.len());
    let mut host = HeadlessHost::new();
    for invocation in &replayed {
        registry
            .dispatch(&mut EditorTarget::new(&mut model, &mut host), invocation)
            .unwrap();
    }
    assert_eq!(text_and_selections(&model), shown);
}

#[gpui::test]
fn app_commands_propagate_from_the_editor(cx: &mut TestAppContext) {
    let (view, cx, recorded) = recording_editor(cx, "ab");
    cx.simulate_keystrokes("cmd-q");
    let commands: Vec<_> = recorded
        .borrow()
        .iter()
        .map(|i| (i.command.to_string(), i.source))
        .collect();
    assert_eq!(commands, [("app.quit".to_string(), Source::Keyboard)]);

    let unknown = RunCommand(Invocation::of("nowhere.unknown").unwrap());
    cx.update(|window, cx| {
        assert!(view.focus_handle(cx).is_focused(window));
        window.dispatch_action(Box::new(unknown), cx);
    });
    cx.run_until_parked();
    assert_eq!(recorded.borrow().len(), 1);
    assert_eq!(
        view.update(cx, |view, _| view.model().text().to_string()),
        "ab"
    );
}
