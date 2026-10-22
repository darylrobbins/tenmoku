//! Language servers for files outside the window's folder, and in windows
//! with no folder: a file's own project root (its marker root) gets a
//! server only if it's inside a folder the user trusted, in any window.
//! Otherwise the file gets syntax highlighting alone, with no prompt, and
//! the status item says why. A temporary `/t` holds the window's folder
//! `w` and the project `other` (`Cargo.toml`, `src/main.rs`); decisions go
//! to a temporary trust file, so a prompt would show.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_lsp::{
    ManualClock,
    fake::{FakeServer, Reply, Script, Scripted},
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use lsp_types::{
    LSPAny, LSPObject, Url,
    request::{HoverRequest, Request as _},
};

use super::super::lsp::window;
use crate::{
    Launch, Workbench, lsp::TestClock, open::ConfigDirOverride, set_server_launcher,
    set_trust_file, trust::decide,
};

/// `/t`, its trust file and the fake, installed as the host's launcher.
struct Folders {
    /// `/t`, resolved.
    t: PathBuf,
    fake: FakeServer,
    _dir: tempfile::TempDir,
    _state: tempfile::TempDir,
}

impl Folders {
    fn new(cx: &mut TestAppContext) -> Self {
        // Server threads wake the workbench's event loop.
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let t = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(t.join("w")).unwrap();
        fs::write(t.join("w/notes.txt"), "notes\n").unwrap();
        fs::create_dir_all(t.join("other/src")).unwrap();
        fs::write(t.join("other/Cargo.toml"), "[package]\nname = \"other\"\n").unwrap();
        fs::write(t.join("other/src/main.rs"), "fn main() {}\n").unwrap();
        let state = tempfile::tempdir().unwrap();
        let fake = FakeServer::new(Script::default());
        let config = state.path().join("config");
        fs::create_dir_all(&config).unwrap();
        let trust_file = state.path().join("trust.toml");
        cx.update(|cx| {
            set_server_launcher(cx, fake.launcher());
            cx.set_global(TestClock(Arc::new(ManualClock::new())));
            cx.set_global(ConfigDirOverride(Some(config)));
            set_trust_file(cx, trust_file);
        });
        Self {
            t,
            fake,
            _dir: dir,
            _state: state,
        }
    }

    /// Records `folder`'s decision, as any window would.
    fn decide(&self, folder: &Path, trusted: bool, cx: &mut TestAppContext) {
        cx.update(|cx| decide(folder, trusted, cx));
    }

    fn main_rs(&self) -> PathBuf {
        self.t.join("other/src/main.rs")
    }

    fn main_uri(&self) -> Url {
        Url::from_file_path(self.main_rs()).unwrap()
    }
}

/// A workbench for `launch`, drawn.
fn open(launch: Launch, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let (workbench, mut cx) = window(cx, launch);
    draw(&mut cx);
    (workbench, cx)
}

fn rooted(root: &Path) -> Launch {
    Launch {
        root: Some(root.to_owned()),
        file: None,
    }
}

/// Lets background work land, then draws a frame, twice: a frame may
/// start work (resolving a file's path) that the next one uses.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// Opens `path` in the active pane, and draws.
fn open_path(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

/// Draws until `done` holds, for up to 60 s: whether it did.
fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        draw(cx);
        if done(cx) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Draws until `done` holds. Panics after 60 s, naming `what`.
fn until(what: &str, cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
    assert!(settle(cx, done), "timed out waiting for {what}");
}

/// The language server status item's text and tooltip.
fn item(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(String, Option<String>)> {
    workbench.read_with(cx, |w, cx| {
        let items = w.status_bar().read(cx).items(cx);
        let item = items.into_iter().find(|item| item.id == crate::lsp::ITEM)?;
        Some((
            item.text.to_string(),
            item.tooltip.map(|tip| tip.to_string()),
        ))
    })
}

/// Whether the item, if any, is the server's own: its tooltip is the
/// log's.
fn servers_own(item: Option<(String, Option<String>)>) -> bool {
    item.is_none_or(|(text, tooltip)| {
        text.starts_with("rust-analyzer: ")
            && tooltip.as_deref() == Some("Show Language Server Log")
    })
}

/// What the status item says for a file whose project at `root` isn't
/// trusted.
fn not_trusted(root: &Path) -> Option<(String, Option<String>)> {
    Some((
        "rust-analyzer: folder not trusted".to_owned(),
        Some(format!(
            "{} isn't inside a trusted folder, so its language servers don't start",
            root.display()
        )),
    ))
}

/// The active editor's language, as its highlighting has it.
fn language(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |w, cx| {
        let editor = w.active_editor(cx)?.read(cx);
        Some(editor.syntax()?.language().name().to_owned())
    })
}

/// Asks for a hover at `main`'s name and waits for the popover.
fn hover(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let editor = (workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned())).unwrap();
    cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn ma".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    draw(cx);
    cx.simulate_keystrokes("cmd-k cmd-i");
    until("the hover", cx, |cx| {
        editor
            .read_with(cx, |view, _| view.popover_text())
            .is_some()
    });
    editor.read_with(cx, |view, _| view.popover_text())
}

