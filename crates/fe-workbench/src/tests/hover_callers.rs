//! The hover card's footer shows how many call the symbol, from the call
//! hierarchy, filled in when it arrives and not before the hover is shown.

use std::time::Duration;

use fe_gpui::{HoverCardPart, HoverCardText, PaintedHoverCard};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Focusable as _, TestAppContext};
use lsp_types::{CallHierarchyServerCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use super::lsp::Lsp;

const HOVER: &str = "textDocument/hover";
const PREPARE: &str = "textDocument/prepareCallHierarchy";
const INCOMING: &str = "callHierarchy/incomingCalls";

fn script(provider: bool) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities = ServerCapabilities {
        call_hierarchy_provider: provider.then_some(CallHierarchyServerCapability::Simple(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn range(line: u32, from: u32, to: u32) -> Value {
    json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } })
}

fn item(s: &Lsp, name: &str, line: u32) -> Value {
    json!({
        "name": name, "kind": 12,
        "uri": s.uri("src/z.rs").to_string(),
        "range": range(line, 0, 12), "selectionRange": range(line, 3, 4),
    })
}

fn callers(s: &Lsp, n: u32) -> Value {
    Value::Array(
        (0..n)
            .map(
                |i| json!({ "from": item(s, &format!("c{i}"), i), "fromRanges": [range(i, 0, 1)] }),
            )
            .collect(),
    )
}

/// `src/z.rs` open and focused, the caret in `a`, the server hovering.
fn setup(cx: &mut TestAppContext, provider: bool) -> (Lsp, gpui::Entity<fe_gpui::EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(provider), Script::default());
    s.set_trust(true);
    std::fs::write(s.path("src/z.rs"), "fn a() {}\nfn b() { a(); }\n").unwrap();
    s.open("src/z.rs");
    let uri = s.uri("src/z.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("z.rs is shown");
    s.cx.update(|window, cx| {
        PaintedHoverCard::set(cx);
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn ".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    let hover = json!({ "contents": { "kind": "markdown", "value": "```rust\nfn a()\n```\nA." } });
    s.fake.set_reply(HOVER, Scripted::now(Reply::Result(hover)));
    let a = item(&s, "a", 0);
    s.fake
        .set_reply(PREPARE, Scripted::now(Reply::Result(json!([a]))));
    (s, editor)
}

fn show(s: &mut Lsp) {
    s.cx.simulate_keystrokes("cmd-k cmd-i");
    s.draw();
}

/// The footer's texts in a fresh frame.
fn footer(s: &mut Lsp, editor: &gpui::Entity<fe_gpui::EditorView>) -> Vec<String> {
    s.cx.update(|_, cx| PaintedHoverCard::take(cx));
    editor.update(&mut s.cx, |_, cx| cx.notify());
    s.draw();
    let texts: Vec<HoverCardText> = s.cx.update(|_, cx| PaintedHoverCard::take(cx));
    (texts.into_iter())
        .filter(|text| text.part == HoverCardPart::Footer)
        .map(|text| text.text)
        .collect()
}

fn requests(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method: m, .. } if m == method))
        .count()
}

fn callers_text(count: usize, shown: &str) -> String {
    kiln_l10n::tr!("gpui-hover-callers", count = count, shown = shown).to_string()
}

#[gpui::test]
fn the_footer_fills_late_without_moving_the_card(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    s.fake.set_reply(
        INCOMING,
        Scripted {
            reply: Reply::Result(callers(&s, 3)),
            delay: Duration::from_secs(1),
        },
    );
    show(&mut s);
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });
    let fake = s.fake.clone();
    fake.wait_for("the incoming calls", |records| {
        records
            .iter()
            .any(|r| matches!(r, Record::Request { method, .. } if method == INCOMING))
    });
    // The card is up without a count.
    assert!(!footer(&mut s, &editor).iter().any(|t| t.contains("caller")));
    // Its room is held: the footer is there, empty of text.
    let before = s.cx.debug_bounds("hover-popover").expect("the card");
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.until("the count", |s| {
        footer(s, &editor).contains(&callers_text(3, "3"))
    });
    let after = s.cx.debug_bounds("hover-popover").expect("the card");
    // Where it is and how tall it is stay; it only ever narrows to the text.
    assert_eq!(before.origin, after.origin);
    assert_eq!(before.size.height, after.size.height);
    assert!(after.size.width <= before.size.width);
    // Counted for the symbol the hover is about.
    assert_eq!(requests(&s, PREPARE), 1);
}

