//! The chat panel's kept chats: per folder under a temporary state
//! directory, reopened, listed, deleted, and memory-only in an untrusted
//! folder. The provider is a [`ScriptedProvider`].

use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime},
};

use fe_chat::{ChatEvent, ChatProvider, ChatStore, Effort, ScriptedProvider, StopReason};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::SendMessage;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;

use super::{chat, dispatch, dispatch_in, draw, opened, set_source};
use crate::{
    Launch, Workbench,
    chat_panel::{ChatPanel, ChatStateDir, chats_dir},
    tests::open,
};

/// A finished reply of `text`.
fn reply(text: &str) -> Vec<ChatEvent> {
    let block = serde_json::json!({"type": "text", "text": text});
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: fe_chat::BlockKind::Text,
        },
        ChatEvent::BlockFinished { index: 0, block },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

/// Has the window keep its chats in `state`, with a key and a provider that
/// answers every turn, and `chat.open` shown; trusts the root or not.
fn ready(
    workbench: &Entity<Workbench>,
    state: &Path,
    trusted: Option<bool>,
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
    cx.update(|_, cx| {
        cx.set_global(ChatStateDir(Some(state.to_owned())));
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(|_: &ApiKey, _| {
                let replies = (0..6).map(|_| reply("Sure."));
                Arc::new(ScriptedProvider::new(replies)) as Arc<dyn ChatProvider>
            }),
        );
    });
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    if let Some(trusted) = trusted {
        workbench.update(cx, |workbench, cx| workbench.set_root_trust(trusted, cx));
        draw(cx);
    }
    dispatch("chat.open", cx);
    chat(workbench, cx).expect("made with the dock")
}

/// Sends `question` and lets the turn finish and be saved.
fn ask(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text(question, cx));
    cx.update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
    draw(cx);
    draw(cx);
}

fn shown(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> fe_chat::Conversation {
    panel.read_with(cx, |panel, cx| panel.view().read(cx).conversation().clone())
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    files.sort();
    files
}

fn age(path: &Path, seconds: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
        .unwrap();
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("drawn");
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
}

#[gpui::test]
fn the_folders_last_chat_reopens(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let canonical = fs::canonicalize(root.path()).unwrap();
    let dir = state
        .path()
        .join("chats")
        .join(kiln::core::layout::layout_key(&canonical));
    assert_eq!(chats_dir(state.path(), Some(root.path())), dir);

    let id = {
        let (workbench, cx) = opened(cx, root.path());
        let panel = ready(&workbench, state.path(), Some(true), cx);
        assert!(shown(&panel, cx).turns.is_empty());
        ask(&panel, "How do I start?", cx);
        let chat = shown(&panel, cx);
        assert_eq!(files(&dir), [dir.join(format!("{}.json", chat.id))]);

        // Settings the chat was made with are kept with it.
        let store = ChatStore::new(&dir);
        let mut saved = store.load(chat.id).unwrap();
        (saved.model, saved.effort) = ("claude-kept-model".to_owned(), Effort::Low);
        store.save(&saved).unwrap();
        chat.id
    };

    let (second, cx) = opened(&mut app, root.path());
    let panel = ready(&second, state.path(), Some(true), cx);
    let reopened = shown(&panel, cx);
    assert_eq!(reopened.id, id);
    assert_eq!(reopened.title, "How do I start?");
    assert_eq!(reopened.turns.len(), 1);
    assert_eq!(reopened.model, "claude-kept-model");
    assert_eq!(reopened.effort, Effort::Low);

    // A window with no folder has its own.
    let launch = Launch {
        root: None,
        file: None,
    };
    let (bare, cx, _) = open(&mut app, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&bare.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(
        bare.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned)),
        None
    );
    let panel = ready(&bare, state.path(), None, cx);
    assert!(shown(&panel, cx).turns.is_empty());
    ask(&panel, "No folder here", cx);
    let none = state.path().join("chats/no-folder");
    assert_eq!(files(&none).len(), 1);
}

