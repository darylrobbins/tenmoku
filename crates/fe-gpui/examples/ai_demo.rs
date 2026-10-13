//! A demo host that drives all of fastedit's AI seams together in a real
//! window, against a scripted fake AI with no network and no model: the
//! reference for how an embedding host uses the AI APIs (docs/ai-host.md).
//!
//! ```text
//! cargo run -p fe-gpui --example ai_demo
//! cargo run -p fe-gpui --example ai_demo -- <file> <script>
//! cargo run -p fe-gpui --example ai_demo -- <file> -
//! ```
//!
//! With no arguments it plays [`scripted_ai::DEMO_SCRIPT`] against an
//! untitled buffer holding [`scripted_ai::DEMO_TEXT`]. With two it opens
//! `<file>` and plays `<script>`, or, for `-`, the steps typed on stdin, each
//! line run as it arrives, a bad one reported and skipped. The script
//! grammar is [`fe_editor::scripted_ai`]'s. A bad file or script exits with
//! status 2 before the window opens. On Linux, add
//! `--features gpui_platform/x11,gpui_platform/wayland` to the `cargo run`,
//! for the window backends only the app turns on.
//!
//! What each seam shows, and the host API it uses:
//!
//! - **Ghost text**: `suggest` shows a dimmed suggestion at the caret, which
//!   typing consumes; `accept`, `accept-word`, `accept-line` and `dismiss`
//!   run its `ghost_text.*` commands in the window, as `tab` does by hand.
//!   [`GhostText::new`](fe_editor::ghost_text::GhostText::new) (in
//!   [`ScriptedAi::step`]), [`EditorView::set_ghost_text`] and
//!   [`GhostTextEvent`]. A suggestion the caret left and came back to is
//!   shown again.
//! - **Streamed edit**: `stream`, `push` and `finish` write into the buffer
//!   while `caret` and `type` edit around it, all one AI-origin undo group
//!   that `undo-ai` takes back whole and `undo` never touches.
//!   [`EditStream`](fe_buffer::EditStream) on
//!   `view.model_mut().buffer_mut()`, and the buffer's undo of
//!   [`Origin::is_ai`](fe_buffer::Origin::is_ai).
//! - **Proposal**: `propose` shows diff blocks with Accept and Reject
//!   buttons; `accept-hunk` and `reject-hunk` resolve one through the view,
//!   as a button does. An accept is the user's edit, which `undo` (or cmd-z)
//!   takes back. [`ProposedChange::from_text`](fe_buffer::ProposedChange::from_text),
//!   [`EditorView::set_proposals`], [`EditorView::accept_hunk`],
//!   [`EditorView::reject_hunk`] and [`ProposalEvent`].
//! - **Touched rows**: the rows the AI wrote, or the user took from it, get
//!   an accent background, hunks accepted by hand included.
//!   [`EditorModel::add_decoration_provider`] with a
//!   [`DecorationSet`](fe_editor::decorations::DecorationSet) (in
//!   [`ScriptedAi::new`]), and [`ScriptedAi::note_accepted`] on
//!   [`ProposalEvent::Accepted`].
//!
//! Printed to stdout: each step's line and what it did or why it couldn't
//! (an error never stops the script), each [`GhostTextEvent`] and
//! [`ProposalEvent`], and `script done`. The window then stays open for
//! hand play until it's closed or, unless the script is stdin, `q` is
//! typed on stdin.

use std::{
    cell::RefCell,
    io::BufRead as _,
    process::ExitCode,
    rc::Rc,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use fe_buffer::{Buffer, HunkId};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::{
    EditorModel,
    scripted_ai::{self, Report, ScriptError, ScriptedAi, Step, StepError},
};
use fe_gpui::{EditorTheme, EditorView, GhostTextEvent, ProposalEvent, commands::RunCommand};
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Focusable as _, WindowHandle, WindowOptions,
};

const USAGE: &str = "usage: ai_demo [<file> <script>]  (<script> `-` reads steps from stdin)";
/// Between steps, so each can be watched landing.
const PACE: Duration = Duration::from_millis(250);
const POLL_EVERY: Duration = Duration::from_millis(20);

/// The key context the suggestion's commands run in while it shows.
const GHOST_TEXT_CONTEXT: &str = "ghost_text";

/// One step of a script, with where it came from.
struct Line {
    /// 1-based.
    number: usize,
    text: String,
    step: Step,
}

