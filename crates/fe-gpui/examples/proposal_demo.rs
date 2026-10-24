//! A stand-in AI host for checking proposed changes by hand: the diff
//! blocks, their Accept and Reject buttons, cmd-z on an accepted hunk and
//! the `proposal.*` keyboard commands.
//!
//! ```text
//! cargo run -p fe-gpui --example proposal_demo -- <file> <proposed-file>
//! ```
//!
//! Opens `<file>` in a window with `<proposed-file>`'s text proposed in
//! place of all of it. Printed to stdout:
//!
//! - the proposal's dry run at start and after every buffer change, a line
//!   per hunk (typing inside a hunk shows it going stale, cmd-z after an
//!   accept shows the text coming back);
//! - each [`ProposalEvent`], with the transaction id of an accept;
//! - each `proposal.*`, `editor.undo` and `editor.redo` invocation, so a
//!   keyboard command and a button click are told apart (a click is an
//!   event with no invocation before it);
//! - `all hunks resolved` once none is pending.
//!
//! Closing the window quits.

use std::{cell::RefCell, process::ExitCode, rc::Rc};

use fe_buffer::{BufferEvent, Origin, Outcome, ProposedChange};
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView, ProposalEvent};
use gpui::{App, AppContext as _, Focusable as _, WindowOptions};

const USAGE: &str = "usage: proposal_demo <file> <proposed-file>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file, proposed] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let (mut model, proposed) = match (EditorModel::open(file), std::fs::read_to_string(proposed)) {
        (Ok(model), Ok(proposed)) => (model, proposed),
        (model, proposed) => {
            if let Err(error) = model {
                eprintln!("{file}: {error}");
            }
            if let Err(error) = proposed {
                eprintln!("{}: {error}", args[1]);
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let language = fe_syntax::language_name_for_path(std::path::Path::new(file));
    let snapshot = model.snapshot().clone();
    let proposal = Rc::new(RefCell::new(ProposedChange::from_text(
        &snapshot,
        0..snapshot.len(),
        &proposed,
        Origin::Ai {
            agent: "demo".into(),
            session: "demo".into(),
        },
    )));

    gpui_platform::application().run(move |cx: &mut App| {
        fe_gpui::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        fe_gpui::commands::observe_invocations(cx, |invocation| {
            let command = invocation.command.as_str();
            if command.starts_with("proposal.") || matches!(command, "editor.undo" | "editor.redo")
            {
                println!("invoked: {command} {}", invocation.args);
            }
        });
        let edits = model.buffer_mut().subscribe();
        let window = cx
            .open_window(WindowOptions::default(), move |window, cx| {
                let view = cx.new(|cx| {
                    model.set_soft_wrap(false);
                    let mut theme = EditorTheme::default_dark(cx);
                    theme.font_family = "Menlo".into();
                    EditorView::new(model, theme, cx)
                });
                window.focus(&view.focus_handle(cx), cx);
                view
            })
            .expect("the window opens");
        let view = window.root(cx).expect("the window is open");
        view.update(cx, |view, cx| {
            view.set_language(language, cx);
            view.set_proposals(vec![proposal.clone()], cx);
        });
        cx.subscribe(&view, |_, event: &ProposalEvent, _| match event {
            ProposalEvent::Accepted { hunk, transaction } => {
                println!("event: accepted {hunk:?} as {transaction:?}");
            }
            ProposalEvent::AcceptedNoChange { hunk } => {
                println!("event: accepted {hunk:?}, nothing to change");
            }
            ProposalEvent::Rejected { hunk } => println!("event: rejected {hunk:?}"),
        })
        .detach();
        let first = proposal.clone();
        // Every change to the view, an edit or a hunk resolved, notifies it.
        let mut announced = false;
        cx.observe(&view, move |view, cx| {
            let view = view.read(cx);
            let proposal = proposal.borrow();
            for event in edits.drain() {
                if let BufferEvent::Edited {
                    transaction,
                    kind,
                    to,
                    ..
                } = event
                {
                    println!("edit: {transaction:?} {kind:?} to {to:?}");
                    // A snapshot older than the buffer's can't be had, so an
                    // edit another one followed within the notification
                    // has no dry run of its own.
                    if to == view.model().snapshot().version() {
                        print_dry_run(&proposal, view);
                    } else {
                        println!("dry run: superseded by a later edit");
                    }
                }
            }
            let resolved = proposal.is_resolved();
            if resolved && !announced {
                println!("all hunks resolved");
            }
            announced = resolved;
        })
        .detach();
        print_dry_run(&first.borrow(), view.read(cx));
        cx.activate(true);
    });
    ExitCode::SUCCESS
}

/// One line per hunk: index, outcome, reason.
fn print_dry_run(proposal: &ProposedChange, view: &EditorView) {
    let dry_run = proposal.dry_run(view.model().snapshot());
    println!("dry run:");
    for (index, hunk) in dry_run.hunks().iter().enumerate() {
        let (outcome, reason) = match &hunk.outcome {
            Outcome::Clean => ("clean", String::new()),
            Outcome::Conflict(conflict) => ("conflict", conflict.to_string()),
            Outcome::Resolved(status) => ("resolved", format!("{status:?}")),
        };
        println!("  {index}: {outcome} {reason}");
    }
}
