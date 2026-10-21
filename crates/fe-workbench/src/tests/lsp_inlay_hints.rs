//! Inlay hints through the fake server: asked for once the buffer and the
//! rows in view have held still, again on a refresh, and never when
//! `editor.inlay_hints` is off or the server has no provider.

use std::{sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent};
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext,
};
use lsp_types::request::InlayHintRefreshRequest;
use lsp_types::{InlayHintOptions, OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const METHOD: &str = "textDocument/inlayHint";
const REST: Duration = Duration::from_millis(250);

fn script(provider: bool) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        inlay_hint_provider: provider.then(|| {
            OneOf::Right(lsp_types::InlayHintServerCapabilities::Options(
                InlayHintOptions {
                    work_done_progress_options: Default::default(),
                    resolve_provider: None,
                },
            ))
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// Two hints, at 7 and 10 of `fn main() {}`, the first labelled `first`.
fn hints(first: &str) -> Scripted {
    Scripted::now(Reply::Result(json!([
        { "position": { "line": 0, "character": 7 }, "label": first, "kind": 1 },
        { "position": { "line": 0, "character": 10 }, "label": "x:", "kind": 2 },
    ])))
}

fn setup(cx: &mut TestAppContext, provider: bool) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(provider), Script::default());
    s.fake.set_reply(METHOD, hints(": ()"));
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
    (s, editor)
}

fn asked(s: &Lsp) -> Vec<Value> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == METHOD => Some(params.clone()),
            _ => None,
        })
        .collect()
}

fn shown(s: &Lsp, editor: &Entity<EditorView>) -> Vec<(usize, String)> {
    editor.read_with(&s.cx, |view, _| view.inlay_hints())
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

fn expect(first: &str) -> Vec<(usize, String)> {
    vec![(7, first.to_owned()), (10, "x:".to_owned())]
}

#[gpui::test]
fn server_hints_show_and_follow_the_view(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    wait(&mut s, Duration::from_millis(100));
    assert_eq!(asked(&s).len(), 0);
    wait(&mut s, REST);
    s.until("the hints", |s| !shown(s, &editor).is_empty());
    assert_eq!(shown(&s, &editor), expect(": ()"));
    let sent = asked(&s);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["textDocument"]["uri"].as_str(),
        Some(s.uri("src/main.rs").as_str())
    );
    assert_eq!(
        sent[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    assert_eq!(sent[0]["range"]["end"], json!({"line": 1, "character": 0}));

    // Two edits inside the wait: one request, at the newer version.
    type_in(&mut s, &editor, "a");
    wait(&mut s, Duration::from_millis(100));
    type_in(&mut s, &editor, "b");
    wait(&mut s, Duration::from_millis(100));
    assert_eq!(asked(&s).len(), 1);
    wait(&mut s, REST);
    s.fake.wait_for("the second request", |records| {
        records
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == METHOD))
            .count()
            == 2
    });
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "both edits", |r| {
        r.is_some_and(|r| r.text.contains("ab"))
    });
    s.until("the hints again", |s| shown(s, &editor) == expect(": ()"));

    // An answer for the older version is dropped: the first request is
    // still waiting when the second goes out.
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: hints("old").reply,
            delay: Duration::from_secs(1),
        },
    );
    type_in(&mut s, &editor, "c");
    wait(&mut s, REST + Duration::from_millis(10));
    s.fake.wait_for("the third request", |records| {
        records
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == METHOD))
            .count()
            == 3
    });
    s.fake.set_reply(
        METHOD,
        Scripted {
            reply: hints("new").reply,
            delay: Duration::from_secs(1),
        },
    );
    type_in(&mut s, &editor, "d");
    wait(&mut s, REST + Duration::from_millis(10));
    s.fake.wait_for("the fourth request", |records| {
        records
            .iter()
            .filter(|r| matches!(r, Record::Request { method, .. } if method == METHOD))
            .count()
            == 4
    });
    s.clock.advance(Duration::from_secs(1));
    s.until("the newer hints", |s| {
        shown(s, &editor).first().map(|h| h.1.as_str()) == Some("new")
    });
    assert_ne!(shown(&s, &editor)[0].1, "old");
}

