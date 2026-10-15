//! Format on save: cmd-s formats through the fake server first when
//! `editor.format_on_save` is on, and not otherwise.

use std::{fs, sync::Arc};

use fe_config::Settings;
use fe_gpui::{EditorView, SettingsEvent};
use fe_lsp::fake::{InitializeReply, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const FORMATTING: &str = "textDocument/formatting";
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
    let mut s = Lsp::with_scripts(cx, rust, Script::default());
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

fn set_format_on_save(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.format_on_save = on;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    // No frame is drawn, so the next save is the first thing after it.
    (s.workbench).update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
}

fn on_disk(s: &Lsp, name: &str) -> String {
    fs::read_to_string(s.path(name)).unwrap()
}

#[gpui::test]
fn formats_when_on(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, formatter());
    s.fake.set_reply(
        FORMATTING,
        Scripted::now(Reply::Result(json!([edit(1, 1, 2, "ff")]))),
    );
    let formatted = TABS.replace("\tf()", "\tff()");

    // Off, the default: saved as it is, with no request.
    s.cx.simulate_keystrokes("cmd-s");
    s.cx.run_until_parked();
    s.draw();
    assert_eq!(on_disk(&s, "src/main.rs"), TABS);
    assert!(requests(&s, FORMATTING).is_empty());

    // On: the formatted text is written, and undo takes the format back.
    // Saved straight after the reload, before a frame could install a hook.
    set_format_on_save(&mut s, true);
    let saved =
        s.cx.update(|window, cx| editor.update(cx, |view, cx| view.save(window, cx)));
    drop(saved);
    s.until("the formatted text written", |s| {
        on_disk(s, "src/main.rs") == formatted
    });
    assert_eq!(requests(&s, FORMATTING).len(), 1);
    assert_eq!(text(&s, &editor), formatted);
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert_eq!(text(&s, &editor), TABS);

    // No server for the language: saved as it is, with no request to any.
    fs::write(s.path("notes.txt"), "plain\n").unwrap();
    s.open("notes.txt");
    let notes = self::editor(&mut s);
    s.cx.update(|window, cx| window.focus(&notes.focus_handle(cx), cx));
    s.draw();
    assert!(!matches!(s.active_server(), Some(Some(_))), "no server");
    s.cx.simulate_input("x");
    s.cx.simulate_keystrokes("cmd-s");
    s.until("the plain file written", |s| {
        on_disk(s, "notes.txt") == "xplain\n"
    });
    assert_eq!(requests(&s, FORMATTING).len(), 1);
    let python = s.python.records().into_iter().any(|record| {
        matches!(record, fe_lsp::fake::Record::Request { method, .. } if method == FORMATTING)
    });
    assert!(!python, "nothing asked of the other server");
}
