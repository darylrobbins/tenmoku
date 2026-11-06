//! Go to definition and find references from a language server (fe-lsp
//! card 6): by search while the fake standing in for rust-analyzer starts,
//! from it once it's ready, and rebased onto text typed after the answer.

use std::{fs, ops::Range, path::PathBuf};

use fe_buffer::Origin;
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _};
use lsp_types::{
    ImplementationProviderCapability, LSPAny, LSPObject, PositionEncodingKind,
    TypeDefinitionProviderCapability, Url, notification::Notification,
};

use super::lsp::Lsp;
use crate::{StatusSide, nav::SOURCE_ITEM};

/// `helper` defined on row 0, and called on row 3.
const MAIN: &str = "fn helper() {}\n\nfn main() {\n    helper();\n}\n";

/// rust-analyzer's readiness notification.
enum ServerStatusNotification {}

impl Notification for ServerStatusNotification {
    type Params = LSPAny;
    const METHOD: &'static str = "experimental/serverStatus";
}

fn object(fields: impl IntoIterator<Item = (&'static str, LSPAny)>) -> LSPAny {
    let fields = fields
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value));
    LSPAny::Object(fields.collect::<LSPObject>())
}

/// A location on one row, as the fake answers it.
fn location(uri: &Url, row: u32, columns: Range<usize>) -> LSPAny {
    let position = |column: usize| {
        object([
            ("line", LSPAny::from(row)),
            ("character", LSPAny::from(column)),
        ])
    };
    let range = object([
        ("start", position(columns.start)),
        ("end", position(columns.end)),
    ]);
    object([("uri", LSPAny::from(uri.as_str())), ("range", range)])
}

fn bytes_of(text: &str, word: &str) -> Range<usize> {
    let start = text.find(word).unwrap();
    start..start + word.len()
}

fn editor(s: &mut Lsp) -> Entity<EditorView> {
    s.workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

/// The active editor's file and selection.
fn at(s: &mut Lsp) -> (PathBuf, Range<usize>) {
    let editor = editor(s);
    editor.read_with(&s.cx, |editor, _| {
        let model = editor.model();
        let path = model.path().unwrap().to_owned();
        (path, model.selections()[0].range.clone())
    })
}

/// Puts the caret inside the call to `helper`.
fn caret_on_call(s: &mut Lsp) {
    let editor = editor(s);
    editor.update(&mut s.cx, |editor, cx| {
        let offset = editor.model().text().to_string().find("helper();").unwrap() + 2;
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    s.draw();
}

/// Types `text` at the start of the active editor's file.
fn type_at_start(s: &mut Lsp, text: &str) {
    let editor = editor(s);
    editor.update(&mut s.cx, |editor, cx| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(0..0, text)], Origin::User).unwrap();
        drop(buffer);
        cx.notify();
    });
    s.draw();
}

/// The status bar's left items.
fn source(s: &mut Lsp) -> Option<String> {
    let bar = s.workbench.read_with(&s.cx, |w, _| w.status_bar().clone());
    let items = bar.read_with(&s.cx, |bar, _| bar.drawn(StatusSide::Left));
    (items.into_iter())
        .find(|(id, _)| id == SOURCE_ITEM)
        .map(|(_, text)| text.to_string())
}

/// How many `method` requests the fake has had.
fn asked(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method: m, .. } if m == method))
        .count()
}

