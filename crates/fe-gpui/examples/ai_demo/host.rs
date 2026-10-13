//! `ai_demo`'s glue between its window's [`EditorView`] and the scripted
//! fake AI, apart from its printing so it runs headless too
//! (`tests/ai_demo_host.rs` includes it as `ai_demo.rs` does).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::scripted_ai::{Report, ScriptedAi, Step, StepError};
use fe_gpui::{EditorView, GhostTextEvent, ProposalEvent, commands::RunCommand};
use gpui::{AnyWindowHandle, App, AsyncApp, Context, Entity, WindowHandle};

/// The key context the suggestion's commands run in while it shows.
const GHOST_TEXT_CONTEXT: &str = "ghost_text";

/// A view wired to its driver by [`install`].
pub(crate) struct Host {
    /// What the last suggestion command did, from its [`GhostTextEvent`].
    outcome: Rc<Cell<Option<Report>>>,
}

/// Wires `view` to `ai`, the driver whose touched rows it paints, for the
/// view's lifetime:
///
/// - A suggestion accepted in full, by a step or by hand, is cleared from
///   the driver, so an undo that takes its text back can't offer it again.
///   One accepted in part stays.
/// - One dismissed while live at the caret is cleared too; one the caret
///   left stays, to be shown when the caret is back ([`show_again`]).
/// - A hunk accepted, by a step, button or key, is marked touched
///   ([`ScriptedAi::note_accepted`]).
/// - Every change of the buffer's version services the driver, so it
///   follows undos and redos of what it marked as they're made, by hand or
///   by the host, not only at its next step.
pub(crate) fn install(
    view: &Entity<EditorView>,
    ai: &Rc<RefCell<ScriptedAi>>,
    cx: &mut App,
) -> Host {
    let outcome = Rc::new(Cell::new(None));
    let (driver, told) = (ai.clone(), outcome.clone());
    cx.subscribe(view, move |view, event: &GhostTextEvent, cx| match event {
        GhostTextEvent::Accepted {
            transaction, whole, ..
        } => {
            told.set(Some(Report::Accepted(*transaction)));
            if *whole {
                // Takes the suggestion, leaving the text be.
                view.update(cx, |view, _| {
                    (driver.borrow_mut())
                        .step(view.model_mut(), &Step::Dismiss)
                        .ok();
                });
            }
        }
        GhostTextEvent::Dismissed => {
            told.set(Some(Report::Dismissed));
            // Still live at the caret, it was turned down (`escape`,
            // `dismiss`, leaving insert mode), so it isn't offered again;
            // one the caret left is, when the caret is back.
            view.update(cx, |view, _| {
                let mut ai = driver.borrow_mut();
                if (ai.ghost_text()).is_some_and(|ghost| ghost.is_live(view.model())) {
                    ai.step(view.model_mut(), &Step::Dismiss).ok();
                }
            });
        }
    })
    .detach();
    let driver = ai.clone();
    cx.subscribe(view, move |view, event: &ProposalEvent, cx| {
        if let ProposalEvent::Accepted { hunk, .. } = event {
            // By button, key or step alike, its rows are touched.
            view.update(cx, |view, cx| {
                driver.borrow_mut().note_accepted(view.model(), *hunk);
                cx.notify();
            });
        }
    })
    .detach();
    let driver = ai.clone();
    let serviced = Rc::new(Cell::new(view.read(cx).model().buffer().version()));
    cx.observe(view, move |view, cx| {
        let (driver, serviced) = (driver.clone(), serviced.clone());
        // Weak, as the app may quit with it pending.
        let view = view.downgrade();
        // After the events emitted with this notify: a hunk's accept is
        // marked from the buffer's last edit, which servicing first would
        // take.
        cx.defer(move |cx| {
            let serviced = view.update(cx, |view, cx| {
                let version = view.model().buffer().version();
                // The guard against servicing forever: the notify below
                // comes back here once, unchanged.
                if serviced.replace(version) == version {
                    return;
                }
                let model = view.model_mut();
                driver
                    .borrow_mut()
                    .step(model, &Step::Wait(Duration::ZERO))
                    .ok();
                cx.notify();
            });
            // Gone, there's nothing to repaint.
            serviced.ok();
        });
    })
    .detach();
    Host { outcome }
}

/// Runs `step`, one of `accept`, `accept-word`, `accept-line` and
/// `dismiss`, in `window` as its `ghost_text.*` command, as a key would,
/// and returns what it did: [`StepError::NoSuggestion`] if it did nothing,
/// as with no suggestion shown or one no longer live. `Err` once the window
/// is gone.
pub(crate) fn run_ghost_step(
    window: WindowHandle<EditorView>,
    host: &Host,
    step: &Step,
    cx: &mut AsyncApp,
) -> gpui::Result<Result<Report, StepError>> {
    let command = ghost_text_command(step);
    host.outcome.take();
    // Dispatched once this update is done, its events with it.
    AnyWindowHandle::from(window).update(cx, |_, window, cx| {
        window.dispatch_action(Box::new(command), cx);
    })?;
    // An accept of a suggestion gone stale only dismisses it.
    Ok(match (step, host.outcome.take()) {
        (Step::Dismiss, Some(report @ Report::Dismissed))
        | (
            Step::Accept | Step::AcceptWord | Step::AcceptLine,
            Some(report @ Report::Accepted(_)),
        ) => Ok(report),
        _ => Err(StepError::NoSuggestion),
    })
}

/// Shows `ai`'s suggestion again if the view shows none and the caret is
/// back where it's live. Returns whether it did.
pub(crate) fn show_again(
    view: &mut EditorView,
    ai: &ScriptedAi,
    cx: &mut Context<EditorView>,
) -> bool {
    if view.ghost_text().is_some() {
        return false;
    }
    let Some(ghost) = (ai.ghost_text()).filter(|ghost| ghost.is_live(view.model())) else {
        return false;
    };
    view.set_ghost_text(Some(ghost.clone()), cx);
    true
}

/// The `ghost_text.*` command for `step`, in the context a key would run it
/// in while the suggestion shows.
fn ghost_text_command(step: &Step) -> RunCommand {
    let command = match step {
        Step::Accept => "ghost_text.accept",
        Step::AcceptWord => "ghost_text.accept_word",
        Step::AcceptLine => "ghost_text.accept_line",
        _ => "ghost_text.dismiss",
    };
    let frame = ContextFrame::new(fe_gpui::KEY_CONTEXT).flag(GHOST_TEXT_CONTEXT);
    let invocation = Invocation::of(command).expect("a valid command ID");
    RunCommand(invocation.context(ContextStack::new().with(frame)))
}
