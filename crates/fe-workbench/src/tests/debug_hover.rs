//! Card D11's hover evaluation (`debug_hover.rs`) against fe-dap's fake
//! adapter in a headless workbench ([`Debug`]): no `evaluate` without the
//! adapter's `supportsEvaluateForHovers`, no value from before a step shown
//! after it, the value with up to 20 children read through the popover's text
//! hook, and the commands `debug.hover` and `debug.evaluate`.

use std::{cell::RefCell, rc::Rc};

use fe_dap::{
    SessionState,
    fake::{Local, Script},
};
use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext};
use kiln_l10n::tr;

use super::debug_commands::{Debug, main_path, requests, script, trusted};
use crate::debug_hover::{self, CHILD_CAP, Probe};

/// The fake's script, its adapter able to evaluate for hovers.
fn hovering(root: &std::path::Path) -> Script {
    let mut script = script(root);
    script.capabilities.supports_evaluate_for_hovers = Some(true);
    script
}

/// [`hovering`], `main` also holding `big`, with 25 children.
fn with_big(root: &std::path::Path) -> Script {
    let mut script = hovering(root);
    let children = (0..25)
        .map(|n| Local::new(&format!("c{n:02}"), &n.to_string(), "i32"))
        .collect();
    script.program.functions[0].locals.push(Local {
        children,
        ..Local::new("point", "Big", "Big")
    });
    // `point`'s value is the last of the name's: replace the first.
    script.program.functions[0].locals.remove(1);
    script
}

/// A session stopped on row 1 (`let point = 2;`) of `src/main.rs`, the file
/// open with its syntax tree parsed.
fn stopped<'a>(
    cx: &'a mut TestAppContext,
    script: impl FnOnce(&std::path::Path) -> Script,
) -> (Debug<'a>, fe_dap::fake::FakeAdapter, Entity<EditorView>) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(1);
    let editor = debug.open_at(&main, 1);
    debug.until("the syntax tree", |debug| {
        editor.read_with(debug.cx, |view, _| {
            view.syntax().is_some_and(|syntax| syntax.tree().is_some())
        })
    });
    (debug, fake, editor)
}

/// Where `needle` is in the editor's text, plus `into`.
fn offset_of(debug: &mut Debug, editor: &Entity<EditorView>, needle: &str, into: usize) -> usize {
    editor.read_with(debug.cx, |view, _| {
        view.model().text().to_string().find(needle).unwrap() + into
    })
}

fn buffer_of(debug: &mut Debug, editor: &Entity<EditorView>) -> fe_buffer::BufferId {
    editor.read_with(debug.cx, |view, _| view.model().snapshot().id())
}

/// Shows the popover's text for the caret at `offset`, by the keys.
fn popover(debug: &mut Debug, editor: &Entity<EditorView>) -> Option<String> {
    editor.read_with(debug.cx, |view, _| view.popover_text())
}

/// C1: without `supportsEvaluateForHovers` nothing is sent, however the
/// value is asked for, and the command says why.
#[gpui::test]
fn no_evaluate_is_sent_without_supports_evaluate_for_hovers(cx: &mut TestAppContext) {
    let (mut debug, fake, editor) = stopped(cx, script);
    let offset = offset_of(&mut debug, &editor, "point", 1);
    let buffer = buffer_of(&mut debug, &editor);
    let workbench = debug.workbench.clone();
    let refused = workbench.update(debug.cx, |workbench, cx| {
        workbench
            .debug_hover_plan(buffer, None, Probe::Word(offset), cx)
            .err()
    });
    assert_eq!(refused, Some(debug_hover::Refusal::Unsupported));
    editor.update(debug.cx, |view, cx| {
        view.select_and_reveal(offset..offset, cx)
    });
    debug.run("debug.hover");
    debug.run("debug.evaluate");
    assert_eq!(requests(&fake, "evaluate"), Vec::<serde_json::Value>::new());
    assert_eq!(popover(&mut debug, &editor), None);
    let said = tr!("workbench-debug-hover-unsupported").to_string();
    assert!(debug.toasts().iter().any(|(_, message)| *message == said));
}

