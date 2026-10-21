//! Path aliases of one file, open as separate buffers, share one server
//! document (T236).

use gpui::TestAppContext;

use super::lsp::Lsp;

const ALIAS: &str = "src/../src/main.rs";

#[gpui::test]
fn aliases_of_one_file_open_one_document(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.open(ALIAS);

    // Edits in the alias go nowhere: the sentinel's didOpen follows them.
    s.cx.simulate_input("x");
    s.draw();
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didChange", &main), 0);
}

#[gpui::test]
fn closing_the_owning_alias_hands_the_document_to_the_other(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.open(ALIAS);

    // The second buffer never owned the document: closing it sends nothing.
    s.cx.simulate_keystrokes("cmd-w");
    s.draw();
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 0);

    // With the alias open again and edited, closing the owner hands it over.
    s.open(ALIAS);
    s.cx.simulate_input("x");
    s.draw();
    s.open("src/main.rs");
    s.cx.simulate_keystrokes("cmd-w");
    s.draw();
    let replica = s.fake.wait_for_document(&main, "didOpen", |replica| {
        replica.is_some_and(|r| r.open && r.text == "xfn main() {}\n")
    });
    assert!(replica.is_some());
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 2);
}

#[path = "lsp_hover_markdown.rs"]
mod hover_markdown;

#[path = "lsp_hover_ra_html.rs"]
mod hover_ra_html;