#[gpui::test]
fn history_lists_and_delete_removes(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask(&panel, "alpha question", cx);
    let alpha = shown(&panel, cx).id;
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    ask(&panel, "beta question", cx);
    let beta = shown(&panel, cx).id;
    age(&dir.join(format!("{alpha}.json")), 600);

    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(vec![
            "beta question".to_owned(),
            "alpha question".to_owned()
        ])
    );
    assert!(cx.debug_bounds("chat-history-row-1").is_some());

    // Picking one opens it.
    click("chat-history-row-1", cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.history_titles()), None);
    assert_eq!(shown(&panel, cx).id, alpha);

    // Delete asks first; Cancel keeps it.
    dispatch_in("chat.delete", &[crate::KEY_CONTEXT, "chat"], cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(shown(&panel, cx).id, alpha);
    assert!(dir.join(format!("{alpha}.json")).exists());

    dispatch_in("chat.delete", &[crate::KEY_CONTEXT, "chat"], cx);
    cx.simulate_prompt_answer("Delete");
    draw(cx);
    assert!(!dir.join(format!("{alpha}.json")).exists());
    assert!(dir.join(format!("{beta}.json")).exists());
    let now = shown(&panel, cx);
    assert!(now.turns.is_empty());
    assert_ne!(now.id, alpha);
}

#[gpui::test]
fn untrusted_folders_keep_chats_in_memory_only(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(false), cx);

    ask(&panel, "secret plans", cx);
    assert!(!state.path().join("chats").exists());
    assert_eq!(shown(&panel, cx).turns.len(), 1);

    // A new panel for the root reads nothing.
    {
        let (second, cx) = opened(&mut app, root.path());
        let other = ready(&second, state.path(), Some(false), cx);
        assert!(shown(&other, cx).turns.is_empty());
    }

    // History is this window's.
    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(vec!["secret plans".to_owned()])
    );
    click("chat-history-row-0", cx);
    assert_eq!(shown(&panel, cx).title, "secret plans");

    // Deleting drops it.
    dispatch_in("chat.delete", &[crate::KEY_CONTEXT, "chat"], cx);
    cx.simulate_prompt_answer("Delete");
    draw(cx);
    assert!(shown(&panel, cx).turns.is_empty());
    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(Vec::new())
    );
    click("chat-history", cx);
    assert!(!state.path().join("chats").exists());

    // Once trusted, the next finished turn is written.
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    ask(&panel, "now it may be kept", cx);
    let id = shown(&panel, cx).id;
    let file = dir.join(format!("{id}.json"));
    assert!(file.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[gpui::test]
fn memory_history_puts_the_chat_just_finished_first(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(false), cx);

    ask(&panel, "alpha question", cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    ask(&panel, "beta question", cx);

    // Reopen the older one and go on with it.
    click("chat-history", cx);
    click("chat-history-row-1", cx);
    assert_eq!(shown(&panel, cx).title, "alpha question");
    ask(&panel, "more", cx);

    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(vec![
            "alpha question".to_owned(),
            "beta question".to_owned()
        ])
    );
}

/// Sends `question` and lets the turn finish, but not the chat store's
/// worker run, so the save stays queued.
fn ask_queued(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text(question, cx));
    cx.update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn worker(cx: &mut VisualTestContext) -> std::sync::Arc<super::super::chat_store_worker::Worker> {
    cx.update(|_, cx| super::super::chat_store_worker::Worker::of(cx))
}

fn set_root(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(path.to_owned(), window, cx)
        })
    });
    draw(cx);
}

