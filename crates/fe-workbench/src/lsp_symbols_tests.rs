//! The outline and breadcrumbs from the fake server's document symbols:
//! its symbols in place of tree-sitter's, asked on open and once edits have
//! rested, tree-sitter's again without them, and each editor's own.

use std::{fs, sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent, settings::LspServerSettings};
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext};
use lsp_types::{OneOf, ServerCapabilities};
use serde_json::json;

use crate::{OutlinePanel, tests::lsp::Lsp};

const METHOD: &str = "textDocument/documentSymbol";
const REST: Duration = Duration::from_millis(500);

/// Tree-sitter's outline: `outer`, and `inner` in it.
const TEXT: &str = "mod outer {\n    fn inner() {}\n}\n";

fn range(line: u32, from: u32, to: u32) -> serde_json::Value {
    json!({
        "start": { "line": line, "character": from },
        "end": { "line": line, "character": to },
    })
}

/// The server's symbols: `Served` (a module, on the whole of TEXT) with
/// `served_child` (a function, on the second row) in it.
fn symbols(name: &str) -> serde_json::Value {
    json!([{
        "name": name,
        "detail": "d",
        "kind": 2,
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 3, "character": 0 } },
        "selectionRange": range(0, 4, 9),
        "children": [{
            "name": format!("{name}_child"),
            "kind": 12,
            "range": range(1, 4, 17),
            "selectionRange": range(1, 7, 12),
        }],
    }])
}

fn script(provider: bool) -> Script {
    let mut init = initialize_result(Some(lsp_types::PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_symbol_provider: provider.then_some(OneOf::Left(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn reply(s: &Lsp, reply: Reply, delay: Duration) {
    s.fake.set_reply(METHOD, Scripted { reply, delay });
}

/// The workbench with `src/main.rs` (TEXT) open on its server and focused,
/// the outline shown, `reply` answering.
fn setup(cx: &mut TestAppContext, provider: bool, answer: Reply) -> Lsp {
    let mut s = Lsp::with_fake_clock(cx, script(provider), Script::default());
    reply(&s, answer, Duration::ZERO);
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), TEXT).unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = editor(&mut s);
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.run("outline.show");
    if provider {
        await_requests(&mut s, 1);
    }
    s
}

fn editor(s: &mut Lsp) -> Entity<EditorView> {
    (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn panel(s: &mut Lsp) -> Entity<OutlinePanel> {
    (s.workbench).read_with(&s.cx, |w, cx| w.dock().read(cx).outline().clone())
}

fn rows(s: &mut Lsp) -> Vec<(u32, String)> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.rows())
}

fn message(s: &mut Lsp) -> Option<String> {
    let panel = panel(s);
    panel.read_with(&s.cx, |panel, _| panel.message_text())
}

fn served(depths: &[(u32, &str)]) -> Vec<(u32, String)> {
    depths.iter().map(|&(d, n)| (d, n.to_owned())).collect()
}

fn tree_sitter() -> Vec<(u32, String)> {
    served(&[(0, "outer"), (1, "inner")])
}

/// The names of the symbols the active pane's breadcrumbs show.
fn crumbs(s: &mut Lsp) -> Vec<String> {
    (s.workbench).read_with(&s.cx, |w, cx| {
        (w.pane().read(cx).toolbar().symbols.items.iter())
            .map(|(_, name, _)| name.to_string())
            .collect()
    })
}

fn move_caret(s: &mut Lsp, offset: usize) {
    let editor = editor(s);
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(offset);
            cx.notify();
        })
    });
    s.draw();
}

fn requests(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|r| matches!(r, Record::Request { method, .. } if method == METHOD))
        .count()
}

/// Draws until `n` requests have been sent. A request waits for a server
/// still initializing on the manager's manual clock (a 250 ms backstop), so
/// the clock moves on meanwhile, though not far enough to time one out.
fn await_requests(s: &mut Lsp, n: usize) {
    for _ in 0..8 {
        s.draw();
        if requests(s) >= n {
            return;
        }
        s.clock.advance(Duration::from_millis(250));
    }
    s.until("the request", |s| requests(s) >= n);
}

fn rest(s: &mut Lsp) {
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
}

