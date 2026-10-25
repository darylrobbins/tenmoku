//! The Call Hierarchy panel over a fake server.

use std::time::Duration;

use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{CallHierarchyServerCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use super::{HierarchyPanel, Mode};
use crate::{DockPanel, tests::lsp::Lsp};

pub(super) const PREPARE: &str = "textDocument/prepareCallHierarchy";
pub(super) const INCOMING: &str = "callHierarchy/incomingCalls";
pub(super) const OUTGOING: &str = "callHierarchy/outgoingCalls";

/// `b` calls `a` twice; `c` calls `b`.
pub(super) const MAIN: &str = "fn a() {}\nfn b() { a(); a(); }\nfn c() { b(); }\n";

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

pub(super) fn range(line: u32, from: u32, to: u32) -> Value {
    json!({ "start": { "line": line, "character": from }, "end": { "line": line, "character": to } })
}

/// A function item named `name` on `line` of `src/z.rs`.
pub(super) fn item(s: &Lsp, name: &str, line: u32) -> Value {
    json!({
        "name": name,
        "kind": 12,
        "detail": "fn()",
        "uri": s.uri("src/z.rs").to_string(),
        "range": range(line, 0, 12),
        "selectionRange": range(line, 3, 4),
        "data": { "token": format!("{name}-data") },
    })
}

/// A call from (or to) `item`, at `sites`.
fn call(key: &str, item: Value, sites: &[Value]) -> Value {
    json!({ key: item, "fromRanges": sites })
}

pub(super) fn reply(s: &Lsp, method: &str, result: Value) {
    s.fake
        .set_reply(method, Scripted::now(Reply::Result(result)));
}

/// `src/z.rs` open and focused, its caret on `a`, with `a` the item at the
/// caret.
fn setup(cx: &mut TestAppContext, provider: bool) -> Lsp {
    let mut s = Lsp::with_fake_clock(cx, script(provider), Script::default());
    s.set_trust(true);
    std::fs::write(s.path("src/z.rs"), MAIN).unwrap();
    s.open("src/z.rs");
    let uri = s.uri("src/z.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("z.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    let a = item(&s, "a", 0);
    reply(&s, PREPARE, json!([a]));
    s
}

pub(super) fn panel(s: &Lsp) -> Entity<HierarchyPanel> {
    (s.workbench).read_with(&s.cx, |w, cx| {
        w.dock()
            .read(cx)
            .hierarchy()
            .cloned()
            .expect("the panel was made")
    })
}

pub(super) fn texts(s: &Lsp) -> Vec<String> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.texts())
}

pub(super) fn requests(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params.clone()),
            _ => None,
        })
        .collect()
}

/// The names of the items the requests of `method` were about.
pub(super) fn asked_about(s: &Lsp, method: &str) -> Vec<String> {
    (requests(s, method).iter())
        .map(|params| params["item"]["name"].as_str().unwrap().to_owned())
        .collect()
}

pub(super) fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

pub(super) fn press(s: &mut Lsp, key: &str) {
    let panel = panel(s);
    s.cx.update(|window, cx| panel.update(cx, |panel, cx| panel.press(key, window, cx)));
    s.draw();
}

pub(super) fn tick(s: &mut Lsp, by: Duration) {
    s.cx.executor().advance_clock(by);
    s.clock.advance(by);
    s.draw();
}

/// Shows the hierarchy and waits for the root's calls to land.
fn show(s: &mut Lsp, rows: usize) {
    s.run("nav.show_call_hierarchy");
    s.until("the rows", |s| {
        s.cx.executor().advance_clock(Duration::from_millis(20));
        s.workbench.read_with(&s.cx, |w, cx| {
            w.dock()
                .read(cx)
                .hierarchy()
                .is_some_and(|p| p.read(cx).texts().len() >= rows)
        })
    });
}

/// The active editor's primary selection, in bytes.
pub(super) fn selection(s: &Lsp) -> std::ops::Range<usize> {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    editor.read_with(&s.cx, |view, _| view.model().primary_selection().range)
}

