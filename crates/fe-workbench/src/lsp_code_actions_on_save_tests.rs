//! Code actions on save: the kinds in `editor.code_actions_on_save` run
//! before format on save, within the save's time limit.

use std::{cell::Cell, fs, rc::Rc, sync::Arc, time::Duration};

use fe_config::Settings;
use fe_gpui::SettingsEvent;
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{
    ApplyWorkspaceEditParams, CodeActionProviderCapability, ExecuteCommandOptions, OneOf,
    PositionEncodingKind, ServerCapabilities, WorkspaceEdit,
    request::{ApplyWorkspaceEdit, ExecuteCommand, Request},
};
use serde_json::{Value, json};

use super::{FORMATTING, TABS, edit, editor, on_disk, requests, text};
use crate::tests::lsp::Lsp;

const CODE_ACTION: &str = "textDocument/codeAction";
const ORGANIZE: &str = "source.organizeImports";
/// The save's limit (`SAVE_HOOK_LIMIT`).
const LIMIT: Duration = Duration::from_secs(1);

/// A rust server that organizes imports and formats.
fn server() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["fix.all".into()],
            ..Default::default()
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn organize(s: &Lsp) -> Value {
    json!([{
        "title": "Organize imports",
        "kind": ORGANIZE,
        "edit": { "changes": { s.uri("src/main.rs").to_string(): [edit(1, 1, 2, "ff")] } },
    }])
}

fn configure(s: &mut Lsp, kinds: &[&str], format: bool) {
    let mut settings = Settings::default();
    settings.editor.format_on_save = format;
    settings.editor.code_actions_on_save = kinds.iter().map(|kind| (*kind).to_owned()).collect();
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    (s.workbench).update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

fn setup(cx: &mut TestAppContext, fake_clock: bool) -> (Lsp, gpui::Entity<fe_gpui::EditorView>) {
    let mut s = match fake_clock {
        true => Lsp::with_fake_clock(cx, server(), Script::default()),
        false => Lsp::with_scripts(cx, server(), Script::default()),
    };
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

#[gpui::test]
fn organize_imports_runs_before_format(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false);
    let reply = organize(&s);
    s.fake
        .set_reply(CODE_ACTION, Scripted::now(Reply::Result(reply)));
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(2, 1, 2, "gg")]))),
    );
    configure(&mut s, &[ORGANIZE], true);
    s.cx.simulate_keystrokes("cmd-s");
    let both = TABS.replace("\tf()", "\tff()").replace("\tg()", "\tgg()");
    s.until("both edits written", |s| on_disk(s, "src/main.rs") == both);
    assert_eq!(text(&s, &editor), both);

    let asked = requests(&s, CODE_ACTION);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["context"]["only"], json!([ORGANIZE]));
    assert_eq!(requests(&s, FORMATTING).len(), 1);
    // The code action went first.
    use fe_lsp::fake::Record;
    let order: Vec<String> = (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request { method, .. } if method == CODE_ACTION || method == FORMATTING => {
                Some(method)
            }
            _ => None,
        })
        .collect();
    assert_eq!(order, [CODE_ACTION, FORMATTING]);
}

#[gpui::test]
fn no_setting_no_request(cx: &mut TestAppContext) {
    let (mut s, _editor) = setup(cx, false);
    configure(&mut s, &[], false);
    assert!(s.workbench.read_with(&s.cx, |w, cx| {
        w.active_editor(cx)
            .is_some_and(|editor| editor.read(cx).save_hook().is_none())
    }));
    s.cx.simulate_keystrokes("cmd-s");
    s.cx.run_until_parked();
    s.draw();
    assert!(requests(&s, CODE_ACTION).is_empty());
    assert!(requests(&s, FORMATTING).is_empty());
}

#[gpui::test]
fn a_slow_server_doesnt_hold_the_save(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    let reply = organize(&s);
    s.fake.set_reply(
        CODE_ACTION,
        Scripted {
            reply: Reply::Result(reply),
            delay: LIMIT * 3,
        },
    );
    configure(&mut s, &[ORGANIZE], false);
    // Make the buffer differ from the file, so a written save shows.
    s.cx.simulate_input("x");
    let saved = Rc::new(Cell::new(false));
    let done = saved.clone();
    let task =
        s.cx.update(|window, cx| editor.update(cx, |view, cx| view.save(window, cx)));
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            let _ = task.await;
            done.set(true);
        })
        .detach()
    });
    s.until("the request", |s| requests(s, CODE_ACTION).len() == 1);
    // The fake holds its reply on its own clock, not the save's.
    s.clock.wait_for_sleep(LIMIT * 3);
    assert!(!saved.get(), "held for the reply");
    // Past the save's limit, the reply still to come.
    // That is all the executor time the save gets: the wait below is for the
    // file's write, not the clock.
    s.cx.executor()
        .advance_clock(LIMIT + Duration::from_millis(50));
    s.until("the save done", |_| saved.get());
    let written = format!("x{TABS}");
    assert_eq!(on_disk(&s, "src/main.rs"), written);
    // The late reply is never applied.
    s.clock.advance(LIMIT * 3);
    // The client has taken it in once it has answered a request the fake
    // sent after it was on the wire.
    settle_after(&mut s, CODE_ACTION);
    assert_eq!(text(&s, &editor), written);
    assert_eq!(on_disk(&s, "src/main.rs"), written);
}