#[gpui::test]
fn switching_folders_keeps_each_chat_with_its_own(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (dir_a, dir_b) = (
        chats_dir(state.path(), Some(a.path())),
        chats_dir(state.path(), Some(b.path())),
    );
    // B already has a chat.
    let earlier = {
        let mut chat = fe_chat::Conversation::new(&fe_chat::ChatSettings::default());
        chat.begin_turn(
            vec![serde_json::json!({"type": "text", "text": "bee chat"})],
            Vec::new(),
        );
        chat
    };
    ChatStore::new(&dir_b).save(&earlier).unwrap();

    let (workbench, cx) = opened(cx, a.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask(&panel, "from A", cx);
    let in_a = shown(&panel, cx).id;
    assert_eq!(files(&dir_a).len(), 1);

    // Its chat stays in A; B's newest opens once B is trusted.
    set_root(&workbench, b.path(), cx);
    assert!(shown(&panel, cx).turns.is_empty());
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    assert_eq!(shown(&panel, cx).id, earlier.id);

    ask(&panel, "from B", cx);
    assert_eq!(files(&dir_b).len(), 1, "B's own chat continues");
    assert_eq!(files(&dir_a), [dir_a.join(format!("{in_a}.json"))]);

    // An untrusted folder's chat never reaches the trusted one's store.
    set_root(&workbench, a.path(), cx);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    draw(cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    ask(&panel, "kept in memory", cx);
    assert_eq!(files(&dir_a).len(), 1);
    set_root(&workbench, b.path(), cx);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    assert_eq!(files(&dir_b).len(), 1);
    assert_eq!(files(&dir_a).len(), 1);
}

#[gpui::test]
fn trust_revoked_before_a_queued_save_runs_writes_nothing(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask_queued(&panel, "first", cx);
    ask_queued(&panel, "second", cx);
    assert_eq!(shown(&panel, cx).turns.len(), 2);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    cx.run_until_parked();

    // The worker runs both saves now: neither touches the filesystem.
    assert_eq!(worker(cx).run_pending(), 2);
    assert!(!state.path().join("chats").exists());
}

#[gpui::test]
fn quitting_runs_what_was_accepted_once(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask_queued(&panel, "in flight", cx);
    let id = shown(&panel, cx).id;
    assert!(files(&dir).is_empty());
    let worker = worker(cx);
    worker.shutdown();
    assert_eq!(files(&dir), [dir.join(format!("{id}.json"))]);

    // Closed: later requests are refused, and a second quit does nothing.
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    ask(&panel, "too late", cx);
    worker.shutdown();
    assert_eq!(files(&dir).len(), 1);
    drop(panel);
    assert_eq!(files(&dir).len(), 1);
}

#[gpui::test]
fn history_listed_right_after_a_save_includes_it(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask_queued(&panel, "just finished", cx);
    // The save has not run; the list is queued behind it.
    panel.update(cx, |panel, cx| panel.toggle_history(cx));
    draw(cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(vec!["just finished".to_owned()])
    );
}

fn target_of(state: &Path, root: &Path) -> super::super::chat_store_worker::Target {
    let canonical = fs::canonicalize(root).unwrap();
    super::super::chat_store_worker::Target {
        dir: chats_dir(state, Some(&canonical)),
        root: Some(canonical),
    }
}

#[gpui::test]
fn what_was_asked_before_leaving_a_folder_is_refused_and_the_chat_written_once(
    cx: &mut TestAppContext,
) {
    let state = tempfile::tempdir().unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir_a = chats_dir(state.path(), Some(a.path()));
    let (workbench, cx) = opened(cx, a.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask_queued(&panel, "queued in A", cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(b.path().to_owned(), window, cx)
        })
    });
    cx.run_until_parked();

    // The save asked for under A's hold is refused; the chat is written
    // from the departure, by a request of its own.
    let worker = worker(cx);
    assert_eq!(worker.run_pending(), 2);
    assert_eq!(worker.refused(), 1);
    assert_eq!(files(&dir_a).len(), 1);
}

#[gpui::test]
fn trust_revoked_from_another_window_refuses_delayed_writes(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (workbench, cx) = opened(cx, a.path());
    let (other, other_cx) = opened(&mut app, a.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask_queued(&panel, "queued in A", cx);

    // The window leaves A, then another window, showing A, revokes it.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(b.path().to_owned(), window, cx)
        })
    });
    cx.run_until_parked();
    other.update(other_cx, |workbench, cx| {
        workbench.set_root_trust(false, cx)
    });
    other_cx.run_until_parked();

    let worker = worker(other_cx);
    worker.run_pending();
    assert_eq!(worker.refused(), 2);
    assert!(!state.path().join("chats").exists());
}

#[gpui::test]
fn deleting_after_a_root_switch_deletes_where_the_chat_began(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (dir_a, dir_b) = (
        chats_dir(state.path(), Some(a.path())),
        chats_dir(state.path(), Some(b.path())),
    );
    let (workbench, cx) = opened(cx, a.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask(&panel, "in A", cx);
    assert_eq!(files(&dir_a).len(), 1);

    dispatch_in("chat.delete", &[crate::KEY_CONTEXT, "chat"], cx);
    assert!(cx.has_pending_prompt());
    set_root(&workbench, b.path(), cx);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    cx.simulate_prompt_answer("Delete");
    draw(cx);

    assert!(files(&dir_a).is_empty());
    assert!(files(&dir_b).is_empty());
}

#[cfg(unix)]
#[gpui::test]
fn a_root_alias_retargeted_before_a_write_runs_changes_nothing(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (x, y) = (scratch.path().join("x"), scratch.path().join("y"));
    fs::create_dir(&x).unwrap();
    fs::create_dir(&y).unwrap();
    let link = scratch.path().join("link");
    std::os::unix::fs::symlink(&x, &link).unwrap();
    let (dir_x, dir_y) = (
        chats_dir(state.path(), Some(&x)),
        chats_dir(state.path(), Some(&y)),
    );

    let (workbench, cx) = opened(cx, &link);
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask_queued(&panel, "through the alias", cx);

    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&y, &link).unwrap();
    worker(cx).run_pending();

    assert_eq!(files(&dir_x).len(), 1, "written where it was asked for");
    assert!(files(&dir_y).is_empty());
}