#[gpui::test]
fn show_call_hierarchy_lists_the_item_at_the_caret_as_root(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    reply(&s, INCOMING, json!([]));
    show(&mut s, 2);
    assert_eq!(texts(&s), ["a", "  No calls"]);
    let panel = panel(&s);
    let title = panel.read_with(&s.cx, |panel, cx| panel.title(cx));
    assert_eq!(title.as_ref(), "Call Hierarchy: a");
    let (visible, active) = (s.workbench).read_with(&s.cx, |w, cx| {
        let dock = w.dock().read(cx);
        (dock.is_visible(), dock.active())
    });
    assert!(visible);
    assert_eq!(active, DockPanel::CallHierarchy);
    assert_eq!(requests(&s, PREPARE).len(), 1);
    assert_eq!(asked_about(&s, INCOMING), ["a"]);
}

#[gpui::test]
fn expanding_loads_incoming_calls_lazily(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(
        &s,
        INCOMING,
        json!([call("from", b, &[range(1, 9, 10), range(1, 14, 15)])]),
    );
    show(&mut s, 2);
    assert_eq!(texts(&s), ["a", "  b ×2"]);
    // Only the root's calls were asked for.
    assert_eq!(asked_about(&s, INCOMING), ["a"]);

    let c = item(&s, "c", 2);
    reply(&s, INCOMING, json!([call("from", c, &[range(2, 9, 10)])]));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("c", |s| texts(s).len() == 3);
    assert_eq!(texts(&s), ["a", "  b ×2", "    c"]);
    // Only `b`'s, none for `c`.
    assert_eq!(asked_about(&s, INCOMING), ["a", "b"]);
    assert!(requests(&s, OUTGOING).is_empty());
}

#[gpui::test]
fn toggling_direction_asks_outgoing_calls(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(&s, INCOMING, json!([call("from", b, &[range(1, 9, 10)])]));
    show(&mut s, 2);
    let panel = panel(&s);
    assert_eq!(
        panel.read_with(&s.cx, |panel, _| panel.mode()),
        Mode::Incoming
    );

    let x = item(&s, "x", 2);
    reply(&s, OUTGOING, json!([call("to", x, &[range(0, 3, 4)])]));
    s.run("hierarchy.toggle_direction");
    s.until("outgoing", |s| texts(s) == ["a", "  x"]);
    assert_eq!(
        panel.read_with(&s.cx, |panel, _| panel.mode()),
        Mode::Outgoing
    );
    assert_eq!(asked_about(&s, OUTGOING), ["a"]);
    assert_eq!(asked_about(&s, INCOMING), ["a"]);

    s.run("hierarchy.toggle_direction");
    s.until("incoming again", |s| texts(s) == ["a", "  b"]);
    assert_eq!(asked_about(&s, INCOMING), ["a", "a"]);
}

#[gpui::test]
fn a_recursive_call_is_marked_and_not_expandable(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let a = item(&s, "a", 0);
    reply(&s, INCOMING, json!([call("from", a, &[range(0, 3, 4)])]));
    show(&mut s, 2);
    assert_eq!(texts(&s), ["a", "  a (recursive)"]);
    press(&mut s, "down");
    press(&mut s, "right");
    assert_eq!(texts(&s), ["a", "  a (recursive)"]);
    assert_eq!(asked_about(&s, INCOMING), ["a"]);
    let panel = panel(&s);
    assert!(!panel.read_with(&s.cx, |panel, _| panel.is_expanded(&[0])));
}

#[gpui::test]
fn enter_opens_the_callers_call_site(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(
        &s,
        INCOMING,
        json!([call("from", b, &[range(1, 9, 10), range(1, 14, 15)])]),
    );
    show(&mut s, 2);
    let line = MAIN.find("fn b").unwrap();
    // Real key events: the panel keeps focus through the opens.
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    s.until("the first call site", |s| {
        selection(s) == (line + 9..line + 10)
    });
    assert!(panel_focused(&mut s));
    // Enter again goes to the next site.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    s.until("the second call site", |s| {
        selection(s) == (line + 14..line + 15)
    });

    assert!(panel_focused(&mut s));
    // The root opens its own selection.
    s.cx.simulate_keystrokes("up enter");
    s.draw();
    s.until("the root's name", |s| selection(s) == (3..4));
}

