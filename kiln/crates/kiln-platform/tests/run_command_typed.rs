// SPDX-License-Identifier: LicenseRef-Proprietary

//! `commands::dispatch_typed`: a raw `RunCommand` naming a typed command,
//! as a test dispatches with `Window::dispatch_action`, reaches the typed
//! action through the view's `RunCommand` handler, with the invocation's
//! context and source; one naming anything else is left to the caller.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Context, FocusHandle, InteractiveElement as _, IntoElement, Render, TestAppContext,
    VisualTestContext, Window, div,
};
use kiln_command::{CommandId, ContextFrame, ContextStack, Invocation, Source};
use kiln_platform::{
    command_actions::current_invocation,
    commands::{self, RunCommand, dispatch_typed},
};

mod notes {
    kiln_platform::commands!(
        notes,
        [
            /// Rename Note
            Rename,
        ]
    );
}

/// What the view's handlers saw: each, with the invocation it ran under.
type Log = Rc<RefCell<Vec<(String, Option<Invocation>)>>>;

struct Notes {
    focus: FocusHandle,
    log: Log,
}

impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &notes::Rename, _, cx| {
                let current = current_invocation(cx);
                this.log.borrow_mut().push(("rename".into(), current));
            }))
            // A host's handler: the typed command if it is one, else its
            // own registry's.
            .on_action(cx.listener(|this, run: &RunCommand, window, cx| {
                if !dispatch_typed(run, window, cx) {
                    let what = format!("registry {}", run.0.command);
                    this.log.borrow_mut().push((what, None));
                }
            }))
    }
}

/// A focused Notes view, with its command registered.
fn notes_window(cx: &mut TestAppContext) -> (Log, &mut VisualTestContext) {
    cx.update(|cx| {
        commands::init(cx, "Notes");
        notes::register(cx);
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
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (log, cx)
}

fn stamped(command: &str) -> Invocation {
    let context: ContextStack = [ContextFrame::new("workspace"), ContextFrame::new("note")]
        .into_iter()
        .collect();
    Invocation::of(command)
        .unwrap()
        .source(Source::Palette)
        .context(context)
}

#[gpui::test]
fn a_raw_run_command_naming_a_typed_command_runs_its_view_method(cx: &mut TestAppContext) {
    let (log, cx) = notes_window(cx);
    let invocation = stamped("notes.rename");
    let run = RunCommand(invocation.clone());
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
    let log = log.borrow();
    assert_eq!(log.len(), 1, "{log:?}");
    let (what, current) = &log[0];
    assert_eq!(what, "rename");
    let current = current.as_ref().expect("an invocation while it runs");
    assert_eq!(current.command, invocation.command);
    assert_eq!(current.source, Source::Palette, "its source kept");
    assert_eq!(current.context, invocation.context, "its context kept");
}

#[gpui::test]
fn a_run_command_naming_no_typed_command_is_left_to_the_caller(cx: &mut TestAppContext) {
    let (log, cx) = notes_window(cx);
    cx.update(|window, cx| {
        let run = RunCommand(stamped("notes.archive"));
        window.dispatch_action(Box::new(run), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        log.borrow()
            .iter()
            .map(|(what, _)| what.clone())
            .collect::<Vec<_>>(),
        ["registry notes.archive"]
    );

    // Called directly: false, and nothing runs.
    log.borrow_mut().clear();
    let typed =
        cx.update(|window, cx| dispatch_typed(&RunCommand(stamped("notes.archive")), window, cx));
    cx.run_until_parked();
    assert!(!typed);
    assert!(log.borrow().is_empty());

    // And true for the typed one, which runs.
    let typed =
        cx.update(|window, cx| dispatch_typed(&RunCommand(stamped("notes.rename")), window, cx));
    cx.run_until_parked();
    assert!(typed);
    let ran: Vec<_> = log.borrow().iter().map(|(what, _)| what.clone()).collect();
    assert_eq!(ran, ["rename"]);
    let id = CommandId::parse("notes.rename").unwrap();
    assert_eq!(
        log.borrow()[0].1.as_ref().map(|run| &run.command),
        Some(&id)
    );
}