#[gpui::test]
fn reads_are_checked_where_they_run(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask(&panel, "kept", cx);
    let id = shown(&panel, cx).id;

    let worker = worker(cx);
    let token = || worker.token(&target_of(state.path(), root.path())).unwrap();
    let (list, load, newest) = (
        worker.list(token()),
        worker.load(token(), id),
        worker.newest(token()),
    );
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    cx.run_until_parked();
    worker.run_pending();

    use super::super::chat_store_worker::WorkerError::Stale;
    assert!(matches!(futures::executor::block_on(list), Err(Stale)));
    assert!(matches!(futures::executor::block_on(load), Err(Stale)));
    assert!(matches!(futures::executor::block_on(newest), Err(Stale)));
}

#[gpui::test]
fn chats_finished_untrusted_are_listed_once_trusted_without_being_written(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(false), cx);
    ask(&panel, "while untrusted", cx);
    let id = shown(&panel, cx).id;

    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(vec!["while untrusted".to_owned()])
    );
    assert!(files(&dir).is_empty(), "not written early");

    // Opened, and continued, it is written like any other.
    click("chat-history-row-0", cx);
    assert_eq!(shown(&panel, cx).id, id);
    ask(&panel, "and now trusted", cx);
    assert_eq!(files(&dir), [dir.join(format!("{id}.json"))]);
}

/// Runs a panel method that needs the window, with no draw after it, so
/// the chat store's worker stays unrun.
fn with_window(
    panel: &Entity<ChatPanel>,
    cx: &mut VisualTestContext,
    run: impl FnOnce(&mut ChatPanel, &mut gpui::Window, &mut gpui::Context<ChatPanel>),
) {
    cx.update(|window, cx| panel.update(cx, |panel, cx| run(panel, window, cx)));
    cx.run_until_parked();
}

#[gpui::test]
fn a_queued_save_survives_new_chat_in_the_same_folder(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask_queued(&panel, "finished just before", cx);
    let id = shown(&panel, cx).id;
    with_window(&panel, cx, |panel, window, cx| panel.start_chat(window, cx));
    assert_ne!(shown(&panel, cx).id, id);

    let worker = worker(cx);
    worker.run_pending();
    assert_eq!(worker.refused(), 0);
    assert_eq!(files(&dir), [dir.join(format!("{id}.json"))]);
}

#[gpui::test]
fn a_queued_save_survives_opening_a_memory_chat(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(false), cx);
    ask(&panel, "kept in memory", cx);
    let memory_id = shown(&panel, cx).id;

    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    ask_queued(&panel, "to disk", cx);
    let disk_id = shown(&panel, cx).id;
    with_window(&panel, cx, |panel, window, cx| {
        panel.open_history_chat(memory_id, window, cx)
    });

    // The load queues behind the save; the chat was never written, so the
    // window's own copy opens.
    let worker = worker(cx);
    worker.run_pending();
    cx.run_until_parked();
    assert_eq!(shown(&panel, cx).id, memory_id);
    assert_eq!(worker.refused(), 0);
    assert_eq!(files(&dir), [dir.join(format!("{disk_id}.json"))]);
}

#[gpui::test]
fn opening_another_window_revokes_nothing(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask_queued(&panel, "queued", cx);

    // The new window's trust is unresolved until it is read.
    let (_second, second_cx) = opened(&mut app, root.path());
    second_cx.run_until_parked();
    let worker = worker(second_cx);
    worker.run_pending();
    assert_eq!(worker.refused(), 0);
    assert_eq!(files(&dir).len(), 1);
}

