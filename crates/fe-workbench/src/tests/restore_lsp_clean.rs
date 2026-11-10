//! A restored layout's files stay clean as the language server takes them
//! up (T1665): each is opened on the server, at the path with no `..` in
//! it for one the layout saved through `..`, gets the server's diagnostics,
//! and stays clean over frames and a restore in a second window; the
//! server is sent no change of either.

use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext};
use lsp_types::{
    Diagnostic, Position, PublishDiagnosticsParams, Range, Url, notification::PublishDiagnostics,
};

use super::lsp::Lsp;

/// The saved layout of one pane holding `src/main.rs` and, through `..`,
/// `src/lib.rs`.
fn entry(s: &Lsp) -> fe_command::Value {
    let lib = s.root.join("src/../src/lib.rs");
    serde_json::json!({
        "panes": {
            "tree": { "pane": { "items": [
                {
                    "kind": "fastedit.editor",
                    "document": 0,
                    "state": { "path": s.path("src/main.rs").to_str().unwrap(), "caret": 3 },
                },
                {
                    "kind": "fastedit.editor",
                    "document": 1,
                    "state": { "path": lib.to_str().unwrap(), "caret": 4 },
                },
            ], "active": 0 } },
            "active": [],
        },
    })
}

fn restore(s: &mut Lsp, entry: &fe_command::Value) {
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.restore_layout(entry, window, cx)
        })
    });
    s.draw();
}

fn editors(s: &mut Lsp) -> Vec<Entity<EditorView>> {
    s.workbench
        .read_with(&s.cx, |workbench, cx| workbench.open_editors(cx))
}

/// Each editor's path, whether it's dirty, and how many diagnostics it
/// shows.
fn states(s: &mut Lsp) -> Vec<(String, bool, usize)> {
    let root = s.root.clone();
    (editors(s).into_iter())
        .map(|editor| {
            editor.read_with(&s.cx, |editor, _| {
                let model = editor.model();
                let path = model.path().unwrap().strip_prefix(&root).unwrap();
                let path = path.to_string_lossy().replace('\\', "/");
                (path, model.is_dirty(), model.diagnostics().len())
            })
        })
        .collect()
}

fn publish(s: &Lsp, uri: &Url) {
    s.fake
        .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 2)),
                message: "unused".into(),
                ..Default::default()
            }],
            version: None,
        });
}

#[gpui::test]
fn restored_files_stay_clean_as_the_server_takes_them_up(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let entry = entry(&s);
    restore(&mut s, &entry);
    let uris = [s.uri("src/main.rs"), s.uri("src/lib.rs")];
    for uri in &uris {
        s.fake.wait_for_document(uri, "didOpen", |r| r.is_some());
        publish(&s, uri);
    }
    s.until("the diagnostics to show", |s| {
        states(s).iter().all(|(_, _, shown)| *shown == 1)
    });
    for _ in 0..4 {
        s.cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        s.draw();
    }
    let clean = [
        ("src/main.rs".to_owned(), false, 1),
        ("src/lib.rs".to_owned(), false, 1),
    ];
    assert_eq!(states(&mut s), clean);

    // Restored again, as the window saves it, in another window on the root.
    let saved = s
        .workbench
        .read_with(&s.cx, |workbench, cx| workbench.layout_entry(cx));
    let root = s.root.clone();
    let mut t = s.another_rooted(&root);
    t.set_trust(true);
    restore(&mut t, &saved);
    for _ in 0..4 {
        t.cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        t.draw();
    }
    let states_again: Vec<(String, bool)> = (states(&mut t).into_iter())
        .map(|(path, dirty, _)| (path, dirty))
        .collect();
    assert_eq!(
        states_again,
        [
            ("src/main.rs".to_owned(), false),
            ("src/lib.rs".to_owned(), false)
        ]
    );
    assert_eq!(states(&mut s), clean, "the first window's");
    for uri in &uris {
        assert_eq!(s.sent(1, "textDocument/didChange", uri), 0, "{uri}");
    }
}