#[gpui::test]
fn goto_definition_before_and_after_ready(cx: &mut gpui::TestAppContext) {
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let (uri, path) = (s.uri("src/main.rs"), s.path("src/main.rs"));
    s.fake.wait_for_document(&uri, "didOpen", |r| r.is_some());
    // `main`, which search wouldn't answer for `helper`.
    let main = bytes_of(MAIN, "main");
    s.fake.set_reply(
        "textDocument/definition",
        Scripted::now(Reply::Result(LSPAny::Array(vec![location(&uri, 2, 3..7)]))),
    );

    // Starting: by search, to `helper`'s own definition.
    caret_on_call(&mut s);
    s.cx.simulate_keystrokes("f12");
    s.until("search's jump", |s| at(s) == (path.clone(), 3..9));
    assert_eq!(source(&mut s).as_deref(), Some("by search"));
    assert_eq!(asked(&s, "textDocument/definition"), 0);

    // Ready.
    let mut status = LSPObject::new();
    status.insert("health".into(), "ok".into());
    status.insert("quiescent".into(), true.into());
    s.fake
        .notify::<ServerStatusNotification>(LSPAny::Object(status));
    s.wait_item(None);

    // Requests carry the editor's buffer and version.
    caret_on_call(&mut s);
    let editor = editor(&mut s);
    let request = s
        .workbench
        .read_with(&s.cx, |w, cx| w.nav_request(cx).unwrap());
    let snapshot = editor.read_with(&s.cx, |editor, _| {
        let snapshot = editor.model().snapshot();
        (snapshot.id(), snapshot.version())
    });
    assert_eq!(request.buffer, Some(snapshot));

    // The answer is held after the server gave it; text typed above its
    // target meanwhile moves the jump with it.
    let (release, held) = async_channel::bounded::<()>(1);
    s.workbench
        .update(&mut s.cx, |w, _| w.nav_state.hold_answer = Some(held));
    s.cx.simulate_keystrokes("f12");
    s.until("the server to be asked", |s| {
        asked(s, "textDocument/definition") == 1
    });
    let typed = "// typed\n";
    type_at_start(&mut s, typed);
    drop(release);
    let moved = main.start + typed.len()..main.end + typed.len();
    s.until("the server's jump", |s| {
        at(s) == (path.clone(), moved.clone())
    });
    let source = source(&mut s).unwrap();
    assert!(!source.contains("search"), "{source}");

    // References, in T112's panel, rebased the same way.
    let text = format!("{typed}{MAIN}");
    let (def, call) = (bytes_of(&text, "helper"), bytes_of(&text, "helper();"));
    s.fake.set_reply(
        "textDocument/references",
        Scripted::now(Reply::Result(LSPAny::Array(vec![
            location(&uri, 4, 4..10),
            location(&uri, 1, 3..9),
        ]))),
    );
    caret_on_call(&mut s);
    s.cx.simulate_keystrokes("shift-alt-f12");
    let panel = s
        .workbench
        .read_with(&s.cx, |w, cx| w.dock().read(cx).references().clone());
    s.until("the server's references", |s| {
        panel.read_with(&s.cx, |panel, _| panel.locations().len()) == 2
    });
    let listed = panel.read_with(&s.cx, |panel, _| panel.locations());
    let ranges: Vec<_> = listed.iter().map(|at| at.range.clone()).collect();
    assert_eq!(ranges, [def.clone(), call.start..call.start + 6]);
    let status = panel.read_with(&s.cx, |panel, _| panel.status_text());
    assert!(!status.as_ref().unwrap().contains("search"), "{status:?}");

    let more = "// more\n";
    type_at_start(&mut s, more);
    s.cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    assert_eq!(panel.read_with(&s.cx, |panel, _| panel.selected()), Some(1));
    s.cx.simulate_keystrokes("enter");
    let moved = def.start + more.len()..def.end + more.len();
    s.until("the reference to open", |s| {
        at(s) == (path.clone(), moved.clone())
    });
}

/// How many tabs the active pane has.
fn tabs(s: &mut Lsp) -> usize {
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    pane.read_with(&s.cx, |pane, _| pane.tabs().len())
}