/// C2: a value asked for in one stop isn't shown once the program has
/// stepped on.
#[gpui::test]
fn a_value_from_before_a_step_is_never_shown_after_it(cx: &mut TestAppContext) {
    let (mut debug, _fake, editor) = stopped(cx, hovering);
    let offset = offset_of(&mut debug, &editor, "point", 1);
    let buffer = buffer_of(&mut debug, &editor);
    let workbench = debug.workbench.clone();
    let plan = workbench
        .update(debug.cx, |workbench, cx| {
            workbench.debug_hover_plan(buffer, None, Probe::Word(offset), cx)
        })
        .expect("a plan while stopped");
    debug.run("debug.step_over");
    debug.wait_stop_at(2);
    let shown = Rc::new(RefCell::new(None));
    let weak = workbench.downgrade();
    let slot = shown.clone();
    debug.cx.update(|_, cx| {
        cx.spawn(async move |cx| {
            let reply = debug_hover::evaluate(&weak, cx, plan).await;
            *slot.borrow_mut() = Some(reply.is_ok());
        })
        .detach();
    });
    debug.until("the answer", |_| shown.borrow().is_some());
    assert_eq!(*shown.borrow(), Some(false));
    assert_eq!(debug.state(), Some(SessionState::Stopped));
}

/// C3: the value shows through the editor's hover provider, its type, value
/// and first 20 children, from the identifier the syntax tree names; the
/// debugger's part goes above whatever else the hover has.
#[gpui::test]
fn the_value_shows_above_the_language_servers_hover_with_up_to_20_children(
    cx: &mut TestAppContext,
) {
    let (mut debug, _fake, editor) = stopped(cx, with_big);
    // The caret inside `point`, not at its start.
    let offset = offset_of(&mut debug, &editor, "point", 2);
    editor.update(debug.cx, |view, cx| {
        view.select_and_reveal(offset..offset, cx)
    });
    debug.draw();
    debug.cx.simulate_keystrokes("cmd-k cmd-i");
    debug.until("the value", |debug| popover(debug, &editor).is_some());
    let mut expected = vec!["Big".to_owned(), "point = Big".to_owned()];
    expected.extend((0..CHILD_CAP).map(|n| format!("c{n:02} = {n}")));
    expected.push(tr!("workbench-debug-hover-more", count = 5).to_string());
    assert_eq!(popover(&mut debug, &editor), Some(expected.join("\n")));
}

/// Makes the language server's answer `text` over `range`, as the hover
/// provider gets it; `delay` makes it wait for a send on the channel it
/// returns.
fn server_says(
    debug: &mut Debug,
    text: &str,
    range: std::ops::Range<usize>,
    delayed: bool,
) -> Option<futures::channel::oneshot::Sender<()>> {
    let (send, receive) = futures::channel::oneshot::channel::<()>();
    let receive = Rc::new(RefCell::new(delayed.then_some(receive)));
    let text = text.to_owned();
    let host = debug
        .workbench
        .read_with(debug.cx, |workbench, _| workbench.hover_host().clone());
    *host.server.borrow_mut() = Some(Rc::new(move || {
        let wait = receive.borrow_mut().take();
        let content = fe_gpui::HoverContent {
            text: text.clone(),
            range: Some(range.clone()),
        };
        Box::pin(async move {
            if let Some(wait) = wait {
                wait.await.ok();
            }
            Some(content)
        })
    }));
    delayed.then_some(send)
}

/// What the editor's hover provider answers about `offset`, awaited.
fn provided(
    debug: &mut Debug,
    editor: &Entity<EditorView>,
    offset: usize,
) -> Option<fe_gpui::HoverContent> {
    let answer = Rc::new(RefCell::new(None));
    let slot = answer.clone();
    let (provider, buffer, version) = editor.read_with(debug.cx, |view, _| {
        let snapshot = view.model().snapshot();
        (
            view.hover_provider().cloned().expect("a hover provider"),
            snapshot.id(),
            snapshot.version(),
        )
    });
    debug.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            *slot.borrow_mut() = Some(provider.hover(buffer, version, offset).await);
        })
        .detach();
    });
    debug.until("the provider's answer", |_| answer.borrow().is_some());
    answer.borrow_mut().take().unwrap()
}

