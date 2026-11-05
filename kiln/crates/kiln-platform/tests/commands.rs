// SPDX-License-Identifier: Apache-2.0

//! The app's commands: what reaches the app registry, what a host adds to
//! it, what observers see, and a view's propagation in its own context.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    App, Context, FocusHandle, InteractiveElement as _, IntoElement, Render, TestAppContext,
    VisualTestContext, Window, div,
};
use kiln_command::{
    Actor, CommandId, ContextFrame, ContextStack, Dispatched, Effect, Invocation, Source, Value,
};
use kiln_core::{CatalogError, CommandCatalog as _};
use kiln_platform::commands::{
    self, RunCommand, observe_invocations, propagate_with_context, register_app_commands,
};

/// A view with no command handlers.
struct Plain;

impl Render for Plain {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A view that propagates every command it's given, stamped with its own
/// context frame, as an editor does with an ID it doesn't know.
struct Propagating(FocusHandle);

impl Render for Propagating {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.0)
            .on_action(cx.listener(|_, run: &RunCommand, _, cx| {
                let stamped = run.0.clone().context(notes_context());
                propagate_with_context(stamped, cx);
            }))
    }
}

fn notes_context() -> ContextStack {
    [ContextFrame::new("notes")].into_iter().collect()
}

/// Notes' app, with a `notes.ping` app command counting its runs, and every
/// invocation the observers see.
fn notes_app(cx: &mut TestAppContext) -> (Rc<RefCell<u32>>, Rc<RefCell<Vec<Invocation>>>) {
    let pings = Rc::new(RefCell::new(0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        commands::init(cx, "Notes");
        let counter = pings.clone();
        register_app_commands(cx, move |registry| {
            registry
                .register("notes.ping", "Ping", move |_: &mut &mut App, _: &Actor| {
                    *counter.borrow_mut() += 1;
                    Ok(Dispatched::new(Effect::Nothing))
                })
                .unwrap();
        });
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    (pings, seen)
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap().source(Source::Menu);
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn unhandled_run_command_reaches_the_app_registry(cx: &mut TestAppContext) {
    let (pings, seen) = notes_app(cx);
    let (_, cx) = cx.add_window_view(|_, _| Plain);
    run("notes.ping", cx);
    assert_eq!(*pings.borrow(), 1, "no view handled it, so the app did");
    run("notes.unknown", cx);
    assert_eq!(*pings.borrow(), 1);
    let ran: Vec<_> = seen
        .borrow()
        .iter()
        .map(|i| i.command.to_string())
        .collect();
    assert_eq!(ran, ["notes.ping"], "an unknown command runs nothing");
}

#[gpui::test]
fn hosts_add_app_commands(cx: &mut TestAppContext) {
    notes_app(cx);
    let catalog = cx.read(commands::catalog);
    for known in [
        "app.quit",
        "app.hide",
        "window.close",
        "window.zoom",
        "notes.ping",
    ] {
        let id = CommandId::parse(known).unwrap();
        assert_eq!(catalog.check(&id, &Value::Null), Ok(()), "{known}");
    }
    let unknown = CommandId::parse("notes.unknown").unwrap();
    assert_eq!(
        catalog.check(&unknown, &Value::Null),
        Err(CatalogError::UnknownCommand)
    );
}

#[gpui::test]
fn observers_see_app_commands(cx: &mut TestAppContext) {
    let (_, seen) = notes_app(cx);
    let (_, cx) = cx.add_window_view(|_, _| Plain);
    run("notes.ping", cx);
    run("notes.ping", cx);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|i| i.source == Source::Menu));
}

#[test]
fn hide_names_the_app() {
    let app = commands::app_commands("Notes");
    let hide = app.get(&CommandId::parse("app.hide").unwrap()).unwrap();
    assert_eq!(hide.title, "Hide Notes");
    let ids: Vec<_> = app.commands().map(|meta| meta.id.to_string()).collect();
    assert_eq!(
        ids,
        [
            "app.quit",
            "app.hide",
            "app.hide_others",
            "app.show_all",
            "window.minimize",
            "window.zoom",
            "window.toggle_fullscreen",
            "window.close",
        ]
    );
}

#[gpui::test]
fn a_view_propagates_with_its_context(cx: &mut TestAppContext) {
    let (pings, seen) = notes_app(cx);
    let (view, cx) = cx.add_window_view(|_, cx| Propagating(cx.focus_handle()));
    cx.update(|window, cx| {
        let focus = view.read(cx).0.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    // Dispatched from the view's element, in no context.
    let invocation = Invocation::of("notes.ping").unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
    assert_eq!(*pings.borrow(), 1);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].context,
        notes_context(),
        "the app ran the view's stamped copy, not GPUI's original"
    );
}

/// A window command acts on the window it was dispatched in, though
/// another is the active one (or none is: an X server without a window
/// manager has none).
#[gpui::test]
fn window_commands_act_on_the_window_they_ran_in(cx: &mut TestAppContext) {
    notes_app(cx);
    let first = cx.add_window(|_, _| Plain);
    let second = cx.add_window(|_, _| Plain);
    first
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    let close = Invocation::of("window.close").unwrap();
    second
        .update(cx, |_, window, cx| {
            kiln_platform::command_actions::dispatch_invocation(close, window, cx).unwrap()
        })
        .unwrap();
    cx.run_until_parked();
    let open = cx.update(|cx| cx.windows());
    assert_eq!(open, [first.into()]);
}
