//! Workspace trust gates the workbench's language servers: none starts for
//! a root not trusted, and trusting it starts them for the files already
//! open.

use gpui::TestAppContext;

use crate::tests::lsp::Lsp;

#[gpui::test]
fn servers_start_only_in_a_trusted_root(cx: &mut TestAppContext) {
    // Never decided.
    let mut s = Lsp::untrusted(cx);
    assert!(!s.workbench.read_with(&s.cx, |w, _| w.root_trusted()));
    s.open("src/main.rs");
    // Opened on the manager, with no server: one would have its id by now.
    assert_eq!(s.active_server(), Some(None));
    assert_eq!(s.fake.launches(), 0);

    // Don't Trust.
    s.set_trust(false);
    s.open("src/lib.rs");
    assert_eq!(s.active_server(), Some(None));
    assert_eq!(s.fake.launches(), 0);

    // Trusted: the server starts, and gets the files already open.
    s.set_trust(true);
    let main = s.uri("src/main.rs");
    let lib = s.uri("src/lib.rs");
    s.fake.wait_for_document(&main, "main.rs", |r| r.is_some());
    s.fake.wait_for_document(&lib, "lib.rs", |r| r.is_some());
    assert_eq!(s.fake.launches(), 1);
    assert_eq!(s.fake.launched()[0].0.name, "rust-analyzer");

    // Untrusted again: it shuts down, and nothing starts for later opens.
    s.set_trust(false);
    s.fake.wait_for_at(1, "shutdown, exit", |records| {
        records.iter().any(|r| r.is_notification("exit"))
    });
    s.open("tool.py");
    assert_eq!(s.active_server(), Some(None));
    s.open("src/other.rs");
    assert_eq!(s.active_server(), Some(None));
    assert_eq!(s.fake.launches(), 1);
}

#[path = "lsp_highlights_tests.rs"]
mod caret_highlights;

#[path = "lsp_lenses_tests.rs"]
mod lenses;

#[path = "lsp_lens_references_tests.rs"]
mod lens_references;

#[path = "lsp_grammarless_tests.rs"]
mod grammarless;