/// C3 (F5): the debugger's part and a language server's answer, through the
/// live hover: the value first, over the identifier's extent from the
/// syntax tree.
#[gpui::test]
fn the_value_goes_above_a_servers_answer_over_the_identifiers_extent(cx: &mut TestAppContext) {
    let (mut debug, _fake, editor) = stopped(cx, hovering);
    let start = offset_of(&mut debug, &editor, "point", 0);
    let inside = start + 2;
    server_says(&mut debug, "docs for point", 0..start + 40, false);
    let content = provided(&mut debug, &editor, inside).expect("a hover");
    assert_eq!(content.range, Some(start..start + "point".len()));
    assert!(content.text.ends_with("docs for point"), "{}", content.text);
    assert!(
        content.text.starts_with("```\nPoint\n```"),
        "{}",
        content.text
    );
    editor.update(debug.cx, |view, cx| {
        view.select_and_reveal(inside..inside, cx)
    });
    debug.draw();
    debug.cx.simulate_keystrokes("cmd-k cmd-i");
    debug.until("both parts", |debug| popover(debug, &editor).is_some());
    assert_eq!(
        popover(&mut debug, &editor).as_deref(),
        Some("Point\npoint = Point { x: 1, y: 2 }\nx = 1\ny = 2\ndocs for point")
    );
}

/// F1: a server that answers after the program ran on gets no value from
/// before; and a value showing goes when its stop does.
#[gpui::test]
fn a_late_server_answer_carries_no_old_value_and_a_shown_one_goes_with_its_stop(
    cx: &mut TestAppContext,
) {
    let (mut debug, _fake, editor) = stopped(cx, hovering);
    let start = offset_of(&mut debug, &editor, "point", 0);
    let release = server_says(&mut debug, "docs", 0..start + 5, true).unwrap();
    let asked = Rc::new(RefCell::new(None));
    let slot = asked.clone();
    let (provider, buffer, version) = editor.read_with(debug.cx, |view, _| {
        let snapshot = view.model().snapshot();
        (
            view.hover_provider().cloned().unwrap(),
            snapshot.id(),
            snapshot.version(),
        )
    });
    debug.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            *slot.borrow_mut() = Some(provider.hover(buffer, version, start + 2).await);
        })
        .detach();
    });
    // The value has come (the fake answers beside the app) while the
    // server's answer is waited for: the program runs on.
    debug.until("the evaluation", |debug| {
        requests_of(debug, "evaluate") >= 1
    });
    debug.run("debug.step_over");
    debug.wait_stop_at(2);
    release.send(()).unwrap();
    debug.until("the answer", |_| asked.borrow().is_some());
    let content = asked.borrow_mut().take().unwrap().expect("the server's");
    assert_eq!(content.text, "docs");

    // A value showing: the program continues, and the popover goes.
    let server = debug
        .workbench
        .read_with(debug.cx, |workbench, _| workbench.hover_host().clone());
    server.server.borrow_mut().take();
    let main = main_path(&debug.root);
    let at = offset_of(&mut debug, &editor, "point", 1);
    editor.update(debug.cx, |view, cx| view.select_and_reveal(at..at, cx));
    // Back on row 1 first: the frame is in `main` there too.
    let _ = main;
    debug.run("debug.hover");
    debug.until("the value", |debug| popover(debug, &editor).is_some());
    debug.run("debug.continue");
    debug.until("the stop's end", |debug| popover(debug, &editor).is_none());
}

/// How many `command` requests the fake got.
fn requests_of(debug: &mut Debug, command: &str) -> usize {
    debug.fake(0);
    let fake = debug.fake(0);
    requests(&fake, command).len()
}