fn set_hints_setting(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.inlay_hints = on;
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
    let (mut s, editor) = setup(cx, true);
    wait(&mut s, REST + Duration::from_millis(10));
    s.until("the hints", |s| shown(s, &editor) == expect(": ()"));

    // A refresh asks at once, and its answer replaces the hints.
    s.fake.set_reply(METHOD, hints("r"));
    let fake = s.fake.clone();
    let refresh = std::thread::spawn(move || fake.request::<InlayHintRefreshRequest>(()));
    s.until("the request after a refresh", |s| asked(s).len() == 2);
    s.until("the refreshed hints", |s| shown(s, &editor) == expect("r"));
    refresh.join().unwrap().ok();

    // Off, they're cleared and nothing is asked after edits.
    set_hints_setting(&mut s, false);
    assert_eq!(shown(&s, &editor), vec![]);
    type_in(&mut s, &editor, "a");
    wait(&mut s, REST + Duration::from_millis(10));
    type_in(&mut s, &editor, "b");
    wait(&mut s, REST + Duration::from_millis(10));
    assert_eq!(asked(&s).len(), 2);
    assert_eq!(shown(&s, &editor), vec![]);

    // On again, they're asked for again.
    set_hints_setting(&mut s, true);
    wait(&mut s, REST + Duration::from_millis(10));
    s.until("the request after turning on", |s| asked(s).len() == 3);
}

#[gpui::test]
fn a_server_without_a_provider_is_not_asked(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false);
    wait(&mut s, REST + Duration::from_millis(10));
    type_in(&mut s, &editor, "a");
    wait(&mut s, REST + Duration::from_millis(10));
    assert_eq!(asked(&s).len(), 0);
    assert_eq!(shown(&s, &editor), vec![]);
}

/// A fake whose server resolves hints, with its hint shown.
fn resolving(cx: &mut TestAppContext) -> (Lsp, Entity<EditorView>) {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        inlay_hint_provider: Some(OneOf::Right(
            lsp_types::InlayHintServerCapabilities::Options(InlayHintOptions {
                work_done_progress_options: Default::default(),
                resolve_provider: Some(true),
            }),
        )),
        ..init.capabilities
    };
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    let mut s = Lsp::with_fake_clock(cx, script, Script::default());
    s.fake.set_reply(
        METHOD,
        Scripted::now(Reply::Result(json!([
            {
                "position": { "line": 0, "character": 7 },
                "label": ": ()",
                "kind": 1,
                "data": { "id": 7 },
            },
        ]))),
    );
    s.fake.set_reply(
        "inlayHint/resolve",
        Scripted::now(Reply::Result(json!({
            "position": { "line": 0, "character": 7 },
            "label": ": ()",
            "kind": 1,
            "data": { "id": 7 },
            "tooltip": "the unit type",
            "textEdits": [
                { "range": {
                    "start": { "line": 0, "character": 7 },
                    "end": { "line": 0, "character": 7 },
                  }, "newText": ": ()" },
            ],
        }))),
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
    wait(&mut s, REST + Duration::from_millis(10));
    s.until("the hint", |s| !shown(s, &editor).is_empty());
    s.draw();

    (s, editor)
}

/// Double-clicks the first hint shown.
fn double_click_hint(s: &mut Lsp, editor: &Entity<EditorView>) {
    let at = editor.read_with(&s.cx, |view, _| view.inlay_hint_bounds())[0]
        .1
        .center();
    for click_count in [1, 2] {
        s.cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        s.cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
}

fn resolves(s: &Lsp) -> Vec<Value> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == "inlayHint/resolve" => {
                Some(params.clone())
            }
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn resolved_hints_insert_their_edits(cx: &mut TestAppContext) {
    let (mut s, editor) = resolving(cx);
    double_click_hint(&mut s, &editor);
    s.until("the resolved edit", |s| {
        editor.read_with(&s.cx, |view, _| {
            view.model().text().to_string().starts_with("fn main: ()()")
        })
    });
    let resolves = resolves(&s);
    assert_eq!(resolves.len(), 1);
    assert_eq!(resolves[0]["data"], json!({ "id": 7 }));
}

#[gpui::test]
fn a_new_manager_resolves_its_own_hints(cx: &mut TestAppContext) {
    let (mut s, editor) = resolving(cx);
    let root = s.root.clone();
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| workbench.update(cx, |w, cx| w.set_root(root, window, cx)));
    s.draw();
    // The old manager's hints go; the new server's come.
    assert!(shown(&s, &editor).is_empty());
    s.set_trust(true);
    wait(&mut s, REST + Duration::from_millis(10));
    s.until("the hint again", |s| !shown(s, &editor).is_empty());
    s.draw();
    double_click_hint(&mut s, &editor);
    s.until("the resolved edit", |s| {
        editor.read_with(&s.cx, |view, _| {
            view.model().text().to_string().starts_with("fn main: ()()")
        })
    });
    assert_eq!(resolves(&s).len(), 1);
}