/// Returns once the fake has put its reply to the first request of `method`
/// on the wire (`wait_sent`), and the client has then answered an empty
/// `workspace/applyEdit` sent after it: the reply has been taken in.
fn settle_after(s: &mut Lsp, method: &str) {
    let id = (s.fake.records().into_iter())
        .find_map(|record| match record {
            fe_lsp::fake::Record::Request { id, method: m, .. } if m == method => Some(id),
            _ => None,
        })
        .expect("the request was made");
    let id = serde_json::to_value(id).unwrap();
    s.fake.wait_sent(1, "the late reply", |sent| {
        sent.iter()
            .any(|message| message["id"] == id && message.get("result").is_some())
    });
    settle(s);
}

/// Returns once the client has answered an empty `workspace/applyEdit` the
/// fake sends now: whatever the fake sent before it has been taken in.
fn settle(s: &mut Lsp) {
    let pushed = apply(s, WorkspaceEdit::default());
    s.until("the client's answer", |_| pushed.is_finished());
    assert_eq!(pushed.join().unwrap()["applied"], true);
}

fn apply(s: &Lsp, edit: WorkspaceEdit) -> std::thread::JoinHandle<Value> {
    let params = ApplyWorkspaceEditParams { label: None, edit };
    let fake = s.fake.clone();
    std::thread::spawn(move || fake.request::<ApplyWorkspaceEdit>(params).unwrap())
}

/// An action of `kind` editing `line`'s `f`/`g` to twice itself.
fn action(s: &Lsp, title: &str, line: u32, text: &str) -> Value {
    json!({
        "title": title,
        "kind": ORGANIZE,
        "edit": { "changes": { s.uri("src/main.rs").to_string(): [edit(line, 1, 2, text)] } },
    })
}

#[gpui::test]
fn formatting_goes_on_when_the_actions_run_out_of_time(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    let reply = organize(&s);
    s.fake.set_reply(
        CODE_ACTION,
        Scripted {
            reply: Reply::Result(reply),
            delay: LIMIT * 5,
        },
    );
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(2, 1, 2, "gg")]))),
    );
    configure(&mut s, &[ORGANIZE], true);
    let saved = Rc::new(Cell::new(false));
    let done = saved.clone();
    let task =
        s.cx.update(|window, cx| editor.update(cx, |view, cx| view.save(window, cx)));
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            let _ = task.await;
            done.set(true);
        })
        .detach()
    });
    s.until("the request", |s| requests(s, CODE_ACTION).len() == 1);
    s.clock.wait_for_sleep(LIMIT * 5);
    // The actions' 750 ms are up before the save's second is: formatting
    // runs in what's left, and the late action never applies.
    s.cx.executor().advance_clock(Duration::from_millis(800));
    s.until("the format asked", |s| requests(s, FORMATTING).len() == 1);
    s.until("the save done", |_| saved.get());
    let formatted = TABS.replace("\tg()", "\tgg()");
    s.until("the formatted text written", |s| {
        on_disk(s, "src/main.rs") == formatted
    });
    s.clock.advance(LIMIT * 5);
    settle_after(&mut s, CODE_ACTION);
    assert_eq!(text(&s, &editor), formatted);
}

#[gpui::test]
fn a_second_action_isnt_started_after_the_limit(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    let mut first = action(&s, "First", 1, "ff");
    first["command"] = json!({ "title": "Fix", "command": "fix.all" });
    let both = json!([first, action(&s, "Second", 2, "gg")]);
    s.fake
        .set_reply(CODE_ACTION, Scripted::now(Reply::Result(both)));
    s.fake.set_reply(
        ExecuteCommand::METHOD,
        Scripted {
            reply: Reply::Result(Value::Null),
            delay: LIMIT * 5,
        },
    );
    configure(&mut s, &[ORGANIZE], false);
    let saved = Rc::new(Cell::new(false));
    let done = saved.clone();
    let task =
        s.cx.update(|window, cx| editor.update(cx, |view, cx| view.save(window, cx)));
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            let _ = task.await;
            done.set(true);
        })
        .detach()
    });
    s.until("the command", |s| {
        requests(s, ExecuteCommand::METHOD).len() == 1
    });
    s.clock.wait_for_sleep(LIMIT * 5);
    // The limit passes with the first action's command still running, and
    // the command then ends: the second action is not started.
    s.cx.executor().advance_clock(Duration::from_millis(800));
    s.until("the save done", |_| saved.get());
    s.clock.advance(LIMIT * 5);
    settle_after(&mut s, ExecuteCommand::METHOD);
    let first_only = TABS.replace("\tf()", "\tff()");
    assert_eq!(text(&s, &editor), first_only);
    assert_eq!(on_disk(&s, "src/main.rs"), first_only);
}

#[gpui::test]
fn typing_while_the_actions_are_asked_for_drops_them(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    // No edit to go stale: only the buffer's version tells.
    let command = json!([{
        "title": "Fix everything",
        "kind": ORGANIZE,
        "command": { "title": "Fix everything", "command": "fix.all" },
    }]);
    let delay = Duration::from_millis(300);
    s.fake.set_reply(
        CODE_ACTION,
        Scripted {
            reply: Reply::Result(command),
            delay,
        },
    );
    configure(&mut s, &[ORGANIZE], false);
    let saved = Rc::new(Cell::new(false));
    let done = saved.clone();
    let task =
        s.cx.update(|window, cx| editor.update(cx, |view, cx| view.save(window, cx)));
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            let _ = task.await;
            done.set(true);
        })
        .detach()
    });
    s.until("the request", |s| requests(s, CODE_ACTION).len() == 1);
    s.clock.wait_for_sleep(delay);
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert("x")));
    s.clock.advance(delay);
    settle_after(&mut s, CODE_ACTION);
    s.until("the save done", |_| saved.get());
    assert!(requests(&s, ExecuteCommand::METHOD).is_empty());
}