#[gpui::test]
fn windows_chats_stay_listed_across_trust_transitions(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(false), cx);
    ask(&panel, "one", cx);

    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    ask_queued(&panel, "two", cx);
    // Revoked before the save of "two" runs: it is refused, never written.
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    cx.run_until_parked();
    let worker = worker(cx);
    worker.run_pending();
    assert_eq!(worker.refused(), 1);
    assert!(!state.path().join("chats").exists());

    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(vec!["two".to_owned(), "one".to_owned()])
    );
}

#[gpui::test]
fn an_ancestors_revocation_refuses_a_departed_folders_requests(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let parent_path = fs::canonicalize(parent.path()).unwrap();
    let (a, b) = (parent_path.join("a"), tempfile::tempdir().unwrap());
    fs::create_dir(&a).unwrap();
    let (workbench, cx) = opened(cx, &a);
    // A's trust is its parent's.
    cx.update(|_, cx| kiln_shell::trust::decide(&parent_path, true, cx));
    let panel = ready(&workbench, state.path(), None, cx);
    draw(cx);
    ask(&panel, "in A", cx);
    ask_queued(&panel, "more in A", cx);
    // A queued list and delete, then the window leaves A.
    with_window(&panel, cx, |panel, _, cx| panel.toggle_history(cx));
    let stack = [crate::KEY_CONTEXT, "chat"]
        .iter()
        .fold(fe_command::ContextStack::new(), |stack, context| {
            stack.with(fe_command::ContextFrame::new(*context))
        });
    let run = fe_gpui::commands::RunCommand(
        fe_command::Invocation::of("chat.delete")
            .unwrap()
            .context(stack),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(b.path().to_owned(), window, cx)
        })
    });
    cx.run_until_parked();

    // No window shows A when its parent is revoked.
    cx.update(|_, cx| kiln_shell::trust::decide(&parent_path, false, cx));
    cx.run_until_parked();
    let worker = worker(cx);
    worker.run_pending();
    assert!(worker.refused() >= 3);
    assert_eq!(
        files(&chats_dir(state.path(), Some(&a))).len(),
        1,
        "only what was written before"
    );
}

#[cfg(unix)]
#[gpui::test]
fn history_after_a_retargeted_alias_reads_nothing_of_the_new_folder(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (x, y) = (scratch.path().join("x"), scratch.path().join("y"));
    fs::create_dir(&x).unwrap();
    fs::create_dir(&y).unwrap();
    let link = scratch.path().join("link");
    std::os::unix::fs::symlink(&x, &link).unwrap();
    // The untrusted folder has a chat of its own on disk.
    let mut theirs = fe_chat::Conversation::new(&fe_chat::ChatSettings::default());
    theirs.begin_turn(
        vec![serde_json::json!({"type": "text", "text": "Y's secret"})],
        Vec::new(),
    );
    ChatStore::new(chats_dir(state.path(), Some(&y)))
        .save(&theirs)
        .unwrap();

    let (workbench, cx) = opened(cx, &link);
    let panel = ready(&workbench, state.path(), Some(true), cx);
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&y, &link).unwrap();

    click("chat-history", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.history_titles()),
        Some(Vec::new())
    );
}

#[gpui::test]
fn history_reopens_what_another_window_wrote(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);

    // A finishes chat C and starts another.
    ask(&a_panel, "first", a_cx);
    let id = shown(&a_panel, a_cx).id;
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], a_cx);

    // B reopens C and adds a turn.
    with_window(&b_panel, b_cx, |panel, _, cx| panel.toggle_history(cx));
    draw(b_cx);
    click("chat-history-row-0", b_cx);
    assert_eq!(shown(&b_panel, b_cx).id, id);
    ask(&b_panel, "second, from B", b_cx);

    // A reopens C through its History: B's turn is there, and A's own
    // goes on from it.
    click("chat-history", a_cx);
    click("chat-history-row-0", a_cx);
    assert_eq!(shown(&a_panel, a_cx).id, id);
    assert_eq!(shown(&a_panel, a_cx).turns.len(), 2);
    ask(&a_panel, "third, from A", a_cx);

    let saved = ChatStore::new(&dir).load(id).unwrap();
    assert_eq!(saved.turns.len(), 3);
}

