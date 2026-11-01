//! The Chat button on the rail from the start, under Git Log, and the key
//! not read until the chat shows.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use fe_buffer::Buffer;
use fe_ui::Icon;
use gpui::{Modifiers, TestAppContext};
use kiln::core::secrets::{MemoryStore, Secret, SecretError, SecretStore};

use super::{
    ChatPanel, KeyOrigin, KeySource, chat, composer_focused, dispatch, draw, opened, rail,
    scripted, shown,
};
use crate::{DockPanel, Launch, Workbench, tests::open};
use gpui::{Entity, Focusable as _, VisualTestContext};

/// A store counting its reads.
struct Counting {
    reads: Arc<AtomicUsize>,
    inner: MemoryStore,
}

impl SecretStore for Counting {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(service, account)
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.inner.set(service, account, secret)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.inner.delete(service, account)
    }
}

fn click_chat(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let button = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        dock.rail_button_bounds(DockPanel::Chat).expect("drawn")
    });
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn chat_is_on_the_rail_from_the_start(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened_on_repo(cx);
    let base = [
        DockPanel::Explorer,
        DockPanel::Search,
        DockPanel::SourceControl,
        DockPanel::Chat,
    ];
    assert_eq!(rail(&workbench, cx), base);
    let icons = workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).rail_icons());
    assert_eq!(icons.len(), base.len() + 1, "the Terminal action follows");
    assert_eq!(icons[3], Icon::MessageSquare);
    assert_eq!(icons[4], Icon::FileTerminal);

    dispatch("git.file_history", cx);
    dispatch("nav.find_references", cx);
    let after = rail(&workbench, cx);
    assert!(after.contains(&DockPanel::History), "{after:?}");
    assert!(after.contains(&DockPanel::References), "{after:?}");
    assert_eq!(after.iter().position(|p| *p == DockPanel::Chat), Some(3));
}

/// A workbench on a git repository with a committed `a.rs`, showing it.
fn opened_on_repo(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    for args in [
        &["init", "-q"][..],
        &["add", "."],
        &["commit", "-q", "-m", "init"],
    ] {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((root.join("a.rs"), Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

#[gpui::test]
fn clicking_chat_on_the_rail_shows_it_focused(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    click_chat(&workbench, cx);
    assert_eq!(shown(&workbench, cx), (true, DockPanel::Chat));
    let panel = chat(&workbench, cx).expect("made with the dock");
    assert!(composer_focused(&panel, cx));

    click_chat(&workbench, cx);
    assert!(!shown(&workbench, cx).0);
}

#[gpui::test]
fn the_key_is_not_read_until_the_chat_shows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(Counting {
        reads: reads.clone(),
        inner: MemoryStore::new(),
    });
    (store
        .inner
        .set("dev.tenmoku.tenmoku", "chat.anthropic", "k"))
    .unwrap();
    cx.update(|cx| {
        cx.set_global(KeySource {
            env: None,
            store: store.clone(),
        });
    });
    let (workbench, cx) = opened(cx, dir.path());
    let _made = scripted(cx);
    for _ in 0..3 {
        draw(cx);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);

    click_chat(&workbench, cx);
    cx.run_until_parked();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let panel: Entity<ChatPanel> = chat(&workbench, cx).unwrap();
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.key_origin()),
        Some(KeyOrigin::Keychain)
    );
}
