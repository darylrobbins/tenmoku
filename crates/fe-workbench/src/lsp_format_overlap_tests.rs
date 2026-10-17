//! A manual format that overlaps an auto-save's hook: only the save's own
//! format answers to the save's rules.

use std::{fs, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::Settings;
use fe_editor::auto_save::SaveReason;
use fe_gpui::{EditorView, SettingsEvent, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::json;
use std::sync::Arc;

use crate::tests::lsp::Lsp;

const FORMATTING: &str = "textDocument/formatting";
/// Tab-indented.
const TABS: &str = "fn main() {\n\tf();\n\tg();\n}\n";
const DELAY: Duration = Duration::from_millis(300);

fn formatter() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_formatting_provider: Some(OneOf::Left(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// A formatting reply of one edit on `line`, held for `delay` on the fake
/// clock.
fn reply(line: u32, delay: Duration) -> Scripted {
    Scripted {
        reply: Reply::Result(json!([{
            "range": {
                "start": { "line": line, "character": 1 },
                "end": { "line": line, "character": 2 },
            },
            "newText": format!("{0}{0}", if line == 1 { "f" } else { "g" }),
        }])),
        delay,
    }
}

/// `src/main.rs` open and focused, with vim on and format on save on.
fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, formatter(), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), TABS).unwrap();
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some())
        .unwrap();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    let mut settings = Settings::default();
    settings.editor.format_on_save = true;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    (s.workbench).update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
    (s, editor)
}

/// Pumps the UI until `task` finishes.
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

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn requests(s: &Lsp) -> usize {
    (s.fake.records().into_iter())
        .filter(|record| matches!(record, Record::Request { method, .. } if method == FORMATTING))
        .count()
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn format_document(s: &mut Lsp) {
    let stack = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let invocation = Invocation::of("lsp.format_document")
        .unwrap()
        .context(stack);
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

/// An auto-save on focus change of `editor`, in normal mode, so its hook
/// runs.
fn auto_save(s: &mut Lsp, editor: &Entity<EditorView>) -> gpui::Task<gpui::Result<()>> {
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.save_with(SaveReason::AutoFocus, window, cx)
        })
    })
}

#[gpui::test]
fn a_manual_format_replying_during_an_auto_save_hook_applies(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    // The manual reply is due first; the save's stays pending after it.
    s.fake.set_reply(FORMATTING, reply(1, DELAY));
    format_document(&mut s);
    s.until("the manual request", |s| requests(s) == 1);
    s.clock.wait_for_sleep(DELAY);
    s.fake.set_reply(FORMATTING, reply(2, DELAY * 2));
    let saved = auto_save(&mut s, &editor);
    s.until("the save's request", |s| requests(s) == 2);
    s.clock.wait_for_sleep(DELAY * 2);
    // Insert mode, with the text as it was.
    s.cx.simulate_keystrokes("i");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);
    s.clock.advance(DELAY);
    let formatted = TABS.replace("\tf()", "\tff()");
    s.until("the manual format applied", |s| {
        text(s, &editor) == formatted
    });
    assert_eq!(toasts(&s), Vec::<String>::new());
    // The save's own reply then comes, and is dropped.
    s.clock.advance(DELAY);
    assert!(block(&mut s, saved).is_ok());
    assert_eq!(text(&s, &editor), formatted);
}

#[gpui::test]
fn a_save_format_replying_after_insert_is_still_dropped(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    s.fake.set_reply(FORMATTING, reply(1, DELAY));
    let saved = auto_save(&mut s, &editor);
    s.until("the save's request", |s| requests(s) == 1);
    s.clock.wait_for_sleep(DELAY);
    s.cx.simulate_keystrokes("i");
    s.draw();
    s.clock.advance(DELAY);
    assert!(block(&mut s, saved).is_ok());
    assert_eq!(text(&s, &editor), TABS);
}
