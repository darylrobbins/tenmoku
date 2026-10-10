//! A file symlink is followed to its target before the root check (T235).

use std::{fs, os::unix::fs::symlink, path::PathBuf};

use gpui::TestAppContext;
use lsp_types::Url;

use super::lsp::Lsp;

impl Lsp {
    fn open_at(&mut self, path: PathBuf) {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        self.draw();
    }
}

#[gpui::test]
fn a_file_symlink_out_of_the_root_is_not_sent(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let outside = tempfile::tempdir().unwrap();
    let target = fs::canonicalize(outside.path()).unwrap().join("secret.rs");
    fs::write(&target, "fn secret() {}\n").unwrap();
    let link = s.path("src/link.rs");
    symlink(&target, &link).unwrap();

    s.open_at(link.clone());
    // Anything sent for the link goes before this.
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());

    for path in [&link, &target] {
        let uri = Url::from_file_path(path).unwrap();
        assert_eq!(s.sent(1, "textDocument/didOpen", &uri), 0);
    }
    let leaked = s
        .fake
        .records_of(1)
        .iter()
        .any(|record| format!("{record:?}").contains("fn secret"));
    assert!(!leaked);
}

#[gpui::test]
fn a_file_symlink_into_the_root_opens_its_target(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let outside = tempfile::tempdir().unwrap();
    let link = fs::canonicalize(outside.path())
        .unwrap()
        .join("main_link.rs");
    symlink(s.path("src/main.rs"), &link).unwrap();

    s.open_at(link);
    let main = s.uri("src/main.rs");
    let replica = s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    assert!(replica.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
}
