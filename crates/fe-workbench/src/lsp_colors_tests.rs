//! Document colours through the fake server: asked for when a buffer
//! opens and after an edit settles, handed to its editor as swatches, and
//! none asked for with `editor.color_decorators` off.

use std::{sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent};
use fe_gpui::{ColorSwatch, EditorView};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{ColorProviderCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const COLORS: &str = "textDocument/documentColor";
/// Row 0 has the colours.
const MAIN: &str = "// #ff0000 and #0000ff80\nfn main() {}\n";

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        color_provider: Some(ColorProviderCapability::Simple(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn start_of(needle: &str) -> usize {
    MAIN.find(needle).unwrap()
}

/// The LSP range of `needle`, on row 0, in UTF-8.
fn range_of(needle: &str) -> Value {
    let start = start_of(needle);
    json!({
        "start": { "line": 0, "character": start },
        "end": { "line": 0, "character": start + needle.len() },
    })
}

/// A colour over `needle` on row 0.
fn color(needle: &str, [red, green, blue, alpha]: [f32; 4]) -> Value {
    json!({
        "range": range_of(needle),
        "color": { "red": red, "green": green, "blue": blue, "alpha": alpha },
    })
}

const RED: [f32; 4] = [1., 0., 0., 1.];
const BLUE: [f32; 4] = [0., 0., 1., 0.5];

/// Row 0's two colours.
fn colors() -> Value {
    json!([color("#0000ff80", BLUE), color("#ff0000", RED)])
}

/// The swatches [`colors`] gives.
fn swatches() -> Vec<ColorSwatch> {
    vec![
        ColorSwatch {
            offset: start_of("#ff0000"),
            rgba: RED,
        },
        ColorSwatch {
            offset: start_of("#0000ff80"),
            rgba: BLUE,
        },
    ]
}

/// A trusted workbench showing `src/main.rs`, its editor focused, the
/// colours answered as [`colors`] gives them, and, if `on`, shown.
fn setup(cx: &mut TestAppContext, on: bool) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    s.fake
        .set_reply(COLORS, Scripted::now(Reply::Result(colors())));
    if !on {
        set_setting(&mut s, false);
    }
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
    if on {
        until_ticking(&mut s, "the swatches", |s| shown(s, &editor) == swatches());
    }
    (s, editor)
}

fn shown(s: &Lsp, editor: &Entity<EditorView>) -> Vec<ColorSwatch> {
    editor.read_with(&s.cx, |view, _| view.color_swatches())
}

fn set_setting(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.color_decorators = on;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

/// `until`, advancing the manager's manual clock 20 ms per poll.
fn until_ticking(s: &mut Lsp, what: &str, done: impl Fn(&mut Lsp) -> bool) {
    s.until(what, |s| {
        s.clock.advance(Duration::from_millis(20));
        done(s)
    });
}

/// The requests named `method`, with their ids.
fn requests(s: &Lsp, method: &str) -> Vec<(Value, Value)> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request {
                id,
                method: m,
                params,
            } if m == method => Some((json!(id), params.clone())),
            _ => None,
        })
        .collect()
}

/// Types a new row at the end of `main.rs`, after the colours.
fn type_at_end(s: &mut Lsp, editor: &Entity<EditorView>) {
    editor.update(&mut s.cx, |view, cx| {
        let end = view.model().text().len();
        view.model_mut().set_cursor(end);
        view.model_mut().insert("\n");
        cx.notify();
    });
    s.draw();
}

#[gpui::test]
fn colors_become_swatches(cx: &mut TestAppContext) {
    let (s, editor) = setup(cx, true);
    assert_eq!(shown(&s, &editor), swatches());
    let sent = requests(&s, COLORS);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].1["textDocument"]["uri"],
        s.uri("src/main.rs").as_str()
    );
}

#[gpui::test]
fn setting_off_asks_nothing(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false);
    for _ in 0..10 {
        s.clock.advance(Duration::from_millis(100));
        s.draw();
    }
    assert!(requests(&s, COLORS).is_empty());
    assert!(shown(&s, &editor).is_empty());

    // On, they're asked for; off again, they go.
    set_setting(&mut s, true);
    until_ticking(&mut s, "the swatches", |s| shown(s, &editor) == swatches());
    assert_eq!(requests(&s, COLORS).len(), 1);
    set_setting(&mut s, false);
    assert!(shown(&s, &editor).is_empty());
    // And an edit asks nothing.
    type_at_end(&mut s, &editor);
    s.cx.executor().advance_clock(Duration::from_millis(510));
    s.draw();
    assert_eq!(requests(&s, COLORS).len(), 1);
    assert!(shown(&s, &editor).is_empty());
}