#[gpui::test]
fn a_stale_shown_chat_is_not_written_back(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let (root, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);

    // A finishes C and keeps it shown; B continues C.
    ask(&a_panel, "first", a_cx);
    let id = shown(&a_panel, a_cx).id;
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    ask(&b_panel, "second, from B", b_cx);
    assert_eq!(ChatStore::new(&dir).load(id).unwrap().turns.len(), 2);

    // Selecting the shown chat in A's History loads B's turn, and writes
    // nothing back of A's older copy.
    click("chat-history", a_cx);
    click("chat-history-row-0", a_cx);
    assert_eq!(shown(&a_panel, a_cx).turns.len(), 2);
    assert_eq!(ChatStore::new(&dir).load(id).unwrap().turns.len(), 2);

    // Nor does A leaving the folder with a stale copy: B goes on once more.
    ask(&b_panel, "third, from B", b_cx);
    set_root(&a, elsewhere.path(), a_cx);
    assert_eq!(ChatStore::new(&dir).load(id).unwrap().turns.len(), 3);
}

#[gpui::test]
fn a_newer_memory_continuation_beats_an_older_file(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);

    ask(&panel, "turn one", cx);
    let id = shown(&panel, cx).id;
    // Revoked, so the second turn's save is refused.
    ask_queued(&panel, "turn two", cx);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    cx.run_until_parked();
    worker(cx).run_pending();
    assert_eq!(ChatStore::new(&dir).load(id).unwrap().turns.len(), 1);

    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    click("chat-history", cx);
    click("chat-history-row-0", cx);
    assert_eq!(shown(&panel, cx).id, id);
    assert_eq!(shown(&panel, cx).turns.len(), 2, "turn two survives");
}

fn turn_texts(chat: &fe_chat::Conversation) -> Vec<String> {
    (chat.turns.iter())
        .map(|turn| serde_json::to_string(&turn.user_content).unwrap())
        .collect()
}

fn has_text(chat: &fe_chat::Conversation, text: &str) -> bool {
    turn_texts(chat).iter().any(|turn| turn.contains(text))
}

#[gpui::test]
fn equal_length_continuations_in_two_windows_leave_one_and_a_copy(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);

    ask(&a_panel, "first", a_cx);
    let id = shown(&a_panel, a_cx).id;
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    assert_eq!(shown(&b_panel, b_cx).id, id);

    // Each window adds a second turn of its own to the same first turn.
    ask(&a_panel, "A's second", a_cx);
    ask(&b_panel, "B's second", b_cx);

    // The file keeps A's; B's turns were written as a copy by the save
    // itself, and B's chat in view is left as it is.
    let disk = ChatStore::new(&dir).load(id).unwrap();
    assert_eq!(disk.turns.len(), 2);
    assert!(has_text(&disk, "A's second"));
    assert!(!has_text(&disk, "B's second"));
    let still = shown(&b_panel, b_cx);
    assert_eq!(still.id, id);
    assert!(has_text(&still, "B's second"));
    let files = files(&dir);
    assert_eq!(files.len(), 2);
    let copy = (files.iter())
        .filter_map(|file| {
            let id = file
                .file_stem()?
                .to_str()?
                .parse::<fe_chat::ChatId>()
                .ok()?;
            ChatStore::new(&dir).load(id).ok()
        })
        .find(|chat| chat.id != id)
        .unwrap();
    assert_eq!(copy.title, "first (copy)");
    assert!(has_text(&copy, "B's second"));

    // B's History lists it.
    click("chat-history", b_cx);
    let titles = b_panel.read_with(b_cx, |panel, _| panel.history_titles());
    assert!(titles.unwrap().contains(&"first (copy)".to_owned()));
}