#[gpui::test]
fn no_provider_toasts(cx: &mut TestAppContext) {
    let mut s = setup(cx, false);
    s.run("nav.show_call_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No call hierarchy available here"]);
    assert!(requests(&s, PREPARE).is_empty());
}

#[gpui::test]
fn no_item_toasts(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    reply(&s, PREPARE, json!([]));
    s.run("nav.show_call_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No call hierarchy available here"]);
    let shown = (s.workbench).read_with(&s.cx, |w, cx| w.dock().read(cx).active());
    assert_ne!(shown, DockPanel::CallHierarchy);
}

#[gpui::test]
fn empty_and_error_rows(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    // The root's request is never answered: "Loading…".
    s.fake.set_reply(INCOMING, Scripted::now(Reply::Never));
    s.run("nav.show_call_hierarchy");
    s.until("loading", |s| {
        s.workbench.read_with(&s.cx, |w, cx| {
            w.dock()
                .read(cx)
                .hierarchy()
                .is_some_and(|p| p.read(cx).texts() == ["a", "  Loading…"])
        })
    });

    // Reloading from the root: nothing out of it, then one caller.
    reply(&s, OUTGOING, json!([]));
    s.run("hierarchy.toggle_direction");
    s.until("no calls", |s| texts(s) == ["a", "  No calls"]);
    reply(&s, INCOMING, json!([call("from", b, &[range(1, 9, 10)])]));
    s.run("hierarchy.toggle_direction");
    s.until("b", |s| texts(s) == ["a", "  b"]);
    // The caller has no callers.
    reply(&s, INCOMING, json!([]));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("no calls under b", |s| texts(s).len() == 3);
    assert_eq!(texts(&s), ["a", "  b", "    No calls"]);

    // An error is shown as the row's message.
    s.fake.set_reply(
        INCOMING,
        Scripted::now(Reply::Error {
            code: -32603,
            message: "boom".into(),
        }),
    );
    press(&mut s, "left");
    press(&mut s, "up");
    s.run("hierarchy.toggle_direction");
    s.run("hierarchy.toggle_direction");
    s.until("the error", |s| {
        let rows = texts(s);
        rows.len() == 2 && rows[1].contains("boom")
    });
}

#[gpui::test]
fn a_newer_request_cancels_the_older_one(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    reply(&s, INCOMING, json!([]));
    let old = item(&s, "old", 1);
    s.fake.set_reply(
        PREPARE,
        Scripted {
            reply: Reply::Result(json!([old])),
            delay: Duration::from_millis(500),
        },
    );
    s.run("nav.show_call_hierarchy");
    let a = item(&s, "a", 0);
    reply(&s, PREPARE, json!([a]));
    s.run("nav.show_call_hierarchy");
    s.until("the newer", |s| {
        s.workbench.read_with(&s.cx, |w, cx| {
            w.dock()
                .read(cx)
                .hierarchy()
                .is_some_and(|p| p.read(cx).root_name() == Some("a"))
        })
    });
    // The older one's answer, once it comes, doesn't replace it.
    tick(&mut s, Duration::from_secs(1));
    tick(&mut s, Duration::from_secs(1));
    let panel = panel(&s);
    assert_eq!(
        panel.read_with(&s.cx, |p, _| p.root_name().map(str::to_owned)),
        Some("a".into())
    );
    assert_eq!(asked_about(&s, INCOMING), ["a"]);
}

#[gpui::test]
fn panel_is_on_the_rail_after_first_use(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let on_rail = |s: &Lsp| {
        s.workbench.read_with(&s.cx, |w, cx| {
            w.dock()
                .read(cx)
                .state()
                .on_rail(DockPanel::CallHierarchy.id(), cx)
        })
    };
    assert!(!on_rail(&s));
    reply(&s, INCOMING, json!([]));
    show(&mut s, 2);
    assert!(on_rail(&s));
}

