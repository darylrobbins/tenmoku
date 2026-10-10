//! A language change on the owner of a shared server document hands it to
//! the other path alias (T315).

use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext};

use super::lsp::Lsp;

const ALIAS: &str = "src/../src/main.rs";

/// Opens both aliases, the plain one owning the document, and returns the
/// owner's editor.
fn two_aliases(s: &mut Lsp) -> Entity<EditorView> {
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.open(ALIAS);
    s.open("src/main.rs");
    s.workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown")
}

#[gpui::test]
fn changing_the_owners_language_hands_the_document_to_the_other_alias(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let owner = two_aliases(&mut s);
    let main = s.uri("src/main.rs");

    s.cx.update(|_, cx| owner.update(cx, |view, cx| view.set_language(Some("python"), cx)));
    s.until("the hand-over", |s| {
        s.sent(1, "textDocument/didOpen", &main) == 2
    });
    s.draw();
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
    let replica = s
        .fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    assert!(replica.is_some());
}

#[gpui::test]
fn owner_switching_to_a_language_without_a_server_leaves_the_alias_served(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let owner = two_aliases(&mut s);
    let main = s.uri("src/main.rs");

    s.cx.update(|_, cx| owner.update(cx, |view, cx| view.set_language(Some("toml"), cx)));
    s.until("the hand-over", |s| {
        s.sent(1, "textDocument/didOpen", &main) == 2
    });
    s.draw();
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
}