#[gpui::test]
#[cfg(unix)]
fn a_jump_goes_to_the_buffer_the_server_named(cx: &mut gpui::TestAppContext) {
    let mut s = Lsp::new(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    let other = "pub fn other() {}\n";
    fs::write(s.path("src/other.rs"), other).unwrap();
    std::os::unix::fs::symlink(s.path("src"), s.path("alias")).unwrap();

    // `other.rs` is open through the link, and edited, unsaved; the server
    // has it by its resolved path.
    s.open("alias/other.rs");
    let other_uri = s.uri("src/other.rs");
    s.fake
        .wait_for_document(&other_uri, "didOpen", |r| r.is_some());
    let typed = "// typed\n";
    type_at_start(&mut s, typed);
    s.fake
        .wait_for_document(&other_uri, "the edit", |r| r.is_some_and(|r| r.version > 0));

    s.open("src/main.rs");
    let mut status = LSPObject::new();
    status.insert("health".into(), "ok".into());
    status.insert("quiescent".into(), true.into());
    s.fake
        .notify::<ServerStatusNotification>(LSPAny::Object(status));
    s.wait_item(None);
    s.fake.set_reply(
        "textDocument/definition",
        Scripted::now(Reply::Result(LSPAny::Array(vec![location(
            &other_uri,
            1,
            7..12,
        )]))),
    );
    let before = tabs(&mut s);
    caret_on_call(&mut s);
    s.cx.simulate_keystrokes("f12");
    let name = bytes_of(&format!("{typed}{other}"), "other");
    let alias = s.path("alias/other.rs");
    s.until("the jump into the open buffer", |s| {
        at(s) == (alias.clone(), name.clone())
    });
    assert_eq!(tabs(&mut s), before, "no second buffer for the file");
}

#[gpui::test]
fn goto_type_definition_and_implementation_jump_to_the_servers_answer(
    cx: &mut gpui::TestAppContext,
) {
    let mut result = initialize_result(Some(PositionEncodingKind::UTF8));
    result.capabilities.type_definition_provider =
        Some(TypeDefinitionProviderCapability::Simple(true));
    result.capabilities.implementation_provider =
        Some(ImplementationProviderCapability::Simple(true));
    let rust = Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, rust, Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let (uri, path) = (s.uri("src/main.rs"), s.path("src/main.rs"));
    s.fake.wait_for_document(&uri, "didOpen", |r| r.is_some());
    let mut status = LSPObject::new();
    status.insert("health".into(), "ok".into());
    status.insert("quiescent".into(), true.into());
    s.fake
        .notify::<ServerStatusNotification>(LSPAny::Object(status));
    s.wait_item(None);

    // One answer: a jump, to `main`.
    s.fake.set_reply(
        "textDocument/typeDefinition",
        Scripted::now(Reply::Result(LSPAny::Array(vec![location(&uri, 2, 3..7)]))),
    );
    caret_on_call(&mut s);
    s.workbench.update_in(&mut s.cx, |w, window, cx| {
        w.run_nav_op(
            crate::nav::NavOp::Related(fe_nav::Related::TypeDefinition),
            window,
            cx,
        )
    });
    let main = bytes_of(MAIN, "main");
    s.until("the type definition's jump", |s| {
        at(s) == (path.clone(), main.clone())
    });
    assert_eq!(asked(&s, "textDocument/typeDefinition"), 1);

    // Two: listed.
    s.fake.set_reply(
        "textDocument/implementation",
        Scripted::now(Reply::Result(LSPAny::Array(vec![
            location(&uri, 0, 3..9),
            location(&uri, 2, 3..7),
        ]))),
    );
    caret_on_call(&mut s);
    s.workbench.update_in(&mut s.cx, |w, window, cx| {
        w.run_nav_op(
            crate::nav::NavOp::Related(fe_nav::Related::Implementation),
            window,
            cx,
        )
    });
    s.until("both implementations", |s| {
        s.workbench.read_with(&s.cx, |w, cx| {
            w.nav_state.definitions.read(cx).delegate.items().len() == 2
        })
    });
    assert_eq!(asked(&s, "textDocument/implementation"), 1);
    // Both items, by file and range.
    let items = s.workbench.read_with(&s.cx, |w, cx| {
        let picker = w.nav_state.definitions.read(cx);
        (picker.delegate.items().iter())
            .map(|item| (item.location.rel.clone(), item.location.range.clone()))
            .collect::<Vec<_>>()
    });
    let rel = PathBuf::from("src/main.rs");
    assert_eq!(
        items,
        [
            (rel.clone(), bytes_of(MAIN, "helper")),
            (rel, bytes_of(MAIN, "main")),
        ]
    );

    // Each request named the file and the caret.
    let column = "    helper();".find("lper").unwrap();
    for method in ["textDocument/typeDefinition", "textDocument/implementation"] {
        let params = (s.fake.records().into_iter())
            .find_map(|record| match record {
                Record::Request {
                    method: m, params, ..
                } if m == method => Some(params),
                _ => None,
            })
            .unwrap();
        assert_eq!(params["textDocument"]["uri"], uri.as_str(), "{method}");
        assert_eq!(params["position"]["line"], 3, "{method}");
        assert_eq!(params["position"]["character"], column, "{method}");
    }
}

#[gpui::test]
fn goto_declaration_without_a_server_says_none_found(cx: &mut gpui::TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    caret_on_call(&mut s);
    let before = tabs(&mut s);
    s.workbench.update_in(&mut s.cx, |w, window, cx| {
        w.run_nav_op(
            crate::nav::NavOp::Related(fe_nav::Related::Declaration),
            window,
            cx,
        )
    });
    s.until("the status", |s| {
        s.workbench.read_with(&s.cx, |w, cx| {
            w.nav_state.definitions.read(cx).delegate.status_text()
                == "No declaration found for `helper`"
        })
    });
    assert_eq!(tabs(&mut s), before);
}