/// Where the steps come from.
enum Script {
    Lines(Vec<Line>),
    /// Read from stdin as they arrive.
    Stdin,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (model, language, script) = match load(&args) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (sender, stdin) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });

    gpui_platform::application().run(move |cx: &mut App| {
        fe_gpui::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let window = cx
            .open_window(WindowOptions::default(), move |window, cx| {
                let view = cx.new(|cx| {
                    let mut model = model;
                    model.set_soft_wrap(false);
                    EditorView::new(model, EditorTheme::default_dark(cx), cx)
                });
                window.focus(&view.focus_handle(cx), cx);
                view
            })
            .expect("the window opens");
        let view = window.root(cx).expect("the window is open");
        // Its touched rows paint in the window: they're one of the view's
        // model's decoration providers.
        let driver = view.update(cx, |view, cx| {
            if language.is_some() {
                view.set_language(language, cx);
            }
            Rc::new(RefCell::new(ScriptedAi::new(view.model_mut())))
        });
        let ai = driver.clone();
        cx.subscribe(&view, move |view, event: &GhostTextEvent, cx| match event {
            GhostTextEvent::Accepted {
                transaction,
                text,
                whole,
            } => {
                let part = if *whole { "" } else { " in part" };
                println!("      event: suggestion accepted{part}: {text:?} as {transaction:?}");
            }
            GhostTextEvent::Dismissed => {
                println!("      event: suggestion dismissed");
                // Still live at the caret, it was turned down (`escape`,
                // `dismiss`, leaving insert mode), so it isn't offered
                // again; one the caret left is, when the caret is back.
                view.update(cx, |view, _| {
                    let mut ai = ai.borrow_mut();
                    if (ai.ghost_text()).is_some_and(|ghost| ghost.is_live(view.model())) {
                        ai.step(view.model_mut(), &Step::Dismiss).ok();
                    }
                });
            }
        })
        .detach();
        let ai = driver.clone();
        cx.subscribe(&view, move |view, event: &ProposalEvent, cx| match event {
            ProposalEvent::Accepted { hunk, transaction } => {
                println!("      event: hunk {hunk:?} accepted as {transaction:?}");
                // By button, key or step alike, its rows are touched.
                view.update(cx, |view, cx| {
                    ai.borrow_mut().note_accepted(view.model(), *hunk);
                    cx.notify();
                });
            }
            ProposalEvent::Rejected { hunk } => println!("      event: hunk {hunk:?} rejected"),
        })
        .detach();
        cx.spawn(async move |cx| play(window, driver, script, stdin, cx).await)
            .detach();
        cx.activate(true);
    });
    ExitCode::SUCCESS
}

/// The model, its language if the view can't tell from a path, and the
/// script, from the command line.
fn load(args: &[String]) -> Result<(EditorModel, Option<&'static str>, Script), String> {
    match args {
        [] => {
            let model = EditorModel::new(Buffer::new(scripted_ai::DEMO_TEXT), None);
            let lines = lines(scripted_ai::DEMO_SCRIPT).expect("the demo script parses");
            Ok((model, Some("rust"), Script::Lines(lines)))
        }
        [file, script] => {
            let model = EditorModel::open(file).map_err(|error| format!("{file}: {error}"))?;
            if script == "-" {
                return Ok((model, None, Script::Stdin));
            }
            let text =
                std::fs::read_to_string(script).map_err(|error| format!("{script}: {error}"))?;
            let lines = lines(&text).map_err(|error| format!("{script}: {error}"))?;
            Ok((model, None, Script::Lines(lines)))
        }
        _ => Err(format!(
            "ai_demo takes no arguments or two, got {}",
            args.len()
        )),
    }
}

/// Every step of `script`, or its first line that doesn't parse.
fn lines(script: &str) -> Result<Vec<Line>, ScriptError> {
    (script.lines().enumerate())
        .filter_map(|(ix, text)| parse_line(ix + 1, text).transpose())
        .collect()
}

/// Line `number` of a script, `None` if it's blank or a comment.
fn parse_line(number: usize, text: &str) -> Result<Option<Line>, ScriptError> {
    let steps = scripted_ai::parse(text).map_err(|error| ScriptError {
        line: number,
        ..error
    })?;
    Ok(steps.into_iter().next().map(|step| Line {
        number,
        text: text.to_owned(),
        step,
    }))
}

