// SPDX-License-Identifier: LicenseRef-Proprietary

//! Typed commands: GPUI actions with command metadata, dispatched from an
//! Invocation.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Action, Context, FocusHandle, InteractiveElement as _, IntoElement, Render, TestAppContext,
    VisualTestContext, Window, div,
};
use kiln_command::{CommandId, DispatchError, Invocation, Source, Value};
use kiln_core::{CatalogError, CommandCatalog as _};
use kiln_platform::{
    command_actions::{
        Command, CommandInfo, command_id, current_invocation, dispatch_invocation,
        register_command, typed_command,
    },
    commands::{self, RunCommand, observe_invocations},
};
use schemars::JsonSchema;
use serde::Deserialize;

mod notes {
    kiln_platform::commands!(
        notes,
        [
            /// New Note
            NewNote,
            /// Delete Note
            ///
            /// Deletes the selected note.
            #[command(context = "notes_list")]
            DeleteNote,
        ]
    );
}

/// Opens the note at `index`: a command with args.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = notes)]
struct OpenNote {
    index: usize,
}

/// What the view's handlers saw: each command, with the invocation it ran
/// under.
type Log = Rc<RefCell<Vec<(String, Option<Invocation>)>>>;

struct Notes {
    focus: FocusHandle,
    log: Log,
}

impl Notes {
    fn record(&self, what: String, cx: &mut Context<Self>) {
        let current = current_invocation(cx);
        self.log.borrow_mut().push((what, current));
    }
}

impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &notes::NewNote, _, cx| this.record("new".into(), cx)))
            .on_action(cx.listener(|this, open: &OpenNote, _, cx| {
                this.record(format!("open {}", open.index), cx)
            }))
            .on_action(cx.listener(|this, run: &RunCommand, _, cx| {
                this.record(format!("run {}", run.0.command), cx)
            }))
    }
}

/// A focused Notes view, with its commands registered, and what observers
/// see.
fn notes_window(
    cx: &mut TestAppContext,
) -> (Log, Rc<RefCell<Vec<Invocation>>>, &mut VisualTestContext) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        commands::init(cx, "Notes");
        notes::register(cx);
        register_command::<OpenNote>(
            cx,
            CommandInfo {
                title: "Open Note".into(),
                context: None,
            },
        );
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let log = Log::default();
    let (view, cx) = cx.add_window_view({
        let log = log.clone();
        move |_, cx| Notes {
            focus: cx.focus_handle(),
            log,
        }
    });
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (log, seen, cx)
}

fn invocation(command: &str, args: Value) -> Invocation {
    Invocation::of(command)
        .unwrap()
        .args(args)
        .source(Source::Menu)
}

fn dispatch(invocation: Invocation, cx: &mut VisualTestContext) -> Result<(), DispatchError> {
    let result = cx.update(|window, cx| dispatch_invocation(invocation, window, cx));
    cx.run_until_parked();
    result
}

#[gpui::test]
fn the_macro_makes_gpui_actions(cx: &mut TestAppContext) {
    assert_eq!(notes::NewNote::name_for_type(), "notes::NewNote");
    assert_eq!(notes::DeleteNote::name_for_type(), "notes::DeleteNote");
    let (log, _, cx) = notes_window(cx);
    cx.update(|window, cx| window.dispatch_action(Box::new(notes::NewNote), cx));
    let handled: Vec<_> = log.borrow().iter().map(|(what, _)| what.clone()).collect();
    assert_eq!(handled, ["new"], "handled the GPUI way, through on_action");
}

#[gpui::test]
fn metadata_is_in_the_catalog(cx: &mut TestAppContext) {
    let (_, _, cx) = notes_window(cx);
    let id = |id: &str| CommandId::parse(id).unwrap();
    let catalog = cx.update(|_, cx| commands::catalog(cx));
    assert_eq!(catalog.check(&id("notes.new_note"), &Value::Null), Ok(()));
    assert_eq!(
        catalog.check(&id("app.quit"), &Value::Null),
        Ok(()),
        "beside the app's"
    );
    assert_eq!(
        catalog.check(&id("notes.unknown"), &Value::Null),
        Err(CatalogError::UnknownCommand)
    );
    let new = cx
        .update(|_, cx| typed_command(&id("notes.new_note"), cx))
        .unwrap();
    assert_eq!(new.title, "New Note");
    assert_eq!(new.context, None);
    let delete = cx
        .update(|_, cx| typed_command(&id("notes.delete_note"), cx))
        .unwrap();
    assert_eq!(delete.title, "Delete Note", "the first doc line");
    assert_eq!(delete.context.unwrap().to_string(), "notes_list");
}