/// F2: a replacement session reaching the old stop's epoch isn't that stop.
#[gpui::test]
fn a_replacement_session_at_the_same_epoch_is_another_stop(cx: &mut TestAppContext) {
    let (mut debug, _fake, editor) = stopped(cx, hovering);
    let offset = offset_of(&mut debug, &editor, "point", 1);
    let buffer = buffer_of(&mut debug, &editor);
    let workbench = debug.workbench.clone();
    let plan = workbench
        .update(debug.cx, |workbench, cx| {
            workbench.debug_hover_plan(buffer, None, Probe::Word(offset), cx)
        })
        .expect("a plan");
    let old = plan.stop();
    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
    debug.start("main");
    debug.wait_stop_at(1);
    let debugger = debug.debugger();
    let epoch = debugger.read_with(debug.cx, |debugger, _| {
        debugger.stop_location().unwrap().epoch
    });
    assert_eq!(epoch, old.epoch, "the same epoch number in the new session");
    let (stale, fresh) = workbench.update(debug.cx, |workbench, cx| {
        let new = crate::debug_hover::StopId {
            session: debugger.read(cx).session_id().unwrap(),
            epoch,
        };
        (
            workbench.debug_hover_current(old, cx),
            workbench.debug_hover_current(new, cx),
        )
    });
    assert_eq!((stale, fresh), (false, true));
}

/// F3: a command's answer isn't shown at a caret that has moved since.
#[gpui::test]
fn a_command_answer_is_dropped_when_the_caret_moved(cx: &mut TestAppContext) {
    let (mut debug, _fake, editor) = stopped(cx, hovering);
    let offset = offset_of(&mut debug, &editor, "point", 1);
    let elsewhere = offset_of(&mut debug, &editor, "add", 1);
    editor.update(debug.cx, |view, cx| {
        view.select_and_reveal(offset..offset, cx)
    });
    let workbench = debug.workbench.clone();
    // The command and the move, with no frame between them.
    debug.cx.update(|_, cx| {
        workbench.update(cx, |workbench, cx| workbench.debug_show_value(false, cx));
        editor.update(cx, |view, cx| {
            view.select_and_reveal(elsewhere..elsewhere, cx)
        });
    });
    debug.until("a while", |debug| requests_of(debug, "evaluate") >= 1);
    for _ in 0..5 {
        debug.draw();
    }
    assert_eq!(popover(&mut debug, &editor), None);
}

/// F4: the adapter's text is shown as it gave it: spaces kept, lines kept, a
/// fence of backticks inside it harmless.
#[gpui::test]
fn the_value_is_shown_exactly_as_the_adapter_gave_it(cx: &mut TestAppContext) {
    let (mut debug, _fake, editor) = stopped(cx, |root| {
        let mut script = hovering(root);
        script.program.functions[0].locals[0] = Local::new("x", "a  b\n```c\n\td", "i32");
        script
    });
    let offset = offset_of(&mut debug, &editor, "let x", 4);
    editor.update(debug.cx, |view, cx| {
        view.select_and_reveal(offset..offset, cx)
    });
    debug.run("debug.hover");
    debug.until("the value", |debug| popover(debug, &editor).is_some());
    assert_eq!(
        popover(&mut debug, &editor).as_deref(),
        Some("i32\nx = a  b\n```c\n\td")
    );
}

/// C4: `debug.hover` shows the caret's identifier, `debug.evaluate` the
/// selection, each in a popover.
#[gpui::test]
fn debug_hover_and_debug_evaluate_show_a_popover(cx: &mut TestAppContext) {
    let (mut debug, fake, editor) = stopped(cx, hovering);
    let offset = offset_of(&mut debug, &editor, "point", 1);
    editor.update(debug.cx, |view, cx| {
        view.select_and_reveal(offset..offset, cx)
    });
    debug.run("debug.hover");
    debug.until("the hover", |debug| popover(debug, &editor).is_some());
    assert_eq!(
        popover(&mut debug, &editor).as_deref(),
        Some("Point\npoint = Point { x: 1, y: 2 }\nx = 1\ny = 2")
    );
    let sent = requests(&fake, "evaluate");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["context"], "hover");
    assert_eq!(sent[0]["expression"], "point");
    // The selection `x`, from the line above.
    let x = offset_of(&mut debug, &editor, "x", 0);
    editor.update(debug.cx, |view, cx| view.select_and_reveal(x..x + 1, cx));
    debug.run("debug.evaluate");
    debug.until("the evaluation", |debug| {
        popover(debug, &editor).as_deref() == Some("i32\nx = 1")
    });
}