#[gpui::test]
fn a_rejected_continuation_does_not_lose_to_an_equal_length_file(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);

    ask(&a_panel, "turn one", a_cx);
    let id = shown(&a_panel, a_cx).id;
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);

    // A's second turn is refused when trust is revoked before its save runs.
    ask_queued(&a_panel, "A's rejected two", a_cx);
    a.update(a_cx, |workbench, cx| workbench.set_root_trust(false, cx));
    a_cx.run_until_parked();
    worker(a_cx).run_pending();
    a.update(a_cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(a_cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], a_cx);

    // B goes on from the persisted first turn: a file of the same length.
    ask(&b_panel, "B's two", b_cx);
    let disk = ChatStore::new(&dir).load(id).unwrap();
    assert_eq!(disk.turns.len(), 2);
    assert!(has_text(&disk, "B's two"));

    // A reopens C: the file stands, A's continuation is kept as a copy.
    click("chat-history", a_cx);
    let titles = a_panel.read_with(a_cx, |panel, _| panel.history_titles());
    assert!(titles.is_some());
    let row = {
        let rows = titles.unwrap();
        rows.iter().position(|title| title == "turn one").unwrap()
    };
    let selector: &'static str = Box::leak(format!("chat-history-row-{row}").into_boxed_str());
    click(selector, a_cx);
    let shown_now = shown(&a_panel, a_cx);
    assert_eq!(shown_now.id, id);
    assert!(has_text(&shown_now, "B's two"));
    assert!(!has_text(&shown_now, "A's rejected two"));
    let copies: Vec<_> = (files(&dir).iter())
        .filter_map(|file| {
            let id = file
                .file_stem()?
                .to_str()?
                .parse::<fe_chat::ChatId>()
                .ok()?;
            ChatStore::new(&dir).load(id).ok()
        })
        .filter(|chat| chat.title == "turn one (copy)")
        .collect();
    assert_eq!(copies.len(), 1);
    assert!(has_text(&copies[0], "A's rejected two"));
}

#[gpui::test]
fn an_equal_length_rewrite_replaces_a_clean_shown_chat(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask(&panel, "first", cx);
    ask(&panel, "second", cx);
    let id = shown(&panel, cx).id;

    // The file is rewritten, with turns of the same count.
    let store = ChatStore::new(&dir);
    let mut rewritten = store.load(id).unwrap();
    rewritten.turns[1].user_content =
        vec![serde_json::json!({"type": "text", "text": "rewritten"})];
    store.save(&rewritten).unwrap();

    click("chat-history", cx);
    click("chat-history-row-0", cx);
    let now = shown(&panel, cx);
    assert!(has_text(&now, "rewritten"));
    assert!(!has_text(&now, "second"));
}

#[gpui::test]
fn a_diverged_save_queued_when_the_window_closes_still_writes_its_copy(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);
    ask(&a_panel, "first", a_cx);
    let id = shown(&a_panel, a_cx).id;
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    ask(&a_panel, "A's second", a_cx);

    // B's save is queued; B's window closes before it runs.
    ask_queued(&b_panel, "B's second", b_cx);
    b_cx.update(|window, _| window.remove_window());
    b_cx.run_until_parked();
    drop(b_panel);
    worker(a_cx).run_pending();

    let copies: Vec<_> = (files(&dir).iter())
        .filter_map(|file| {
            let copy = file
                .file_stem()?
                .to_str()?
                .parse::<fe_chat::ChatId>()
                .ok()?;
            ChatStore::new(&dir).load(copy).ok()
        })
        .filter(|chat| chat.id != id)
        .collect();
    assert_eq!(copies.len(), 1);
    assert!(has_text(&copies[0], "B's second"));
    assert!(!has_text(
        &ChatStore::new(&dir).load(id).unwrap(),
        "B's second"
    ));
}

#[gpui::test]
fn quitting_with_a_diverged_save_queued_writes_its_copy(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);
    ask(&a_panel, "first", a_cx);
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    ask(&a_panel, "A's second", a_cx);
    ask_queued(&b_panel, "B's second", b_cx);

    worker(a_cx).shutdown();

    assert_eq!(files(&dir).len(), 2);
}

#[gpui::test]
fn a_diverged_reply_after_another_turn_leaves_the_turn_and_draft(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);
    ask(&a_panel, "first", a_cx);
    let id = shown(&a_panel, a_cx).id;
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    ask(&a_panel, "A's second", a_cx);

    // B's save is queued, B goes on with another turn and a draft, and only
    // then does the worker answer.
    ask_queued(&b_panel, "B's second", b_cx);
    ask_queued(&b_panel, "B's third", b_cx);
    let view = b_panel.read_with(b_cx, |panel, _| panel.view().clone());
    view.update(b_cx, |view, cx| view.set_composer_text("a draft", cx));
    worker(b_cx).run_pending();
    b_cx.run_until_parked();

    let now = shown(&b_panel, b_cx);
    assert_eq!(now.id, id);
    assert_eq!(now.turns.len(), 3);
    assert!(has_text(&now, "B's third"));
    let draft = view.read_with(b_cx, |view, cx| view.composer_text(cx));
    assert_eq!(draft, "a draft");
    // Both of B's turns are in the one copy, written in order.
    let copies: Vec<_> = (files(&dir).iter())
        .filter_map(|file| {
            let copy = file
                .file_stem()?
                .to_str()?
                .parse::<fe_chat::ChatId>()
                .ok()?;
            ChatStore::new(&dir).load(copy).ok()
        })
        .filter(|chat| chat.id != id)
        .collect();
    assert_eq!(copies.len(), 1);
    assert!(has_text(&copies[0], "B's third"));
}

