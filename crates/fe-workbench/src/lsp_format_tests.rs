//! Formatting through the fake server: the document and a selection as one
//! undoable step with the editor's options, and the cases that show
//! nothing, or a message.

use std::{fs, time::Duration};

use fe_buffer::ResolvedSelection;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use super::FormatOutcome;
use crate::tests::lsp::Lsp;

const FORMATTING: &str = "textDocument/formatting";
const RANGE: &str = "textDocument/rangeFormatting";
/// Tab-indented.
const TABS: &str = "fn main() {\n\tf();\n\tg();\n\th();\n\ti();\n\tj();\n}\n";

/// A rust server that formats documents and ranges.
fn formatter() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn edit(line: u32, from: u32, to: u32, text: &str) -> Value {
    json!({
        "range": {
            "start": { "line": line, "character": from },
            "end": { "line": line, "character": to },
        },
        "newText": text,
    })
}

/// The workbench with `src/main.rs` (as TABS) and `tool.py` open on
/// servers, the first formatting, its editor focused.
fn setup(cx: &mut TestAppContext, rust: Script) -> (Lsp, Entity<EditorView>) {
    setup_on(cx, rust, Lsp::with_scripts)
}

/// [`setup`] with the workbench built by `make`.
fn setup_on(
    cx: &mut TestAppContext,
    rust: Script,
    make: fn(&mut TestAppContext, Script, Script) -> Lsp,
) -> (Lsp, Entity<EditorView>) {
    let mut s = make(cx, rust, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), TABS).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let editor = editor(&mut s);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    (s, editor)
}

fn editor(s: &mut Lsp) -> Entity<EditorView> {
    (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn run(s: &mut Lsp, command: &str) {
    let stack = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let invocation = Invocation::of(command).unwrap().context(stack);
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

/// The params of the requests of `method` the fake got.
fn requests(s: &Lsp, method: &str) -> Vec<Value> {
    use fe_lsp::fake::Record;
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

#[gpui::test]
fn document_and_selection(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, formatter());
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(1, 1, 2, "ff")]))),
    );
    s.fake.set_reply(
        RANGE,
        Scripted::now(Reply::Result(json!([edit(2, 1, 2, "gg")]))),
    );
    let tab_size = editor.read_with(&s.cx, |view, _| view.model().tab_size());

    // The document: edited as one step, with the editor's options.
    run(&mut s, "lsp.format_document");
    let documented = TABS.replace("\tf()", "\tff()");
    s.until("the document formatted", |s| text(s, &editor) == documented);
    let sent = requests(&s, FORMATTING);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["options"]["insertSpaces"], json!(false));
    assert_eq!(sent[0]["options"]["tabSize"], json!(tab_size));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);

    // A selection inside the third row: that row, whole.
    let inside = TABS.find("g()").unwrap();
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            view.model_mut().set_selections([ResolvedSelection {
                range: inside..inside + 1,
                reversed: false,
            }]);
        })
    });
    run(&mut s, "lsp.format_selection");
    let selected = TABS.replace("\tg()", "\tgg()");
    s.until("the selection formatted", |s| text(s, &editor) == selected);
    let sent = requests(&s, RANGE);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["range"],
        json!({
            "start": { "line": 2, "character": 0 },
            "end": { "line": 3, "character": 0 },
        })
    );
    assert_eq!(sent[0]["options"]["insertSpaces"], json!(false));
    assert_eq!(sent[0]["options"]["tabSize"], json!(tab_size));
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);
}

#[gpui::test]
fn stale_missing_and_slow(cx: &mut TestAppContext) {
    let (mut s, editor) = setup_on(cx, formatter(), Lsp::with_fake_clock);
    // Typed before the reply: its edits are dropped, with nothing shown.
    // The fake holds the reply on the test's clock: the test sees the
    // request, types, then releases the reply and waits for the format to
    // finish.
    let delay = Duration::from_millis(300);
    s.fake.set_reply(
        FORMATTING,
        Scripted {
            reply: Reply::Result(json!([edit(1, 1, 2, "ff")])),
            delay,
        },
    );
    let task = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(&editor, cx));
    s.until("the request", |s| !requests(s, FORMATTING).is_empty());
    s.clock.wait_for_sleep(delay);
    s.cx.simulate_input("x");
    s.draw();
    let typed = format!("x{TABS}");
    assert_eq!(text(&s, &editor), typed);
    s.clock.advance(delay);
    // The format finishes, its edits dropped: nothing changed, nothing shown.
    assert_eq!(block(&mut s, task), Ok(FormatOutcome::Unchanged));
    s.draw();
    assert_eq!(text(&s, &editor), typed);
    assert_eq!(toasts(&s), Vec::<String>::new());

    // No formatter: a message naming the language.
    fs::write(s.path("tool.py"), "print(1)\n").unwrap();
    s.open("tool.py");
    s.python
        .wait_for_document(&s.uri("tool.py"), "didOpen", |r| r.is_some());
    let python = self::editor(&mut s);
    s.until("its language", |s| {
        python.read_with(&s.cx, |view, _| view.syntax().is_some())
    });
    run(&mut s, "lsp.format_document");
    s.until("the message", |s| toasts(s) == ["No formatter for Python"]);

    // format_buffer says what happened.
    let outcome = |s: &mut Lsp, editor: &Entity<EditorView>| {
        let task = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(editor, cx));
        let outcome = block(s, task);
        s.draw();
        outcome
    };
    assert_eq!(outcome(&mut s, &python), Ok(FormatOutcome::NoFormatter));
    s.fake
        .set_reply(FORMATTING, Scripted::now(Reply::Result(json!([]))));
    assert_eq!(outcome(&mut s, &editor), Ok(FormatOutcome::Unchanged));
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(1, 1, 2, "ff")]))),
    );
    assert_eq!(outcome(&mut s, &editor), Ok(FormatOutcome::Formatted));
    assert_eq!(text(&s, &editor), typed.replace("\tf()", "\tff()"));

    // A server that doesn't answer: a message with the timeout.
    let now = text(&s, &editor);
    s.draw();
    (s.fake)
        .wait_for_document(&s.uri("src/main.rs"), "didChange", |r| {
            r.is_some_and(|r| r.text == now)
        })
        .expect("the server caught up");
    s.fake.set_reply(FORMATTING, Scripted::now(Reply::Never));
    let slow = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(&editor, cx));
    s.until("the timer", |s| {
        s.clock.sleeping().contains(&Duration::from_secs(3))
    });
    s.clock.advance(Duration::from_secs(3));
    assert_eq!(block(&mut s, slow), Err("the request timed out".to_owned()));
    s.open("src/main.rs");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    run(&mut s, "lsp.format_document");
    s.until("the timer", |s| {
        s.clock.sleeping().contains(&Duration::from_secs(3))
    });
    s.clock.advance(Duration::from_secs(3));
    s.until("the timeout shown", |s| {
        toasts(s).contains(&"the request timed out".to_owned())
    });
}