/// The card at a window edge, its count arriving late (or never): it's
/// drawn at the same place before and after.
fn stays_put_at_the_edge(cx: &mut TestAppContext, width: f32, height: f32, none: bool) {
    let (mut s, editor) = setup(cx, true);
    s.cx.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
    s.draw();
    if none {
        // A symbol with no call hierarchy item: the count is none.
        s.fake
            .set_reply(PREPARE, Scripted::now(Reply::Result(json!([]))));
    } else {
        s.fake.set_reply(
            INCOMING,
            Scripted {
                reply: Reply::Result(callers(&s, 2)),
                delay: Duration::from_secs(1),
            },
        );
    }
    show(&mut s);
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });
    s.draw();
    let before = s.cx.debug_bounds("hover-popover").expect("the card");
    if none {
        s.until("the footer to go", |s| {
            s.draw();
            !footer(s, &editor)
                .iter()
                .any(|text| text.contains("caller"))
                && requests(s, PREPARE) == 1
        });
    } else {
        s.clock.wait_for_sleep(Duration::from_secs(1));
        s.clock.advance(Duration::from_secs(1));
        s.until("the count", |s| {
            footer(s, &editor).contains(&callers_text(2, "2"))
        });
    }
    s.draw();
    let after = s.cx.debug_bounds("hover-popover").expect("the card");
    assert_eq!(before.origin, after.origin);
    assert!(after.size.height <= before.size.height);
}

#[gpui::test]
fn a_late_count_leaves_a_card_at_the_bottom_edge_where_it_was(cx: &mut TestAppContext) {
    stays_put_at_the_edge(cx, 900., 130., false);
}

#[gpui::test]
fn a_late_count_leaves_a_card_at_the_right_edge_where_it_was(cx: &mut TestAppContext) {
    stays_put_at_the_edge(cx, 440., 700., false);
}

#[gpui::test]
fn no_count_leaves_a_card_at_the_edges_where_it_was(cx: &mut TestAppContext) {
    stays_put_at_the_edge(cx, 440., 130., true);
}

#[gpui::test]
fn no_count_without_a_provider(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false);
    show(&mut s);
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });
    s.draw();
    assert!(!footer(&mut s, &editor).iter().any(|t| t.contains("caller")));
    assert_eq!(requests(&s, PREPARE), 0);
    assert_eq!(requests(&s, INCOMING), 0);
}

/// Rests the pointer on the `a` of `fn a`, until the card asks the server.
fn rest_on_the_symbol(s: &mut Lsp, editor: &gpui::Entity<fe_gpui::EditorView>) {
    let caret = editor
        .update(&mut s.cx, |view, _| view.caret_bounds_at("fn ".len()))
        .expect("the caret is laid out");
    s.cx.simulate_mouse_move(
        gpui::point(caret.left() + gpui::px(1.), caret.center().y),
        None,
        gpui::Modifiers::none(),
    );
    s.cx.executor().advance_clock(Duration::from_millis(400));
    s.draw();
}

/// The pointer goes where neither the symbol nor the card is, and the
/// hiding delay passes.
fn leave(s: &mut Lsp) {
    s.cx.simulate_mouse_move(
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        gpui::Modifiers::none(),
    );
    s.cx.executor().advance_clock(Duration::from_millis(600));
    s.draw();
}

fn cancels(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| record.is_notification("$/cancelRequest"))
        .count()
}

#[gpui::test]
fn the_request_is_cancelled_when_the_pointer_leaves(cx: &mut TestAppContext) {
    // While the incoming calls are being counted.
    let (mut s, editor) = setup(cx, true);
    s.fake.set_reply(INCOMING, Scripted::now(Reply::Never));
    rest_on_the_symbol(&mut s, &editor);
    s.until("the incoming calls", |s| {
        s.draw();
        requests(s, INCOMING) == 1
    });
    assert_eq!(cancels(&s), 0);
    leave(&mut s);
    s.until("$/cancelRequest", |s| {
        s.draw();
        cancels(s) == 1
    });
    assert_eq!(editor.read_with(&s.cx, |view, _| view.popover_text()), None);
}

#[gpui::test]
fn leaving_while_the_hierarchy_is_prepared_cancels_it(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    s.fake.set_reply(PREPARE, Scripted::now(Reply::Never));
    rest_on_the_symbol(&mut s, &editor);
    s.until("the prepare request", |s| {
        s.draw();
        requests(s, PREPARE) == 1
    });
    leave(&mut s);
    s.until("$/cancelRequest", |s| {
        s.draw();
        cancels(s) == 1
    });
    assert_eq!(requests(&s, INCOMING), 0);
}

#[gpui::test]
fn one_caller_reads_singular(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    s.fake
        .set_reply(INCOMING, Scripted::now(Reply::Result(callers(&s, 1))));
    show(&mut s);
    s.until("the count", |s| {
        footer(s, &editor).contains(&callers_text(1, "1"))
    });
    let one = callers_text(1, "1");
    assert_eq!(one, "1 caller");
    assert_ne!(one, callers_text(2, "2"));
}
