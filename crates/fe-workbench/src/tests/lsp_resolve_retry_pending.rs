//! A save that arrives while the root's retry is in flight isn't lost
//! (T317). The retry's resolution runs, and the save is delivered, before
//! its completion is let land: only background tasks are ticked until then.

use std::{fs, path::Path};

use gpui::{Keystroke, TestAppContext};

use super::lsp::Lsp;

impl Lsp {
    fn open_file(&mut self, name: &str) {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        let path = self.path(name);
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        self.draw();
    }

    fn replace_root(&mut self, root: &Path) {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.set_root(root.to_owned(), window, cx)
            })
        });
        self.draw();
    }

    /// Saves the active buffer, then draws a frame without letting any
    /// task but the background's run. Typing would run them all, so the
    /// edit is made beforehand.
    fn save_and_hold(&mut self) {
        self.cx.update(|window, cx| {
            window.dispatch_keystroke(Keystroke::parse("cmd-s").unwrap(), cx);
        });
        self.hold();
    }

    /// Draws a frame, and runs the background tasks: the foreground's wait.
    fn hold(&mut self) {
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
        while self.cx.dispatcher.tick(true) {}
    }
}

#[gpui::test]
fn a_save_during_a_failing_root_retry_opens_the_file_after_it(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let later = s.path("later");
    s.replace_root(&later);
    s.set_trust(true);

    // The first retry fails to resolve the root; its completion waits.
    // The file under the root is shown, but doesn't exist yet.
    s.open_file("later/src/main.rs");
    let main = s.uri("later/src/main.rs");
    s.open_file("src/main.rs");
    s.cx.simulate_input("x");
    s.save_and_hold();

    // The root exists by the next save, which arrives before that
    // completion does.
    fs::create_dir_all(later.join("src")).unwrap();
    fs::write(later.join("src/main.rs"), "fn main() {}\n").unwrap();
    s.save_and_hold();
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 0);

    s.draw();
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
}

#[gpui::test]
fn a_successful_retry_drops_the_pending_save(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let later = s.path("later");
    s.replace_root(&later);
    s.set_trust(true);
    fs::create_dir_all(later.join("src")).unwrap();
    fs::write(later.join("src/main.rs"), "fn main() {}\n").unwrap();
    let main = s.uri("later/src/main.rs");

    // The retry resolves the root; a save arrives before it completes.
    s.open_file("later/src/main.rs");
    s.cx.simulate_input("x");
    s.save_and_hold();
    s.save_and_hold();
    s.draw();
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    s.draw();
    // One manager, one launch: no further resolution restarted it.
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
    assert_eq!(s.sent(2, "textDocument/didOpen", &main), 0);
}

#[gpui::test]
fn a_root_replacement_drops_the_pending_save(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let later = s.path("later");
    s.replace_root(&later);
    s.set_trust(true);

    // A retry fails, and a save is kept for it.
    s.open_file("src/main.rs");
    s.cx.simulate_input("x");
    s.save_and_hold();
    s.save_and_hold();

    // A new root that fails too, and a retry of its own.
    let other = s.path("other");
    s.replace_root(&other);
    s.set_trust(true);
    s.cx.simulate_input("x");
    s.save_and_hold();

    // Its folder exists before the retry completes, but no save follows:
    // the earlier save isn't resolved again.
    fs::create_dir_all(other.join("src")).unwrap();
    fs::write(other.join("src/main.rs"), "fn main() {}\n").unwrap();
    let main = s.uri("other/src/main.rs");
    s.draw();
    s.open_file("other/src/main.rs");
    s.draw();
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 0);
}
