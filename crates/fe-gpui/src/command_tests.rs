//! Key bindings dispatch through the command registries: one keymap file
//! binds them all, keystrokes become recorded invocations, and the recordings
//! replay headlessly.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::Buffer;
use fe_command::{ContextStack, Invocation, Source, Value};
use fe_editor::{
    EditorModel, Movement,
    commands::{EditorTarget, HeadlessHost},
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    commands::{RunCommand, observe_invocations},
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
    // The table is now `fe_keymap::DEFAULT_MACOS`; `keymap_tests` checks
    // its rows. GPUI's own keymap stays empty.
    let keymap = default_keymap();
    let bindings = keymap.bindings();
    // 52 for the editor and the app, 38 for fe-workbench.
    assert_eq!(bindings.len(), 90);
    let editor = fe_editor::commands::context();
    let workbench = |binding: &fe_keymap::Binding| {
        let context = binding.context.as_ref().map(ToString::to_string);
        matches!(
            context.as_deref(),
            Some(
                "workbench"
                    | "picker"
                    | "text_input"
                    | "search_panel"
                    | "search_panel && text_input"
                    | "project_panel"
            )
        )
    };
    let others: Vec<_> = bindings
        .iter()
        .filter(|binding| binding.depth(&editor) != Some(1) && !workbench(binding))
        .map(|binding| {
            (
                binding.keys.to_string(),
                binding.command.as_str(),
                binding.context.is_none(),
            )
        })
        .collect();
    let context_free = [
        ("cmd-q", "app.quit"),
        ("cmd-h", "app.hide"),
        ("alt-cmd-h", "app.hide_others"),
        ("cmd-m", "window.minimize"),
        ("ctrl-cmd-f", "window.toggle_fullscreen"),
        ("cmd-shift-w", "window.close"),
    ]
    .map(|(keys, command)| {
        let keys = fe_keymap::KeySequence::parse(keys).unwrap().to_string();
        (keys, command, true)
    });
    assert_eq!(others, context_free);
    for binding in bindings {
        assert_eq!(binding.invocation(&editor).source, Source::Keyboard);
    }

    cx.update(init);
    let gpui_bindings = cx.update(|cx| cx.key_bindings().borrow().bindings().count());
    assert_eq!(gpui_bindings, 0);
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
    let quit = Invocation::of("app.quit")
        .unwrap()
        .source(Source::Keyboard)
        .context(fe_editor::commands::context());
    assert_eq!(*recorded.borrow(), std::slice::from_ref(&quit));

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

    // Unfocused, the app gets it straight from the keymap, in no context.
    cx.update(|window, cx| window.blur(cx));
    cx.simulate_keystrokes("cmd-q");
    assert_eq!(
        *recorded.borrow(),
        [quit.clone(), quit.context(ContextStack::new())]
    );
}