/// Plays `script` in `window`, then waits for `q` on stdin.
async fn play(
    window: WindowHandle<EditorView>,
    driver: Rc<RefCell<ScriptedAi>>,
    script: Script,
    stdin: Receiver<String>,
    cx: &mut AsyncApp,
) {
    match script {
        Script::Lines(lines) => {
            for line in &lines {
                if quit_asked(&stdin) {
                    cx.update(|cx| cx.quit());
                    return;
                }
                if run(window, &driver, line, cx).await.is_err() {
                    return;
                }
            }
            println!("script done");
            loop {
                match stdin.try_recv() {
                    Ok(line) if line.trim() == "q" => {
                        cx.update(|cx| cx.quit());
                        return;
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => cx.background_executor().timer(POLL_EVERY).await,
                    // Without stdin, closing the window quits.
                    Err(TryRecvError::Disconnected) => return,
                }
            }
        }
        Script::Stdin => {
            let mut number = 0;
            loop {
                match stdin.try_recv() {
                    Ok(text) => {
                        number += 1;
                        match parse_line(number, &text) {
                            Ok(Some(line)) => {
                                if run(window, &driver, &line, cx).await.is_err() {
                                    return;
                                }
                            }
                            Ok(None) => {}
                            Err(error) => println!("{error}"),
                        }
                    }
                    Err(TryRecvError::Empty) => cx.background_executor().timer(POLL_EVERY).await,
                    Err(TryRecvError::Disconnected) => {
                        println!("script done");
                        return;
                    }
                }
            }
        }
    }
}

/// Whether `q` came on stdin, taking whatever else came.
fn quit_asked(stdin: &Receiver<String>) -> bool {
    stdin.try_iter().any(|line| line.trim() == "q")
}

/// Runs `line`'s step in `window` and prints what it did. `Err` once the
/// window is gone.
async fn run(
    window: WindowHandle<EditorView>,
    driver: &Rc<RefCell<ScriptedAi>>,
    line: &Line,
    cx: &mut AsyncApp,
) -> gpui::Result<()> {
    println!("{:>4}  {}", line.number, line.text);
    match &line.step {
        // What the user does with a suggestion, they do in the window.
        step @ (Step::Accept | Step::AcceptWord | Step::AcceptLine | Step::Dismiss) => {
            let shown = window.update(cx, |view, _, _| view.ghost_text().is_some())?;
            if shown {
                let command = ghost_text_command(step);
                AnyWindowHandle::from(window).update(cx, |_, window, cx| {
                    window.dispatch_action(Box::new(command), cx)
                })?;
            } else {
                print(Err(StepError::NoSuggestion));
            }
        }
        // And a hunk they accept or reject, through the view, as a button.
        Step::AcceptHunk(n) | Step::RejectHunk(n) => {
            let accept = matches!(line.step, Step::AcceptHunk(_));
            window.update(cx, |view, _, cx| {
                let result = latest_hunk(&driver.borrow(), view, *n).and_then(|id| {
                    let resolved = if accept {
                        view.accept_hunk(id, cx).map(Report::HunkAccepted)
                    } else {
                        view.reject_hunk(id, cx).map(|()| Report::HunkRejected)
                    };
                    resolved.map_err(StepError::Proposal)
                });
                print(result);
            })?;
        }
        step => {
            window.update(cx, |view, _, cx| {
                let mut ai = driver.borrow_mut();
                print(ai.step(view.model_mut(), step));
                match step {
                    Step::Suggest(_) => view.set_ghost_text(ai.ghost_text().cloned(), cx),
                    Step::Propose { .. } => view.set_proposals(ai.proposals().to_vec(), cx),
                    // The caret back where a suggestion was left shows it
                    // again.
                    _ if view.ghost_text().is_none() => {
                        let ghost = ai.ghost_text().filter(|ghost| ghost.is_live(view.model()));
                        if let Some(ghost) = ghost.cloned() {
                            view.set_ghost_text(Some(ghost), cx);
                            println!("      suggestion shown again");
                        }
                    }
                    _ => {}
                }
                cx.notify();
            })?;
            if let Step::Wait(pause) = step {
                cx.background_executor().timer(*pause).await;
            }
        }
    }
    cx.background_executor().timer(PACE).await;
    Ok(())
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

/// The latest proposal's `n`th (1-based) hunk, as the script counts them.
fn latest_hunk(driver: &ScriptedAi, view: &mut EditorView, n: usize) -> Result<HunkId, StepError> {
    let proposal = driver.proposals().last().ok_or(StepError::NoProposal)?;
    // Caught up with a stream's edits, which go to the buffer.
    view.model_mut().sync_display();
    let hunks = proposal.borrow().hunks(view.model().snapshot());
    (n.checked_sub(1).and_then(|ix| hunks.get(ix)))
        .map(|hunk| hunk.id)
        .ok_or(StepError::NoSuchHunk(n))
}

fn print(result: Result<Report, StepError>) {
    match result {
        Ok(report) => println!("      {report}"),
        Err(error) => println!("      error: {error}"),
    }
}