#[gpui::test]
fn server_symbols_replace_the_tree_sitter_outline(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Served")));
    s.until("the served outline", |s| rows(s)[0].1 == "Served");
    assert_eq!(rows(&mut s), served(&[(0, "Served"), (1, "Served_child")]));
    assert_eq!(message(&mut s), None);
    let panel = panel(&mut s);
    panel.read_with(&s.cx, |panel, _| {
        let outline = panel.outline().unwrap();
        assert_eq!(outline.items[0].detail, "d");
        assert_eq!(outline.items[0].name_range, 4..9);
        assert_eq!(outline.items[1].row, 1);
        assert_eq!(outline.items[1].parent, Some(0));
        assert!(!outline.truncated);
    });
}

#[gpui::test]
fn breadcrumbs_follow_the_served_outline(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Served")));
    s.until("the served outline", |s| rows(s)[0].1 == "Served");
    // On `inner`'s body.
    move_caret(&mut s, 18);
    s.until("the served crumbs", |s| {
        crumbs(s) == ["Served", "Served_child"]
    });
}

#[gpui::test]
fn clicking_a_served_row_selects_its_name_range(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Served")));
    s.until("the served outline", |s| rows(s)[0].1 == "Served");
    let panel = panel(&mut s);
    let bounds = panel.read_with(&s.cx, |panel, _| panel.row_bounds(1).expect("drawn"));
    s.cx.simulate_click(
        bounds.origin + gpui::point(gpui::px(30.), bounds.size.height / 2.),
        Modifiers::none(),
    );
    let editor = editor(&mut s);
    let selected = editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    assert_eq!(selected, 19..19);
}

#[gpui::test]
fn no_provider_keeps_the_tree_sitter_outline(cx: &mut TestAppContext) {
    let mut s = setup(cx, false, Reply::Result(symbols("Served")));
    s.until("the outline", |s| rows(s) == tree_sitter());
    assert_eq!(requests(&s), 0);
}

#[gpui::test]
fn empty_reply_falls_back(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(json!([])));
    s.until("the request", |s| requests(s) == 1);
    s.until("the outline", |s| rows(s) == tree_sitter());

    // An error does too, and a served outline gives way to it.
    reply(&s, Reply::Result(symbols("Served")), Duration::ZERO);
    let editor = editor(&mut s);
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert(" ")));
    s.draw();
    rest(&mut s);
    s.until("the served outline", |s| rows(s)[0].1 == "Served");
    reply(
        &s,
        Reply::Error {
            code: -32603,
            message: "boom".into(),
        },
        Duration::ZERO,
    );
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert(" ")));
    s.draw();
    rest(&mut s);
    s.until("tree-sitter again", |s| rows(s) == tree_sitter());
}

#[gpui::test]
fn language_without_an_outline_query_shows_served_symbols(cx: &mut TestAppContext) {
    assert!(fe_syntax::language_by_name("html").is_some());
    let mut s = setup(cx, true, Reply::Result(symbols("Served")));
    let file = "[lsp.servers.rust-analyzer]\nlanguages = [\"rust\", \"html\"]\n";
    s.write_user_settings(file);
    let mut settings = Settings::default();
    settings.lsp.servers.insert(
        "rust-analyzer".to_owned(),
        LspServerSettings {
            languages: Some(vec!["rust".into(), "html".into()]),
            ..LspServerSettings::default()
        },
    );
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    fs::write(s.path("page.html"), TEXT).unwrap();
    s.open("page.html");
    let page = s.uri("page.html");
    s.fake
        .wait_for_document(&page, "didOpen", |r| r.is_some_and(|r| r.open));
    s.until("the served outline", |s| {
        rows(s).first().is_some_and(|r| r.1 == "Served")
    });
    assert_eq!(message(&mut s), None);
}

#[gpui::test]
fn edit_asks_again_after_the_rest_and_drops_stale_answers(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("First")));
    s.until("the first", |s| rows(s)[0].1 == "First");
    assert_eq!(requests(&s), 1);

    // An edit asks only once the buffer has rested, and the answer to an
    // older version is dropped.
    reply(&s, Reply::Result(symbols("Slow")), Duration::from_secs(1));
    let editor = editor(&mut s);
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert(" ")));
    s.draw();
    assert_eq!(requests(&s), 1);
    rest(&mut s);
    s.until("the second request", |s| requests(s) == 2);
    s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert(" ")));
    s.draw();
    reply(&s, Reply::Result(symbols("Last")), Duration::ZERO);
    rest(&mut s);
    s.until("the last", |s| rows(s)[0].1 == "Last");
    s.clock.advance(Duration::from_secs(1));
    s.draw();
    assert_eq!(rows(&mut s)[0].1, "Last");
}

