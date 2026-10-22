//! Code lenses through the fake server: asked for when a buffer opens and
//! after edits, resolved, shown as rows above their lines, run on a click,
//! asked again on a refresh, and gone when `editor.code_lens` is off.

use std::{sync::Arc, time::Duration};

use fe_command::{ContextFrame, ContextStack, Source};
use fe_config::{Settings, SettingsEvent};
use fe_gpui::{EditorView, Lens, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{
    CodeLensOptions, ExecuteCommandOptions, PositionEncodingKind, ServerCapabilities,
    request::CodeLensRefresh,
};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const LENS: &str = "textDocument/codeLens";
const RESOLVE: &str = "codeLens/resolve";
const EXECUTE: &str = "workspace/executeCommand";
const REST: Duration = Duration::from_millis(250);
const MAIN: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(true),
        }),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec!["rust-analyzer.runSingle".into()],
            ..Default::default()
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn row3() -> Value {
    json!({ "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 9 } })
}

/// Row 3's two lenses: `run`, with its command, and one that resolves to
/// `2 references`, a command the client runs.
fn answer(run: &str) -> Scripted {
    Scripted::now(Reply::Result(json!([
        { "range": row3(), "command": {
            "title": run, "command": "rust-analyzer.runSingle", "arguments": [{ "n": 1 }] } },
        { "range": row3(), "data": { "id": 1 } },
    ])))
}

fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    s.fake.set_reply(LENS, answer("Run"));
    s.fake.set_reply(
        RESOLVE,
        Scripted::now(Reply::Result(json!({
            "range": row3(), "data": { "id": 1 },
            "command": { "title": "2 references", "command": "rust-analyzer.showReferences" },
        }))),
    );
    s.fake
        .set_reply(EXECUTE, Scripted::now(Reply::Result(Value::Null)));
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    (s, editor)
}

fn requests(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params.clone()),
            _ => None,
        })
        .collect()
}

/// The lens rows `editor` shows: row, then the item titles.
fn shown(s: &Lsp, editor: &Entity<EditorView>) -> Vec<(u32, Vec<String>)> {
    editor.read_with(&s.cx, |view, _| {
        let snapshot = view.model().snapshot().clone();
        (view.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .filter(|lens: &Lens| !lens.items.is_empty())
            .map(|lens| {
                (
                    lens.row,
                    lens.items.iter().map(|i| i.title.to_string()).collect(),
                )
            })
            .collect()
    })
}

fn row_of(titles: &[&str], row: u32) -> Vec<(u32, Vec<String>)> {
    vec![(row, titles.iter().map(|t| (*t).to_owned()).collect())]
}

/// `until`, advancing the manager's manual clock 20 ms per poll.
fn until_ticking(s: &mut Lsp, what: &str, done: impl Fn(&mut Lsp) -> bool) {
    s.until(what, |s| {
        s.clock.advance(Duration::from_millis(20));
        done(s)
    });
}

fn type_in(s: &mut Lsp, editor: &Entity<EditorView>, text: &str) {
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().insert(text);
            cx.notify();
        })
    });
    s.draw();
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// Clicks item `item` of the first lens row.
fn click(s: &mut Lsp, editor: &Entity<EditorView>, item: usize) {
    let invocation = editor.read_with(&s.cx, |view, _| {
        let snapshot = view.model().snapshot().clone();
        (view.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .find(|lens| !lens.items.is_empty())
            .expect("a lens row")
            .items[item]
            .invocation
            .clone()
    });
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(invocation.context(context).source(Source::Mouse));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

#[gpui::test]
fn server_lenses_show_above_their_rows(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    until_ticking(&mut s, "the lenses", |s| !shown(s, &editor).is_empty());
    assert_eq!(shown(&s, &editor), row_of(&["Run", "2 references"], 3));
    assert_eq!(requests(&s, LENS).len(), 1);
    assert_eq!(requests(&s, RESOLVE).len(), 1);

    // An edit asks again 250 ms later; the old row follows the text and
    // stays until the answer lands.
    s.fake.set_reply(
        LENS,
        Scripted {
            reply: answer("Run again").reply,
            delay: Duration::from_secs(1),
        },
    );
    editor.update(&mut s.cx, |view, _| view.model_mut().set_cursor(0));
    type_in(&mut s, &editor, "// x\n");
    assert_eq!(requests(&s, LENS).len(), 1);
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
    until_ticking(&mut s, "the second request", |s| {
        requests(s, LENS).len() == 2
    });
    assert_eq!(shown(&s, &editor), row_of(&["Run", "2 references"], 4));
    s.clock.advance(Duration::from_secs(1));
    until_ticking(&mut s, "the new answer", |s| {
        shown(s, &editor) == row_of(&["Run again", "2 references"], 3)
    });
}

#[gpui::test]
fn clicking_a_lens_runs_its_command(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    until_ticking(&mut s, "the lenses", |s| !shown(s, &editor).is_empty());

    click(&mut s, &editor, 0);
    until_ticking(&mut s, "the command", |s| !requests(s, EXECUTE).is_empty());
    let sent = requests(&s, EXECUTE);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["command"], "rust-analyzer.runSingle");
    assert_eq!(sent[0]["arguments"], json!([{ "n": 1 }]));

    // A command the server doesn't list is the client's, which can't run
    // it yet: said, and not sent.
    click(&mut s, &editor, 1);
    until_ticking(&mut s, "the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["2 references: this command isn't supported"]);
    assert_eq!(requests(&s, EXECUTE).len(), 1);
}

fn set_lens_setting(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.code_lens = on;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

#[gpui::test]
fn refresh_and_the_setting(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    until_ticking(&mut s, "the lenses", |s| !shown(s, &editor).is_empty());

    // A refresh asks at once.
    s.fake.set_reply(LENS, answer("Refreshed"));
    let fake = s.fake.clone();
    let refresh = std::thread::spawn(move || fake.request::<CodeLensRefresh>(()));
    until_ticking(&mut s, "the second request", |s| {
        requests(s, LENS).len() == 2
    });
    until_ticking(&mut s, "the refreshed lenses", |s| {
        shown(s, &editor) == row_of(&["Refreshed", "2 references"], 3)
    });
    refresh.join().unwrap().ok();

    // Off, the rows go and nothing is asked after edits.
    set_lens_setting(&mut s, false);
    assert_eq!(shown(&s, &editor), vec![]);
    type_in(&mut s, &editor, "a");
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
    assert_eq!(requests(&s, LENS).len(), 2);
    assert_eq!(shown(&s, &editor), vec![]);

    // On again, they're asked for again.
    set_lens_setting(&mut s, true);
    until_ticking(&mut s, "the request after turning on", |s| {
        requests(s, LENS).len() == 3
    });
}