/// A format answered after the buffer was saved under another name is
/// dropped: its edits were for the old file's document.
#[gpui::test]
fn a_format_across_a_save_as(cx: &mut TestAppContext) {
    let (mut s, editor) = setup_on(cx, formatter(), Lsp::with_fake_clock);
    // The fake holds the reply on the test's clock: the test sees the
    // request, saves under another name, then releases the reply.
    let delay = Duration::from_millis(300);
    s.fake.set_reply(
        FORMATTING,
        Scripted {
            reply: Reply::Result(json!([edit(1, 1, 2, "ff")])),
            delay,
        },
    );
    let task = (s.workbench).update(&mut s.cx, |w, cx| w.format_buffer(&editor, cx));
    s.until("the request", |s| !requests(s, FORMATTING).is_empty());
    s.clock.wait_for_sleep(delay);
    let moved = s.path("src/moved.rs");
    s.cx.update(|window, cx| {
        (s.workbench.clone()).update(cx, |w, cx| {
            w.save_editor_as(&editor, moved.clone(), window, cx)
        })
    });
    s.until("the save as", |_| moved.exists());
    s.clock.advance(delay);
    assert_eq!(block(&mut s, task), Ok(FormatOutcome::Unchanged));
    assert_eq!(text(&s, &editor), TABS);
}

/// `task`'s answer, drawing until it has one.
fn block<T: 'static>(s: &mut Lsp, task: gpui::Task<T>) -> T {
    let answer = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = answer.clone();
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach()
    });
    s.until("the task", |_| answer.borrow().is_some());
    answer.take().unwrap()
}

#[test]
fn a_changed_buffer_is_no_error() {
    use fe_lsp::LspError;
    let changed = LspError::Unavailable("buffer changed".into());
    assert_eq!(super::failure(changed), Ok(FormatOutcome::Unchanged));
    let other = LspError::Unavailable("too large for the language server".into());
    assert!(super::failure(other).is_err());
    assert!(super::failure(LspError::Timeout).is_err());
}

#[gpui::test]
fn caught_up_options_and_one_step(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, formatter());
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(1, 1, 2, "ff")]))),
    );
    s.fake.set_reply(
        RANGE,
        Scripted::now(Reply::Result(json!([edit(3, 1, 2, "gg")]))),
    );

    // Spaces of a width other than the tab size: the tab size is still the
    // editor's.
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            let model = view.model_mut();
            model.set_tab_size(8);
            model.set_indent_style(Some(fe_editor::IndentStyle::Spaces(2)));
        })
    });
    run(&mut s, "lsp.format_document");
    s.until("the document formatted", |s| text(s, &editor) != TABS);
    let sent = requests(&s, FORMATTING);
    assert_eq!(sent[0]["options"]["insertSpaces"], json!(true));
    assert_eq!(sent[0]["options"]["tabSize"], json!(8));

    // An open transaction of the user (a Vim insert) isn't joined: one undo
    // takes back the format alone, then the typing.
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            let model = view.model_mut();
            let mut buffer = model.buffer_mut();
            buffer.start_transaction(fe_buffer::Origin::User);
            buffer.edit([(0..0, "// c\n")], fe_buffer::Origin::User);
        })
    });
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(2, 1, 2, "ff")]))),
    );
    run(&mut s, "lsp.format_document");
    let both = format!("// c\n{TABS}");
    s.until("typed and formatted", |s| {
        text(s, &editor) == both.replace("\tf()", "\tff()")
    });
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            view.model_mut().buffer_mut().end_transaction();
        })
    });
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), both);

    // An edit through the buffer the editor hasn't caught up with: the rows
    // sent are those of the text the version names.
    let at = both.find("g()").unwrap();
    s.cx.update(|_, cx| {
        editor.update(cx, |view, _| {
            let model = view.model_mut();
            model.sync_display();
            model.set_selections([ResolvedSelection {
                range: at..at + 1,
                reversed: false,
            }]);
            model
                .buffer_mut()
                .edit([(0..0, "// d\n")], fe_buffer::Origin::User);
        })
    });
    run(&mut s, "lsp.format_selection");
    s.until("the range asked", |s| !requests(s, RANGE).is_empty());
    assert_eq!(
        requests(&s, RANGE)[0]["range"],
        json!({
            "start": { "line": 4, "character": 0 },
            "end": { "line": 5, "character": 0 },
        })
    );
}