pub(super) fn panel_focused(s: &mut Lsp) -> bool {
    let panel = panel(s);
    s.cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn a_new_ask_drops_the_old_tree_and_its_loading_rows(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(&s, INCOMING, json!([call("from", b, &[range(1, 9, 10)])]));
    show(&mut s, 2);
    // The next ask finds nothing: no stale tree, no row stuck loading.
    reply(&s, PREPARE, json!([]));
    s.run("nav.show_call_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(texts(&s), Vec::<String>::new());
}

#[gpui::test]
fn an_ask_without_a_server_cancels_a_delayed_prepare(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    reply(&s, INCOMING, json!([]));
    let old = item(&s, "old", 1);
    s.fake.set_reply(
        PREPARE,
        Scripted {
            reply: Reply::Result(json!([old])),
            delay: Duration::from_millis(500),
        },
    );
    s.run("nav.show_call_hierarchy");
    // A file no server has.
    std::fs::write(s.path("notes.txt"), "text\n").unwrap();
    s.open("notes.txt");
    s.run("nav.show_call_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["No language server for this file"]);
    tick(&mut s, Duration::from_secs(1));
    tick(&mut s, Duration::from_secs(1));
    let shown = (s.workbench).read_with(&s.cx, |w, cx| w.dock().read(cx).active());
    assert_ne!(shown, DockPanel::CallHierarchy);
    assert_eq!(texts(&s), Vec::<String>::new());
}

#[gpui::test]
fn right_stays_on_a_node_with_no_calls_to_enter(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let (b, c) = (item(&s, "b", 1), item(&s, "c", 2));
    reply(
        &s,
        INCOMING,
        json!([
            call("from", b, &[range(1, 9, 10)]),
            call("from", c, &[range(2, 9, 10)])
        ]),
    );
    show(&mut s, 3);
    s.fake.set_reply(INCOMING, Scripted::now(Reply::Never));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("loading", |s| texts(s).len() == 4);
    // Loading: Right doesn't jump to the sibling.
    press(&mut s, "right");
    let panel = panel(&s);
    assert_eq!(
        panel.read_with(&s.cx, |p, _| p.selected_path().to_vec()),
        [0]
    );
}

#[gpui::test]
fn opening_a_caller_in_an_unopened_file_keeps_the_panel_focused(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    std::fs::write(s.path("src/y.rs"), "fn y() { a(); }\n").unwrap();
    let mut y = item(&s, "y", 0);
    y["uri"] = json!(s.uri("src/y.rs").to_string());
    reply(&s, INCOMING, json!([call("from", y, &[range(0, 9, 10)])]));
    show(&mut s, 2);
    s.cx.simulate_keystrokes("down enter");
    s.draw();
    s.until("y.rs shown at the call", |s| {
        let editor = (s.workbench).read_with(&s.cx, |w, cx| w.active_editor(cx).cloned());
        editor.is_some_and(|editor| {
            editor.read_with(&s.cx, |view, _| {
                view.model().path().is_some_and(|p| p.ends_with("y.rs"))
                    && view.model().primary_selection().range == (9..10)
            })
        })
    });
    // The load landed, and the panel still has the keys.
    assert!(panel_focused(&mut s));
    s.cx.simulate_keystrokes("up");
    s.draw();
    let panel = panel(&s);
    assert!(panel.read_with(&s.cx, |p, _| p.selected_path().is_empty()));
}

#[gpui::test]
fn a_cancelled_expansion_can_be_retried(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let b = item(&s, "b", 1);
    reply(&s, INCOMING, json!([call("from", b, &[range(1, 9, 10)])]));
    show(&mut s, 2);
    s.fake.set_reply(INCOMING, Scripted::now(Reply::Never));
    press(&mut s, "down");
    press(&mut s, "right");
    s.until("loading", |s| texts(s).len() == 3);
    // An ask with no server drops that request.
    std::fs::write(s.path("notes.txt"), "text\n").unwrap();
    s.open("notes.txt");
    s.run("nav.show_call_hierarchy");
    s.until("the toast", |s| !toasts(s).is_empty());
    // No row is left loading; expanding again asks again.
    assert_eq!(texts(&s).len(), 2);
    reply(&s, INCOMING, json!([]));
    press(&mut s, "left");
    press(&mut s, "right");
    s.until("asked again", |s| asked_about(s, INCOMING).len() >= 3);
}