#[gpui::test]
fn a_rejected_delete_is_reported(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (workbench, cx) = opened(cx, root.path());
    let panel = ready(&workbench, state.path(), Some(true), cx);
    ask(&panel, "keep me", cx);

    let stack = [crate::KEY_CONTEXT, "chat"]
        .iter()
        .fold(fe_command::ContextStack::new(), |stack, context| {
            stack.with(fe_command::ContextFrame::new(*context))
        });
    let run = fe_gpui::commands::RunCommand(
        fe_command::Invocation::of("chat.delete")
            .unwrap()
            .context(stack),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    // Trust is revoked before the delete runs: it is refused, and said so.
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    cx.run_until_parked();
    worker(cx).run_pending();
    cx.run_until_parked();

    assert_eq!(files(&dir).len(), 1, "still on disk");
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let messages: Vec<String> = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    });
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Couldn't delete the chat")),
        "{messages:?}"
    );
}

/// The row of History titled `title`, clicked.
fn pick(title: &str, panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) {
    click("chat-history", cx);
    let titles = panel
        .read_with(cx, |panel, _| panel.history_titles())
        .unwrap();
    let row = titles.iter().position(|t| t == title).unwrap();
    let selector: &'static str = Box::leak(format!("chat-history-row-{row}").into_boxed_str());
    click(selector, cx);
}

fn chats_in(dir: &Path) -> Vec<fe_chat::Conversation> {
    (files(dir).iter())
        .filter_map(|file| {
            let id = file
                .file_stem()?
                .to_str()?
                .parse::<fe_chat::ChatId>()
                .ok()?;
            ChatStore::new(dir).load(id).ok()
        })
        .collect()
}

#[gpui::test]
fn a_copy_that_diverges_is_copied_again(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);
    ask(&a_panel, "first", a_cx);
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    ask(&a_panel, "A2", a_cx);
    ask(&b_panel, "B2", b_cx);

    // A opens B's copy and goes on in it; then B goes on too.
    pick("first (copy)", &a_panel, a_cx);
    ask(&a_panel, "A on the copy", a_cx);
    ask(&b_panel, "B3", b_cx);

    let all = chats_in(&dir);
    assert_eq!(all.len(), 3);
    let with = |text: &str| all.iter().filter(|chat| has_text(chat, text)).count();
    assert_eq!(with("B3"), 1, "B's new turn reached disk");
    assert_eq!(with("A on the copy"), 1);
    assert!(
        all.iter()
            .all(|chat| !(has_text(chat, "B3") && has_text(chat, "A on the copy")))
    );
    assert!(has_text(&shown(&b_panel, b_cx), "B3"));
}

#[gpui::test]
fn a_persisted_copy_reopens_without_another_copy(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = chats_dir(state.path(), Some(root.path()));
    let (a, a_cx) = opened(cx, root.path());
    let (b, b_cx) = opened(&mut app, root.path());
    let a_panel = ready(&a, state.path(), Some(true), a_cx);
    let b_panel = ready(&b, state.path(), None, b_cx);
    ask(&a_panel, "first", a_cx);
    click("chat-history", b_cx);
    click("chat-history-row-0", b_cx);
    ask(&a_panel, "A2", a_cx);
    ask(&b_panel, "B2", b_cx);
    assert_eq!(chats_in(&dir).len(), 2);

    // B goes on in its chat; its turn goes to the copy. Reopening the copy
    // loads its file, with both turns, and adds no third chat.
    ask(&b_panel, "B3", b_cx);
    assert_eq!(chats_in(&dir).len(), 2);
    pick("first (copy)", &b_panel, b_cx);
    let copy = shown(&b_panel, b_cx);
    assert!(has_text(&copy, "B2") && has_text(&copy, "B3"));
    assert_eq!(chats_in(&dir).len(), 2);
}