#[gpui::test]
fn an_invocation_dispatches_the_typed_action(cx: &mut TestAppContext) {
    let (log, seen, cx) = notes_window(cx);
    dispatch(invocation("notes.new_note", Value::Null), cx).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    let (what, current) = &log[0];
    assert_eq!(what, "new");
    let current = current.as_ref().expect("the handler sees the invocation");
    assert_eq!(current.source, Source::Menu);
    assert_eq!(seen.borrow().len(), 1, "observers hear of it");
    assert_eq!(
        cx.update(|_, cx| current_invocation(cx)),
        None,
        "only while it runs"
    );
}

#[gpui::test]
fn args_become_action_fields(cx: &mut TestAppContext) {
    let (log, seen, cx) = notes_window(cx);
    let args = |json: serde_json::Value| -> Value { json };
    dispatch(
        invocation("notes.open_note", args(serde_json::json!({"index": 3}))),
        cx,
    )
    .unwrap();
    assert_eq!(log.borrow().last().unwrap().0, "open 3");

    let bad = dispatch(
        invocation("notes.open_note", args(serde_json::json!({"index": "x"}))),
        cx,
    );
    assert!(matches!(bad, Err(DispatchError::BadArgs { .. })), "{bad:?}");
    assert_eq!(log.borrow().len(), 1, "nothing ran");
    assert_eq!(seen.borrow().len(), 1);

    let catalog = cx.update(|_, cx| commands::catalog(cx));
    let open = CommandId::parse("notes.open_note").unwrap();
    assert!(matches!(
        catalog.check(&open, &args(serde_json::json!({"index": "x"}))),
        Err(CatalogError::BadArgs(_))
    ));
}

#[gpui::test]
fn unknown_ids_fall_back_to_run_command(cx: &mut TestAppContext) {
    let (log, _, cx) = notes_window(cx);
    dispatch(invocation("editor.move_left", Value::Null), cx).unwrap();
    let handled: Vec<_> = log.borrow().iter().map(|(what, _)| what.clone()).collect();
    assert_eq!(handled, ["run editor.move_left"]);
}

#[test]
fn ids_come_from_action_names() {
    let id = |name| command_id(name).map(|id| id.to_string());
    assert_eq!(id("notes::NewNote").as_deref(), Some("notes.new_note"));
    assert_eq!(
        id("a::b::HTTPServerStart").as_deref(),
        Some("a.b.http_server_start")
    );
    assert_eq!(id("notes::Undo2").as_deref(), Some("notes.undo2"));
    assert_eq!(id("NoNamespace"), None);
}

/// An action with args opts in to [`Command`] for its ID.
impl Command for OpenNote {}

#[test]
fn typed_commands_know_their_ids() {
    assert_eq!(notes::NewNote::id().as_str(), "notes.new_note");
    assert_eq!(notes::DeleteNote::id().as_str(), "notes.delete_note");
    assert_eq!(OpenNote::id().as_str(), "notes.open_note");
}

/// `commands!` names GPUI only through kiln, so it works where `gpui`
/// isn't the GPUI crate, as in an app that depends on kiln alone.
mod shadowed {
    #[allow(dead_code, reason = "shadows the gpui crate for the macro")]
    mod gpui {}

    kiln_platform::commands!(
        shadowed,
        [
            /// Shadowed
            ///
            /// Declared where `gpui` is something else.
            Shadowed,
        ]
    );
}

#[gpui::test]
fn the_macro_needs_no_gpui_of_its_own(cx: &mut TestAppContext) {
    assert_eq!(shadowed::Shadowed::name_for_type(), "shadowed::Shadowed");
    assert_eq!(
        <shadowed::Shadowed as Action>::documentation(),
        Some("Shadowed\n\nDeclared where `gpui` is something else."),
        "documented as GPUI's derive does"
    );
    assert_eq!(
        <notes::NewNote as Action>::documentation(),
        Some("New Note")
    );
    // GPUI's registry knows it, so it builds by name.
    let built = cx.update(|cx| cx.build_action("shadowed::Shadowed", None));
    assert!(built.unwrap().partial_eq(&shadowed::Shadowed));
}
