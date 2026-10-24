//! Folding through the fake server: asked for when a buffer opens and after
//! edits settle, the server's ranges replacing the tree's folds, and the
//! tree's folds kept for a server without the provider.

use std::time::Duration;

use fe_command::Invocation;
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{FoldingRangeProviderCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const FOLDING: &str = "textDocument/foldingRange";
const REST: Duration = Duration::from_millis(400);
/// The syntax folds rows 0..=2 and 4..=5.
const MAIN: &str = "fn a() {\n    one();\n    two();\n}\nfn b() {\n    x();\n}\n";

fn script(provider: bool) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        folding_range_provider: provider.then_some(FoldingRangeProviderCapability::Simple(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn reply(ranges: Value) -> Scripted {
    Scripted::now(Reply::Result(ranges))
}

fn setup(cx: &mut TestAppContext, provider: bool) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(provider), Script::default());
    s.fake.set_reply(
        FOLDING,
        reply(json!([{ "startLine": 1, "endLine": 3 }, { "startLine": 4, "endLine": 4 }])),
    );
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

fn requests(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method, .. } if method == FOLDING))
        .count()
}

/// `until`, advancing the manager's manual clock 20 ms per poll.
fn until_ticking(s: &mut Lsp, what: &str, done: impl Fn(&mut Lsp) -> bool) {
    s.until(what, |s| {
        s.clock.advance(Duration::from_millis(20));
        done(s)
    });
}

fn run(s: &mut Lsp, command: &str) {
    let invocation = Invocation::of(command).unwrap();
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    s.draw();
}

/// What Fold All folds, undone again.
fn fold_all(s: &mut Lsp) -> Vec<(u32, u32)> {
    run(s, "editor.fold_all");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("shown");
    let folded = editor.read_with(&s.cx, |view, _| {
        let folds = view.model().display().folds();
        folds.iter().map(|fold| (fold.start, fold.end)).collect()
    });
    run(s, "editor.unfold_all");
    folded
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
fn fold_command_folds_the_servers_range(cx: &mut TestAppContext) {
    let (mut s, _editor) = setup(cx, true);
    until_ticking(&mut s, "the server's folds", |s| fold_all(s) == [(1, 3)]);
    // The single-row range is dropped, and the tree's folds are gone.
    assert_eq!(requests(&s), 1);
}

#[gpui::test]
fn no_provider_keeps_tree_sitter_folds(cx: &mut TestAppContext) {
    let (mut s, _editor) = setup(cx, false);
    for _ in 0..5 {
        s.clock.advance(Duration::from_millis(20));
        s.draw();
    }
    assert_eq!(requests(&s), 0);
    assert_eq!(fold_all(&mut s), [(0, 2), (4, 5)]);
}

#[gpui::test]
fn edit_asks_again(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true);
    until_ticking(&mut s, "the server's folds", |s| fold_all(s) == [(1, 3)]);

    s.fake
        .set_reply(FOLDING, reply(json!([{ "startLine": 3, "endLine": 4 }])));
    editor.update(&mut s.cx, |view, _| view.model_mut().set_cursor(0));
    type_in(&mut s, &editor, "// x\n");
    // The old ranges follow the text meanwhile.
    assert_eq!(fold_all(&mut s), [(2, 4)]);
    assert_eq!(requests(&s), 1);
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
    until_ticking(&mut s, "the second request", |s| requests(s) == 2);
    until_ticking(&mut s, "the new folds", |s| fold_all(s) == [(3, 4)]);

    // An error puts the tree's folds back.
    s.fake.set_reply(
        FOLDING,
        Scripted::now(Reply::Error {
            code: -32603,
            message: "boom".into(),
        }),
    );
    type_in(&mut s, &editor, "// y\n");
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
    until_ticking(&mut s, "the third request", |s| requests(s) == 3);
    until_ticking(&mut s, "the tree's folds", |s| {
        fold_all(s) == [(2, 4), (6, 7)]
    });
}

#[gpui::test]
fn a_restarted_server_is_asked_afresh(cx: &mut TestAppContext) {
    let (mut s, _editor) = setup(cx, true);
    until_ticking(&mut s, "the server's folds", |s| fold_all(s) == [(1, 3)]);

    // The old opening's ranges are retired with it; the new session's
    // answer replaces them.
    s.fake
        .set_reply(FOLDING, reply(json!([{ "startLine": 4, "endLine": 6 }])));
    s.run("lsp.restart");
    until_ticking(&mut s, "the new session's folds", |s| {
        fold_all(s) == [(4, 6)]
    });
    assert_eq!(requests(&s), 2);
}
