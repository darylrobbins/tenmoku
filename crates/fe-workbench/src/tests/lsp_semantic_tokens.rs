//! Semantic tokens through the fake server: asked for when the buffer opens,
//! as a delta after edits, again on a refresh, and never when
//! `editor.semantic_highlighting` is off.

use std::{sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent};
use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::request::SemanticTokensRefresh;
use lsp_types::{
    PositionEncodingKind, SemanticTokenModifier, SemanticTokenType, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensServerCapabilities,
    ServerCapabilities,
};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const FULL: &str = "textDocument/semanticTokens/full";
const DELTA: &str = "textDocument/semanticTokens/full/delta";
const REST: Duration = Duration::from_millis(250);

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: vec![
                        SemanticTokenType::PARAMETER,
                        SemanticTokenType::FUNCTION,
                        SemanticTokenType::VARIABLE,
                    ],
                    token_modifiers: vec![SemanticTokenModifier::DEFAULT_LIBRARY],
                },
                full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
                ..Default::default()
            },
        )),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn setup(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    // `fn` is a parameter and `main` a built-in function.
    s.fake.set_reply(
        FULL,
        Scripted::now(Reply::Result(
            json!({ "resultId": "1", "data": [0, 0, 2, 0, 0, 0, 3, 4, 1, 1] }),
        )),
    );
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    // The first ask can find the server's capabilities not yet known: it
    // polls for them every 20 ms of the manager's (manual) clock, answers
    // `Retry`, and asks again after REST on the executor's (fake) clock.
    s.until("the first ask", |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(REST);
        !asked(s).is_empty()
    });
    (s, editor)
}

fn asked(s: &Lsp) -> Vec<(String, Value)> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method.starts_with(FULL) => {
                Some((method.clone(), params.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The `Foreground` decorations: bytes and highlight name.
fn painted(s: &Lsp, editor: &Entity<EditorView>) -> Vec<(std::ops::Range<usize>, String)> {
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        let mut found: Vec<_> = (model
            .decorations(0..model.snapshot().text().len())
            .into_iter())
        .filter_map(|item| match item.kind {
            DecorationKind::Foreground(name) => Some((item.range, name.to_string())),
            _ => None,
        })
        .collect();
        found.sort_by_key(|(range, _)| range.start);
        found
    })
}

fn wait(s: &mut Lsp, by: Duration) {
    s.cx.executor().advance_clock(by);
    s.draw();
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

#[gpui::test]
fn server_tokens_recolour_and_follow_edits(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx);
    s.until("the tokens", |s| !painted(s, &editor).is_empty());
    assert_eq!(
        painted(&s, &editor),
        vec![
            (0..2, "variable.parameter".to_owned()),
            (3..7, "function.builtin".to_owned()),
        ]
    );
    let sent = asked(&s);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, FULL);

    // An edit, then the wait: a delta from the first result.
    s.fake.set_reply(
        DELTA,
        Scripted::now(Reply::Result(json!({
            "resultId": "2",
            "edits": [{ "start": 0, "deleteCount": 5, "data": [0, 1, 2, 2, 0] }],
        }))),
    );
    type_in(&mut s, &editor, "a");
    wait(&mut s, Duration::from_millis(100));
    assert_eq!(asked(&s).len(), 1);
    wait(&mut s, REST);
    s.fake.wait_for("the delta request", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { method, .. } if method == DELTA))
    });
    let sent = asked(&s);
    assert_eq!(sent[1].0, DELTA);
    assert_eq!(sent[1].1["previousResultId"], json!("1"));
    s.until("the new tokens", |s| {
        painted(s, &editor).first().map(|p| p.1.as_str()) == Some("variable")
    });
    assert_eq!(
        painted(&s, &editor),
        vec![
            (1..3, "variable".to_owned()),
            (4..8, "function.builtin".to_owned()),
        ]
    );
}

fn set_setting(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.semantic_highlighting = on;
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
    s.until("the tokens", |s| painted(s, &editor).len() == 2);

    // A refresh asks at once.
    s.fake.set_reply(
        DELTA,
        Scripted::now(Reply::Result(json!({ "resultId": "2", "edits": [] }))),
    );
    let fake = s.fake.clone();
    let refresh = std::thread::spawn(move || fake.request::<SemanticTokensRefresh>(()));
    s.until("the request after a refresh", |s| asked(s).len() == 2);
    refresh.join().unwrap().ok();

    // Off, they're cleared and edits ask nothing.
    set_setting(&mut s, false);
    assert_eq!(painted(&s, &editor), vec![]);
    type_in(&mut s, &editor, "a");
    wait(&mut s, REST + Duration::from_millis(10));
    type_in(&mut s, &editor, "b");
    wait(&mut s, REST + Duration::from_millis(10));
    assert_eq!(asked(&s).len(), 2);
    assert_eq!(painted(&s, &editor), vec![]);

    // On again, it asks again.
    set_setting(&mut s, true);
    s.until("the request after turning on", |s| asked(s).len() == 3);
}

#[path = "lsp_semantic_underline.rs"]
mod underline;

#[path = "lsp_semantic_timeout.rs"]
mod timeout;