#[gpui::test]
fn switching_editors_shows_each_editors_own_outline(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Main")));
    s.until("main's", |s| rows(s)[0].1 == "Main");
    reply(&s, Reply::Result(symbols("Lib")), Duration::ZERO);
    fs::write(s.path("src/lib.rs"), TEXT).unwrap();
    s.open("src/lib.rs");
    s.until("lib's", |s| rows(s).first().is_some_and(|r| r.1 == "Lib"));
    s.open("src/main.rs");
    assert_eq!(rows(&mut s)[0].1, "Main");
}

#[gpui::test]
fn edits_while_the_opening_answer_is_pending_wait_for_the_rest(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(true), Script::default());
    reply(&s, Reply::Result(symbols("Slow")), Duration::from_secs(5));
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), TEXT).unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    await_requests(&mut s, 1);

    // Each edit restarts the wait, though the opening answer hasn't come.
    let editor = editor(&mut s);
    for _ in 0..3 {
        s.cx.update(|_, cx| editor.update(cx, |view, _| view.model_mut().insert(" ")));
        s.draw();
        s.cx.executor().advance_clock(Duration::from_millis(300));
        s.draw();
    }
    assert_eq!(requests(&s), 1);
    rest(&mut s);
    s.until("the request after the rest", |s| requests(s) == 2);
}

#[gpui::test]
fn a_restarted_server_is_asked_again_without_an_edit(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Before")));
    s.until("the first", |s| rows(s)[0].1 == "Before");
    reply(&s, Reply::Result(symbols("After")), Duration::ZERO);
    s.run("lsp.restart");
    await_requests(&mut s, 2);
    s.until("the new server's", |s| {
        rows(s).first().is_some_and(|r| r.1 == "After")
    });
}

#[gpui::test]
fn losing_the_server_goes_back_to_tree_sitter(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Served")));
    s.until("the served outline", |s| rows(s)[0].1 == "Served");
    s.set_trust(false);
    s.until("tree-sitter again", |s| rows(s) == tree_sitter());
    // And trusted again, the server is asked afresh.
    s.set_trust(true);
    await_requests(&mut s, 2);
    s.until("the served outline again", |s| {
        rows(s).first().is_some_and(|r| r.1 == "Served")
    });
}

#[gpui::test]
fn crash_recovery_under_the_same_server_asks_again_without_an_edit(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Before")));
    s.until("the first", |s| rows(s)[0].1 == "Before");
    let server = s.active_server();

    // The crashed server isn't served from while it awaits recovery.
    reply(&s, Reply::Result(symbols("After")), Duration::ZERO);
    s.fake.crash_at(1);
    s.until("tree-sitter while down", |s| rows(s) == tree_sitter());
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.fake.wait_initialized_at(2);
    await_requests(&mut s, 2);
    s.until("the recovered server's", |s| {
        rows(s).first().is_some_and(|r| r.1 == "After")
    });
    assert_eq!(s.active_server(), server, "the same ServerId");
}

#[gpui::test]
fn closing_and_reopening_the_document_asks_again_without_an_edit(cx: &mut TestAppContext) {
    let mut s = setup(cx, true, Reply::Result(symbols("Before")));
    s.until("the first", |s| rows(s)[0].1 == "Before");
    let main = s.uri("src/main.rs");
    let path = s.path("src/main.rs");
    let root = s.root.clone();
    let editor = editor(&mut s);
    let manager = s
        .workbench
        .read_with(&s.cx, |w, _| w.lsp.manager().unwrap());

    // The document is closed and opened again on the same server within one
    // frame, so no frame sees it closed.
    reply(&s, Reply::Result(symbols("After")), Duration::ZERO);
    s.cx.update(|_, cx| {
        let (buffer, synced, text, version) = editor.update(cx, |view, _| {
            let mut buffer = view.model_mut().buffer_mut();
            (
                buffer.id(),
                buffer.subscribe(),
                buffer.text().clone(),
                buffer.version(),
            )
        });
        let mut manager = manager.lock().unwrap();
        manager.close(buffer);
        manager
            .open(Some(&root), buffer, text, version, synced, &path, "rust")
            .unwrap();
    });
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    await_requests(&mut s, 2);
    s.until("the reopened document's", |s| {
        rows(s).first().is_some_and(|r| r.1 == "After")
    });
}
