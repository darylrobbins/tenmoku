//! A language change on the owner of a shared server document hands it to
//! the other path alias that server handles (T315), and the owner opens
//! under its new server (T23).

use fe_gpui::EditorView;
use fe_lsp::fake::Record;
use gpui::{AppContext as _, Entity, TestAppContext};

use super::lsp::Lsp;

const ALIAS: &str = "src/../src/main.rs";
const THIRD: &str = "src/../src/../src/main.rs";

/// Loads the languages the tests switch between, so that setting either
/// takes effect within the frame.
fn preload() {
    for name in ["rust", "python", "toml"] {
        assert!(fe_syntax::language_by_name(name).is_some());
    }
}

/// Opens the plain path, owning the document, then each of `aliases` with
/// an edit that tells it apart (`x`, `y`, ...), and returns the owner's
/// editor.
fn open_aliases(s: &mut Lsp, aliases: &[&str]) -> Entity<EditorView> {
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    for (alias, edit) in aliases.iter().zip(["x", "y"]) {
        s.open(alias);
        s.cx.simulate_input(edit);
        s.draw();
    }
    s.open("src/main.rs");
    s.workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

/// The methods of the notifications launch `launch` got for `uri`, in order.
fn methods(records: &[Record], uri: &lsp_types::Url) -> Vec<String> {
    records
        .iter()
        .filter_map(|record| match record {
            Record::Notification { method, params }
                if params["textDocument"]["uri"] == uri.as_str() =>
            {
                Some(method.clone())
            }
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn changing_the_owners_language_hands_the_document_to_the_other_alias(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let owner = open_aliases(&mut s, &[ALIAS]);
    let main = s.uri("src/main.rs");

    s.cx.update(|_, cx| owner.update(cx, |view, cx| view.set_language(Some("python"), cx)));
    // Within the frame transition: no waiting for the server.
    s.draw();
    // The alias, by its edit, now owns the document.
    let replica = s.fake.wait_for_document(&main, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.text == "xfn main() {}\n")
    });
    assert!(replica.is_some());
    let rust = methods(&s.fake.records_of(1), &main);
    let tail: Vec<&str> = rust.iter().rev().take(2).map(String::as_str).collect();
    assert_eq!(tail, ["textDocument/didOpen", "textDocument/didClose"]);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
    // The former owner opened under Python, with its own text.
    let python = s
        .python
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(python.expect("opened").text, "fn main() {}\n");
    assert_eq!(
        methods(&s.python.records_of(1), &main),
        ["textDocument/didOpen"]
    );

    // The alias keeps the document: its edits go to Rust, not Python.
    s.open(ALIAS);
    s.cx.simulate_input("z");
    s.draw();
    s.fake.wait_for_document(&main, "didChange", |r| {
        r.is_some_and(|r| r.text == "xzfn main() {}\n")
    });
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 1);
}

#[gpui::test]
fn owner_switching_to_a_language_without_a_server_leaves_the_alias_served(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let owner = open_aliases(&mut s, &[ALIAS]);
    let main = s.uri("src/main.rs");

    s.cx.update(|_, cx| owner.update(cx, |view, cx| view.set_language(Some("toml"), cx)));
    s.draw();
    let replica = s.fake.wait_for_document(&main, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.text == "xfn main() {}\n")
    });
    assert!(replica.is_some());
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
}

/// The open editors, in the order the frame hook sees them.
fn editors(s: &mut Lsp) -> Vec<Entity<EditorView>> {
    s.workbench.read_with(&s.cx, |w, cx| w.open_editors(cx))
}

/// F3: the alias reserved for the document became unsupported before the
/// next frame; the other alias gets it.
#[gpui::test]
fn a_reserved_alias_that_became_unsupported_does_not_keep_the_document(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let owner = open_aliases(&mut s, &[ALIAS, THIRD]);
    let main = s.uri("src/main.rs");
    // The owner is seen last in a frame, so the aliases are held back by
    // it before it hands the document over.
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.reorder(0, None, window, cx)));
    s.draw();

    // The frame hook run by hand, in the update that changes the language,
    // so that the reservation can be seen and changed before the next frame.
    let workbench = s.workbench.clone();
    let owner_buffer = s.cx.read_entity(&owner, |v, _| v.model().buffer().id());
    let reserved_editor = s.cx.update(|_, cx| {
        owner.update(cx, |view, cx| view.set_language(Some("python"), cx));
        workbench.update(cx, |w, cx| w.sync_lsp(cx));
        let reserved = workbench.read(cx).lsp.reserved_buffers();
        assert_eq!(reserved.len(), 1, "one alias is reserved the document");
        assert_ne!(reserved[0], owner_buffer);
        let editor = workbench
            .read(cx)
            .open_editors(cx)
            .into_iter()
            .find(|e| e.read(cx).model().buffer().id() == reserved[0])
            .expect("the reserved alias is shown");
        // It became unsupported before the next frame.
        editor.update(cx, |view, cx| view.set_language(Some("toml"), cx));
        workbench.update(cx, |w, cx| w.sync_lsp(cx));
        editor
    });
    let reserved_editor_text = s.cx.read_entity(&reserved_editor, |v, _| {
        v.model().buffer().text().to_string()
    });
    // The other alias, told apart by its edit, opens it.
    let replica = s.fake.wait_for_document(&main, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.text != "fn main() {}\n")
    });
    let replica = replica.expect("opened");
    assert_ne!(
        replica.text, reserved_editor_text,
        "not the unsupported alias"
    );
    assert!(
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp.reserved_buffers())
            .is_empty()
    );
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
}

/// F2: an alias goes by its first view, which here is plain text though a
/// second view of it is Rust: it isn't a candidate, and the other is served.
#[gpui::test]
fn an_alias_goes_by_its_first_view(cx: &mut TestAppContext) {
    preload();
    let mut s = Lsp::new(cx);
    let owner = open_aliases(&mut s, &[ALIAS, THIRD]);
    let main = s.uri("src/main.rs");
    s.open(ALIAS);
    s.run("pane.split_right");
    let alias = s.path(ALIAS);
    let views: Vec<_> = editors(&mut s)
        .into_iter()
        .filter(|e| {
            s.cx.read_entity(e, |v, _| v.model().path() == Some(alias.as_path()))
        })
        .collect();
    assert_eq!(views.len(), 2, "the alias has two views");
    s.cx.update(|_, cx| {
        views[0].update(cx, |view, cx| view.set_language(None, cx));
        owner.update(cx, |view, cx| view.set_language(Some("python"), cx));
    });
    s.draw();
    let replica = s.fake.wait_for_document(&main, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.text == "yfn main() {}\n")
    });
    assert!(replica.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
}