#[gpui::test]
fn edit_asks_again_and_drops_stale_answers(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);

    // An edit, then a rest, asks again; the answer comes a second late.
    s.fake.set_reply(
        COLORS,
        Scripted {
            reply: Reply::Result(json!([color("#ff0000", [0., 1., 0., 1.])])),
            delay: Duration::from_secs(1),
        },
    );
    type_at_end(&mut s, &editor);
    // Not before the text has held still.
    s.cx.executor().advance_clock(Duration::from_millis(400));
    s.draw();
    assert_eq!(requests(&s, COLORS).len(), 1);
    s.cx.executor().advance_clock(Duration::from_millis(110));
    s.draw();
    s.until("the second ask", |s| requests(s, COLORS).len() == 2);
    // The swatches stay meanwhile, following the text.
    assert_eq!(shown(&s, &editor), swatches());

    // Typing again before it lands: the late answer is for text that's
    // gone, and isn't shown; the next ask's is.
    s.fake
        .set_reply(COLORS, Scripted::now(Reply::Result(json!([]))));
    type_at_end(&mut s, &editor);
    s.clock.advance(Duration::from_secs(1));
    s.draw();
    assert_eq!(shown(&s, &editor), swatches());
    let stale = requests(&s, COLORS)[1].0.clone();
    s.fake.wait_for("the stale ask cancelled", |records| {
        records.iter().any(|record| {
            matches!(record, Record::Notification { method, params }
                if method == "$/cancelRequest" && params["id"] == stale)
        })
    });
    s.cx.executor().advance_clock(Duration::from_millis(510));
    s.draw();
    s.until("the third ask", |s| requests(s, COLORS).len() == 3);
    // An empty answer clears them.
    until_ticking(&mut s, "no swatches", |s| shown(s, &editor).is_empty());
    assert_eq!(requests(&s, COLORS).len(), 3);
}

/// A server that keeps asking for a retry is given up on after the
/// retries, and the swatches clear (review px81xb F2).
#[gpui::test]
fn exhausted_retries_clear_the_swatches(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    s.fake.set_reply(
        COLORS,
        Scripted::now(Reply::Error {
            code: -32801,
            message: "content modified".into(),
        }),
    );
    type_at_end(&mut s, &editor);
    // The ask after the edit, then eight retries, each after a rest.
    for asked in 2..=10 {
        // The rest timer is made when the previous reply lands, which on a
        // loaded host is after any one advance: advance on every poll.
        until_ticking(&mut s, "the next ask", |s| {
            if requests(s, COLORS).len() < asked {
                s.cx.executor().advance_clock(Duration::from_millis(510));
                s.draw();
            }
            requests(s, COLORS).len() == asked
        });
        if asked < 10 {
            // Kept while it's retried.
            assert_eq!(shown(&s, &editor), swatches());
        }
    }
    until_ticking(&mut s, "no swatches", |s| {
        s.cx.executor().advance_clock(Duration::from_millis(510));
        shown(s, &editor).is_empty()
    });
    // And none asked for after.
    s.cx.executor().advance_clock(Duration::from_millis(510));
    s.draw();
    assert_eq!(requests(&s, COLORS).len(), 10);
}

#[gpui::test]
fn drawing_before_the_document_activates_asks_nothing(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    s.fake
        .set_reply(COLORS, Scripted::now(Reply::Result(colors())));
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), MAIN).unwrap();
    let manager = || {
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp.manager.clone())
            .expect("trusted, so there's a manager")
    };
    let manager = manager();
    crate::lsp::lock(&manager).hold_next_activation();
    s.open("src/main.rs");
    crate::lsp::lock(&manager).wait_activation_held();
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    for _ in 0..5 {
        s.clock.advance(Duration::from_millis(20));
        s.draw();
    }
    assert!(requests(&s, COLORS).is_empty());

    crate::lsp::lock(&manager).release_activation();
    until_ticking(&mut s, "the swatches", |s| shown(s, &editor) == swatches());
    assert_eq!(requests(&s, COLORS).len(), 1);
}