#[gpui::test]
fn files_outside_the_folder_get_a_server_in_trusted_folders(cx: &mut TestAppContext) {
    let f = Folders::new(cx);
    // `{"contents": {"kind": "markdown", "value": "fn main()"}}`.
    let markup = LSPObject::from_iter([
        ("kind".to_owned(), LSPAny::from("markdown")),
        ("value".to_owned(), LSPAny::from("fn main()")),
    ]);
    let answer = LSPObject::from_iter([("contents".to_owned(), LSPAny::Object(markup))]);
    f.fake.set_reply(
        HoverRequest::METHOD,
        Scripted::now(Reply::Result(LSPAny::Object(answer))),
    );
    f.decide(&f.t, true, cx);
    let (workbench, mut cx) = open(rooted(&f.t.join("w")), cx);
    assert!(
        workbench.read_with(&cx, |w, _| w.root_trusted()),
        "/t/w too"
    );
    assert!(!cx.has_pending_prompt());

    open_path(&workbench, f.main_rs(), &mut cx);
    settle(&mut cx, |_| f.fake.launches() > 0);
    let launched: Vec<_> = (f.fake.launched().into_iter())
        .map(|(spec, root)| (spec.name, root))
        .collect();
    let at_marker_root = ("rust-analyzer".to_owned(), f.t.join("other"));
    assert_eq!(launched, [at_marker_root]);
    let replica = f
        .fake
        .wait_for_document(&f.main_uri(), "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(replica.map(|r| r.text).as_deref(), Some("fn main() {}\n"));
    assert_eq!(hover(&workbench, &mut cx).as_deref(), Some("fn main()"));
    until("the server's status", &mut cx, |cx| {
        servers_own(item(&workbench, cx))
    });

    // `/t/other` untrusted: its server stops, and the item says why.
    f.decide(&f.t.join("other"), false, &mut cx);
    draw(&mut cx);
    until("the server to stop", &mut cx, |_| {
        f.fake
            .records_of(1)
            .iter()
            .any(|r| r.is_notification("exit"))
    });
    assert_eq!(item(&workbench, &mut cx), not_trusted(&f.t.join("other")));
    assert_eq!(f.fake.launches(), 1);
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
fn untrusted_roots_get_no_server(cx: &mut TestAppContext) {
    let f = Folders::new(cx);
    // The window's folder alone is trusted.
    f.decide(&f.t.join("w"), true, cx);
    let (workbench, mut cx) = open(rooted(&f.t.join("w")), cx);
    assert!(workbench.read_with(&cx, |w, _| w.root_trusted()));

    open_path(&workbench, f.main_rs(), &mut cx);
    assert_eq!(f.fake.launches(), 0);
    assert!(!cx.has_pending_prompt(), "no prompt for a loose file");
    assert_eq!(item(&workbench, &mut cx), not_trusted(&f.t.join("other")));
    // The status bar draws it.
    let bar = workbench.read_with(&cx, |w, _| w.status_bar().clone());
    let drawn = bar.read_with(&cx, |bar, _| bar.drawn(crate::StatusSide::Right));
    assert!(
        drawn
            .iter()
            .any(|(id, text)| id == crate::lsp::ITEM && text == "rust-analyzer: folder not trusted"),
        "{drawn:?}"
    );
    assert_eq!(language(&workbench, &mut cx).as_deref(), Some("rust"));

    // Trusted, in any window: its server starts.
    f.decide(&f.t.join("other"), true, &mut cx);
    draw(&mut cx);
    f.fake
        .wait_for_document(&f.main_uri(), "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(f.fake.launched()[0].1, f.t.join("other"));
    until("the item to drop the reason", &mut cx, |cx| {
        servers_own(item(&workbench, cx))
    });
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
fn a_window_with_no_folder_gets_servers(cx: &mut TestAppContext) {
    let f = Folders::new(cx);
    let empty = || Launch {
        root: None,
        file: None,
    };
    f.decide(&f.t, true, cx);
    let (workbench, mut cx) = open(empty(), cx);
    assert_eq!(
        workbench.read_with(&cx, |w, _| w.lsp_manager()),
        Some(false)
    );

    open_path(&workbench, f.main_rs(), &mut cx);
    f.fake
        .wait_for_document(&f.main_uri(), "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(f.fake.launched()[0].1, f.t.join("other"));

    // A project in an untrusted location: none.
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere_root = fs::canonicalize(elsewhere.path()).unwrap();
    fs::write(elsewhere_root.join("Cargo.toml"), "[package]\n").unwrap();
    fs::create_dir(elsewhere_root.join("src")).unwrap();
    fs::write(elsewhere_root.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    let (other, mut other_cx) = open(empty(), &mut cx);
    open_path(&other, elsewhere_root.join("src/lib.rs"), &mut other_cx);
    assert_eq!(f.fake.launches(), 1);
    assert_eq!(item(&other, &mut other_cx), not_trusted(&elsewhere_root));
    assert_eq!(language(&other, &mut other_cx).as_deref(), Some("rust"));
    assert!(!other_cx.has_pending_prompt());
}
