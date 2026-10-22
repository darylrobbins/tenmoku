//! A failed path resolution is retried at the next save and at a root change
//! (T237).

use std::{fs, path::Path};

use gpui::TestAppContext;

use super::lsp::Lsp;

impl Lsp {
    fn open_new(&mut self, name: &str) {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        let path = self.path(name);
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        self.draw();
    }

    fn set_root(&mut self, root: &Path) {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.set_root(root.to_owned(), window, cx)
            })
        });
        self.draw();
    }
}

#[gpui::test]
fn a_new_file_opens_once_its_folder_exists_and_it_is_saved(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "didOpen", |r| r.is_some());

    s.open_new("new/mod.rs");
    let new = s.uri("new/mod.rs");
    // Anything sent for it goes before this.
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &new), 0);

    fs::create_dir(s.path("new")).unwrap();
    s.open_new("new/mod.rs");
    s.cx.simulate_input("x");
    s.cx.simulate_keystrokes("cmd-s");
    s.draw();
    s.fake.wait_for_document(&new, "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &new), 1);
}

#[gpui::test]
fn a_failure_cached_before_a_root_change_is_resolved_again(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open_new("new/mod.rs");
    let new = s.uri("new/mod.rs");
    assert_eq!(s.sent(1, "textDocument/didOpen", &new), 0);

    fs::create_dir(s.path("new")).unwrap();
    let root = s.root.clone();
    s.set_root(&root);
    s.set_trust(true);
    s.fake.wait_for_document(&new, "didOpen", |r| r.is_some());
}

#[gpui::test]
fn a_root_that_failed_to_resolve_starts_servers_after_a_save(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let missing = s.path("later");
    s.set_root(&missing);
    s.set_trust(true);

    fs::create_dir_all(missing.join("src")).unwrap();
    fs::write(missing.join("src/main.rs"), "fn main() {}\n").unwrap();
    s.set_trust(true);
    s.open_new("later/src/main.rs");
    let main = s.uri("later/src/main.rs");
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 0);

    s.cx.simulate_input("x");
    s.cx.simulate_keystrokes("cmd-s");
    s.draw();
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
}

#[path = "lsp_completion_command.rs"]
mod completion_command;
