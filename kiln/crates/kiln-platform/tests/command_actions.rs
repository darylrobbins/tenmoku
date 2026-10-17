// SPDX-License-Identifier: LicenseRef-Proprietary

//! Typed commands: GPUI actions with command metadata, dispatched from an
//! Invocation.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Action, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, TestAppContext, VisualTestContext, Window, div,
};
use kiln_command::{CommandId, DispatchError, Invocation, Source, Value};
use kiln_core::{CatalogError, CommandCatalog as _};
use kiln_platform::{
    action_dispatch::{dispatch_action, invocation_for},
    command_actions::{
        Command, CommandInfo, command_id, current_invocation, dispatch_invocation,
        register_command, typed_command,
    },
    commands::{self, RunCommand, observe_invocations},
    focus,
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
                unlisted: false,
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

#[gpui::test]
fn a_held_typed_action_dispatches_as_an_invocation(cx: &mut TestAppContext) {
    let (log, seen, cx) = notes_window(cx);
    cx.update(|window, cx| dispatch_action(&notes::NewNote, Source::Mouse, window, cx));
    cx.run_until_parked();
    let (what, current) = log.borrow().last().cloned().unwrap();
    assert_eq!(what, "new");
    assert_eq!(current.unwrap().source, Source::Mouse);
    assert_eq!(seen.borrow().len(), 1, "observers hear of it");
    assert_eq!(
        cx.update(|_, cx| invocation_for(&notes::NewNote, cx))
            .map(|invocation| invocation.command.to_string())
            .as_deref(),
        Some("notes.new_note")
    );
}

#[gpui::test]
fn a_held_action_with_args_dispatches_as_itself(cx: &mut TestAppContext) {
    let (log, seen, cx) = notes_window(cx);
    let open = OpenNote { index: 2 };
    assert!(cx.update(|_, cx| invocation_for(&open, cx)).is_none());
    cx.update(|window, cx| dispatch_action(&open, Source::Mouse, window, cx));
    cx.run_until_parked();
    let (what, current) = log.borrow().last().cloned().unwrap();
    assert_eq!(what, "open 2");
    assert_eq!(current, None, "no invocation describes it");
    assert_eq!(seen.borrow().len(), 0);
}

/// Which of a [`Page`]'s elements ran `NewNote`, in order.
type Ran = Rc<RefCell<Vec<&'static str>>>;

/// The invocations observers saw.
type Seen = Rc<RefCell<Vec<Invocation>>>;

/// A root that handles `NewNote`, holding a child that handles it too
/// while it's shown.
struct Page {
    root: FocusHandle,
    child: FocusHandle,
    shown: bool,
    log: Ran,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let child = self.shown.then(|| {
            let log = self.log.clone();
            div()
                .track_focus(&self.child)
                .on_action(move |_: &notes::NewNote, _, _| log.borrow_mut().push("child"))
        });
        let log = self.log.clone();
        div()
            .track_focus(&self.root)
            .on_action(move |_: &notes::NewNote, _, _| log.borrow_mut().push("root"))
            .children(child)
    }
}

/// A [`Page`] window with its child focused, `NewNote` registered, and
/// what observers see.
fn page_window(cx: &mut TestAppContext) -> (Entity<Page>, Ran, Seen, &mut VisualTestContext) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        notes::register(cx);
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let log = Rc::new(RefCell::new(Vec::new()));
    let (page, cx) = cx.add_window_view({
        let log = log.clone();
        move |_, cx| Page {
            root: cx.focus_handle(),
            child: cx.focus_handle(),
            shown: true,
            log,
        }
    });
    cx.update(|window, cx| {
        let child = page.read(cx).child.clone();
        window.focus(&child, cx);
    });
    cx.run_until_parked();
    (page, log, seen, cx)
}

/// Stops drawing the page's child, and draws.
fn hide_child(page: &Entity<Page>, cx: &mut VisualTestContext) {
    page.update(cx, |page, cx| {
        page.shown = false;
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn new_note() -> Invocation {
    invocation("notes.new_note", Value::Null)
}

#[gpui::test]
fn focus_on_an_element_no_longer_drawn_moves_to_its_ancestor(cx: &mut TestAppContext) {
    let (page, log, seen, cx) = page_window(cx);

    // Drawn: it runs on the child, before dispatch returns.
    let ran = cx.update(|window, cx| {
        dispatch_invocation(new_note(), window, cx).unwrap();
        log.borrow().clone()
    });
    assert_eq!(ran, ["child"], "synchronously, on the focused element");

    // Dispatch watched the window: once the child isn't drawn, the root,
    // its focusable ancestor, has focus, and runs the next one.
    hide_child(&page, cx);
    let ran = cx.update(|window, cx| {
        let root = page.read(cx).root.clone();
        assert!(root.is_focused(window), "focus moved to the root");
        dispatch_invocation(new_note(), window, cx).unwrap();
        log.borrow().clone()
    });
    assert_eq!(ran, ["child", "root"]);
    assert_eq!(seen.borrow().len(), 2, "observers hear of both");
}

#[gpui::test]
fn an_action_for_an_undrawn_element_goes_to_the_window(cx: &mut TestAppContext) {
    let (page, log, _, cx) = page_window(cx);
    let global = Rc::new(RefCell::new(0));
    cx.update(|_, cx| {
        let global = global.clone();
        cx.on_action(move |_: &notes::NewNote, _| *global.borrow_mut() += 1);
    });

    // Nothing watched the window yet, so focus stays on the hidden child.
    hide_child(&page, cx);
    cx.update(|window, cx| {
        let child = page.read(cx).child.clone();
        assert!(child.is_focused(window), "GPUI keeps the focus");
    });
    dispatch(new_note(), cx).unwrap();
    assert_eq!(*global.borrow(), 1, "the app's global handler runs it");
    assert!(log.borrow().is_empty(), "no element was reached");
}

#[gpui::test]
fn is_drawn_for_answers_as_the_probe_does(cx: &mut TestAppContext) {
    let (page, _, _, cx) = page_window(cx);
    // For an action with a listener on the child's path, and one without.
    let answers = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let child = page.read(cx).child.clone();
            (
                focus::is_drawn_for(&child, &notes::NewNote, window, cx),
                focus::is_drawn_for(&child, &notes::DeleteNote, window, cx),
                focus::is_drawn(&child, window, cx),
            )
        })
    };
    assert_eq!(answers(cx), (true, true, true), "drawn");
    hide_child(&page, cx);
    assert_eq!(answers(cx), (false, false, false), "no longer drawn");
}
