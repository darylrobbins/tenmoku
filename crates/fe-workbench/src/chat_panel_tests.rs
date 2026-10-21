//! The Chat panel in a headless window: made and shown by `chat.open`, its
//! key from a [`KeySource`] (a `MemoryStore` for the keychain), new chats
//! from `[chat]` in kiln's settings service, and the first-run notice in a
//! temporary state directory. The provider is a [`ScriptedProvider`]
//! from the panel's test factory: no network.

use std::{
    cell::RefCell,
    fs,
    path::Path,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context as TaskContext, Poll},
};

use fe_chat::{
    ChatEvent, ChatProvider, ChatRequest, ChatStream, ContextItem, ContextKind, Effort,
    ScriptedProvider, StopReason,
};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::{ChatOptions, RetryTurn, SendMessage};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::SettingsStore;
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    command::Capability,
    core::secrets::{MemoryStore, SecretError, SecretStore as _},
    platform::config::{BatchSender, batch_channel},
};

use super::{CHAT_CAPABILITIES, ChatPanel, ChatStateDir, KeyOrigin, KeySource, Keychain};
use crate::{DockPanel, Launch, Workbench, tests::open};

/// A workbench on an empty folder, focused as the app focuses it.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs command `id` as an unbound key would, in `contexts`.
fn dispatch_in(id: &str, contexts: &[&str], cx: &mut VisualTestContext) {
    let stack = (contexts.iter()).fold(ContextStack::new(), |stack, context| {
        stack.with(ContextFrame::new(*context))
    });
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Runs workbench command `id`.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    dispatch_in(id, &[crate::KEY_CONTEXT], cx);
}

/// The rail's panels, top to bottom.
fn rail(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<DockPanel> {
    workbench.read_with(cx, |workbench, cx| {
        let state = workbench.dock().read(cx).state();
        (state.panel_ids().into_iter())
            .filter(|id| state.on_rail(*id, cx))
            .map(|id| {
                let panels = [DockPanel::Explorer, DockPanel::Search];
                (panels.into_iter())
                    .chain([
                        DockPanel::SourceControl,
                        DockPanel::Chat,
                        DockPanel::References,
                        DockPanel::Problems,
                    ])
                    .chain([DockPanel::History])
                    .find(|panel| panel.id() == id)
                    .expect("a sidebar panel")
            })
            .collect()
    })
}

fn chat(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<ChatPanel>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned()
    })
}

/// Whether the dock shows, and the panel it shows.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        (dock.is_visible(), dock.active())
    })
}

/// Whether the chat's composer has focus.
fn composer_focused(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let view = panel.read(cx).view().read(cx);
        view.composer().focus_handle(cx).is_focused(window)
    })
}

/// The keys and options the panel made providers with, oldest first.
type Made = Rc<RefCell<Vec<(String, ChatOptions)>>>;

/// Has the panels make scripted providers, noting each key and options.
fn scripted(cx: &mut VisualTestContext) -> Made {
    let made = Made::default();
    let noted = made.clone();
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |key: &ApiKey, options| {
                noted.borrow_mut().push((key.expose().to_owned(), options));
                Arc::new(ScriptedProvider::new([])) as Arc<dyn ChatProvider>
            }),
        );
    });
    made
}

fn last_key(made: &Made) -> Option<String> {
    made.borrow().last().map(|(key, _)| key.clone())
}

fn set_source(env: Option<&str>, store: &Arc<MemoryStore>, cx: &mut VisualTestContext) {
    let source = KeySource {
        env: env.map(str::to_owned),
        store: store.clone(),
    };
    cx.update(|_, cx| cx.set_global(source));
}

fn reload_key(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.reload_key(cx));
    draw(cx);
}

#[gpui::test]
fn chat_open_shows_the_panel(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let base = [
        DockPanel::Explorer,
        DockPanel::Search,
        DockPanel::SourceControl,
        DockPanel::Chat,
    ];
    assert_eq!(rail(&workbench, cx), base);
    let panel = chat(&workbench, cx).expect("made with the dock");
    assert!(!shown(&workbench, cx).0 || shown(&workbench, cx).1 != DockPanel::Chat);

    dispatch("chat.open", cx);
    assert_eq!(shown(&workbench, cx), (true, DockPanel::Chat));
    assert!(composer_focused(&panel, cx));
    let with_chat = base;
    assert_eq!(rail(&workbench, cx), with_chat);
    let title = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        dock.state().title(cx).unwrap_or_default().to_string()
    });
    assert_eq!(title, "Chat");

    // Hidden, then from the Explorer: the same panel shows again.
    dispatch("dock.toggle", cx);
    assert!(!shown(&workbench, cx).0);
    dispatch("dock.show_project", cx);
    dispatch("chat.open", cx);
    assert_eq!(shown(&workbench, cx), (true, DockPanel::Chat));
    assert_eq!(chat(&workbench, cx), Some(panel.clone()));
    assert!(composer_focused(&panel, cx));
    assert_eq!(rail(&workbench, cx), with_chat);

    // From inside the panel too.
    dispatch_in("chat.open", &[crate::KEY_CONTEXT, "chat"], cx);
    assert_eq!(chat(&workbench, cx), Some(panel.clone()));
    assert_eq!(shown(&workbench, cx), (true, DockPanel::Chat));
}

#[gpui::test]
fn the_key_comes_from_the_environment_then_the_keychain(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let made = scripted(cx);
    let store = Arc::new(MemoryStore::new());
    (store.set("dev.fastedit.fastedit", "chat.anthropic", "store-key")).unwrap();

    // The environment's first, said in the header.
    set_source(Some("env-key"), &store, cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    let (origin, note) = panel.read_with(cx, |panel, _| (panel.key_origin(), panel.header_note()));
    assert_eq!(origin, Some(KeyOrigin::Environment));
    assert_eq!(note.as_deref(), Some("Key from ANTHROPIC_API_KEY"));
    assert_eq!(last_key(&made).as_deref(), Some("env-key"));
    assert!(cx.debug_bounds("chat-key-note").is_some());
    assert!(cx.debug_bounds("chat-no-key").is_none());

    // With none, the keychain's.
    set_source(None, &store, cx);
    reload_key(&panel, cx);
    let (origin, note) = panel.read_with(cx, |panel, _| (panel.key_origin(), panel.header_note()));
    assert_eq!(origin, Some(KeyOrigin::Keychain));
    assert_eq!(note, None);
    assert_eq!(last_key(&made).as_deref(), Some("store-key"));
    assert!(cx.debug_bounds("chat-key-note").is_none());
    assert!(cx.debug_bounds("chat-no-key").is_none());

    // An empty variable counts as unset.
    set_source(Some(""), &store, cx);
    reload_key(&panel, cx);
    let origin = panel.read_with(cx, |panel, _| panel.key_origin());
    assert_eq!(origin, Some(KeyOrigin::Keychain));

    // With neither, the view's no-key state.
    (store.delete("dev.fastedit.fastedit", "chat.anthropic")).unwrap();
    let keys = made.borrow().len();
    reload_key(&panel, cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.key_origin()), None);
    assert_eq!(made.borrow().len(), keys, "no provider made");
    assert!(cx.debug_bounds("chat-no-key").is_some());

    // A key set since is read again.
    (store.set("dev.fastedit.fastedit", "chat.anthropic", "new-key")).unwrap();
    reload_key(&panel, cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.key_origin()),
        Some(KeyOrigin::Keychain)
    );
    assert_eq!(last_key(&made).as_deref(), Some("new-key"));
    assert!(cx.debug_bounds("chat-no-key").is_none());
}

#[gpui::test]
fn the_keychain_is_read_on_open(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let made = scripted(cx);
    let store = Arc::new(MemoryStore::new());
    (store.set("dev.fastedit.fastedit", "chat.anthropic", "store-key")).unwrap();
    set_source(None, &store, cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.key_origin()),
        Some(KeyOrigin::Keychain)
    );
    assert_eq!(last_key(&made).as_deref(), Some("store-key"));
}

const CHAT: &str = "\
[chat]
model = \"claude-sonnet-5-5\"
effort = \"high\"
show_thinking = true
max_tokens = 4096
fallbacks = false
";

const CHANGED: &str = "\
[chat]
model = \"claude-haiku-5-5\"
effort = \"low\"
show_thinking = false
max_tokens = 2048
fallbacks = true
";

/// kiln's settings service over a user settings file holding `text`, and
/// the sender that reloads it.
fn settings(dir: &Path, text: &str, cx: &mut TestAppContext) -> (std::path::PathBuf, BatchSender) {
    let path = dir.join(fe_config::paths::SETTINGS_FILE);
    fs::write(&path, text).unwrap();
    let store = SettingsStore::load(Some(path.clone()), None);
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(store.into_inner())
        .watch_with(batches);
    cx.update(|cx| assert_eq!(kiln::init(cx, config), []));
    cx.run_until_parked();
    (path, sender)
}

/// The panel's chat's model, effort and thinking, and its options.
fn chat_settings(
    panel: &Entity<ChatPanel>,
    cx: &mut VisualTestContext,
) -> (String, Effort, bool, ChatOptions) {
    panel.read_with(cx, |panel, cx| {
        let conversation = panel.view().read(cx).conversation();
        (
            conversation.model.clone(),
            conversation.effort,
            conversation.show_thinking,
            panel.options(),
        )
    })
}

#[gpui::test]
fn new_chats_take_settings_and_grants(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    let (path, sender) = settings(config.path(), CHAT, cx);
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let made = scripted(cx);
    let store = Arc::new(MemoryStore::new());
    set_source(Some("env-key"), &store, cx);

    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    let first = ChatOptions {
        max_tokens: 4096,
        fallbacks: false,
    };
    let first_chat = ("claude-sonnet-5-5".to_owned(), Effort::High, true, first);
    assert_eq!(chat_settings(&panel, cx), first_chat);
    assert_eq!(made.borrow().last(), Some(&("env-key".to_owned(), first)));

    // A change leaves the open chat as it is.
    fs::write(&path, CHANGED).unwrap();
    sender.send(vec![path.clone()]);
    draw(cx);
    let model = cx.update(|_, cx| {
        let settings = kiln::platform::config::settings::<fe_config::Settings>(cx);
        settings.chat.model.clone()
    });
    assert_eq!(model, "claude-haiku-5-5", "reloaded");
    assert_eq!(chat_settings(&panel, cx), first_chat);

    // A new chat takes it, from the key or the header's button.
    let changed = ChatOptions {
        max_tokens: 2048,
        fallbacks: true,
    };
    let changed_chat = ("claude-haiku-5-5".to_owned(), Effort::Low, false, changed);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    assert_eq!(chat_settings(&panel, cx), changed_chat);
    assert_eq!(made.borrow().last(), Some(&("env-key".to_owned(), changed)));
    assert!(composer_focused(&panel, cx));
    let before = panel.read_with(cx, |panel, cx| panel.view().read(cx).conversation().id);
    let button = cx.debug_bounds("chat-new").expect("drawn");
    cx.simulate_click(button.center(), Default::default());
    draw(cx);
    let after = panel.read_with(cx, |panel, cx| panel.view().read(cx).conversation().id);
    assert_ne!(before, after, "a new chat");
    assert_eq!(chat_settings(&panel, cx), changed_chat);

    // The grants: exactly what the chat declares.
    let grants = panel.read_with(cx, |panel, _| panel.grants().clone());
    let granted: Vec<String> = grants.iter().map(ToString::to_string).collect();
    assert_eq!(granted, CHAT_CAPABILITIES);
    for capability in CHAT_CAPABILITIES {
        assert!(grants.allows(&Capability::parse(capability).unwrap()));
    }
    for other in [
        "buffer.write",
        "net:example.com",
        "net:*",
        "fs.read:workspace",
    ] {
        assert!(
            !grants.allows(&Capability::parse(other).unwrap()),
            "{other}"
        );
    }
    let hosts = grants.net_hosts();
    assert_eq!(hosts.hosts, ["api.anthropic.com"]);
    assert!(!hosts.any);
}

#[gpui::test]
fn the_notice_shows_until_dismissed(cx: &mut TestAppContext) {
    let mut app = cx.clone();
    let state = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let seen = state.path().join("chats/notice-seen");
    {
        let (workbench, cx) = opened(cx, dir.path());
        cx.update(|_, cx| cx.set_global(ChatStateDir(Some(state.path().to_owned()))));
        dispatch("chat.open", cx);
        let panel = chat(&workbench, cx).expect("made");
        assert!(panel.read_with(cx, |panel, _| panel.notice()));
        assert!(cx.debug_bounds("chat-notice").is_some());
        assert!(!seen.exists());

        let got_it = cx.debug_bounds("chat-notice-dismiss").expect("drawn");
        cx.simulate_click(got_it.center(), Default::default());
        draw(cx);
        assert!(cx.debug_bounds("chat-notice").is_none());
        assert!(!panel.read_with(cx, |panel, _| panel.notice()));
        assert!(seen.is_file(), "Got It writes {}", seen.display());

        // A new chat in this window doesn't show it either.
        dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
        assert!(cx.debug_bounds("chat-notice").is_none());
    }

    // Nor does a new window's panel.
    let (second, cx) = opened(&mut app, dir.path());
    dispatch("chat.open", cx);
    let panel = chat(&second, cx).expect("made");
    assert!(!panel.read_with(cx, |panel, _| panel.notice()));
    assert!(cx.debug_bounds("chat-notice").is_none());
}

/// A provider whose replies never come, noting each request made and
/// each reply's stream dropped.
#[derive(Default)]
struct Endless {
    made: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl ChatProvider for Endless {
    fn name(&self) -> &str {
        "endless"
    }

    fn stream(&self, _: ChatRequest) -> ChatStream {
        self.made.fetch_add(1, Ordering::SeqCst);
        Box::pin(Pending(self.dropped.clone()))
    }
}

struct Pending(Arc<AtomicUsize>);

impl futures::Stream for Pending {
    type Item = ChatEvent;

    fn poll_next(self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Option<ChatEvent>> {
        Poll::Pending
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[gpui::test]
fn replies_stream_on_the_background_executor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let provider = Arc::new(Endless::default());
    let shared = provider.clone();
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
    });
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    let streaming = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, cx| panel.view().read(cx).is_streaming())
    };
    let send = move |cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| view.set_composer_text("Hello?", cx));
        cx.update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
    };

    // Sending makes the request on the executor, not in `chat.send`.
    send(cx);
    assert_eq!(provider.made.load(Ordering::SeqCst), 0);
    assert!(streaming(cx));
    draw(cx);
    assert_eq!(provider.made.load(Ordering::SeqCst), 1);
    assert_eq!(provider.dropped.load(Ordering::SeqCst), 0);

    // Stopping drops the stream polled there.
    dispatch_in("chat.stop", &[crate::KEY_CONTEXT, "chat"], cx);
    assert!(!streaming(cx));
    assert_eq!(provider.dropped.load(Ordering::SeqCst), 1);

    // As does a new chat in place of one streaming.
    send(cx);
    draw(cx);
    assert_eq!(provider.made.load(Ordering::SeqCst), 2);
    drop(send);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    assert_eq!(provider.dropped.load(Ordering::SeqCst), 2);
}

/// A reply that ends refused, so the turn can be retried.
fn refused() -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::Refusal,
            stop_details: None,
        },
    ]
}

#[gpui::test]
fn retry_after_trust_is_revoked_sends_no_attached_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let provider = Arc::new(ScriptedProvider::new((0..3).map(|_| refused())));
    let shared = provider.clone();
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
    });
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);

    let secret = ContextItem {
        kind: ContextKind::File {
            path: "secret.txt".to_owned(),
            rows: None,
        },
        label: "secret.txt".to_owned(),
        text: "hunter2".to_owned(),
    };
    view.update(cx, |view, cx| {
        view.attach(secret, cx);
        view.set_composer_text("What is in it?", cx);
    });
    cx.update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
    draw(cx);
    let sent = |request: &ChatRequest| format!("{:?}", request.messages.last().unwrap().content);
    assert!(sent(&provider.requests()[0]).contains("hunter2"));

    // The same authorization sends it again.
    cx.update(|window, cx| window.dispatch_action(Box::new(RetryTurn), cx));
    draw(cx);
    assert!(sent(&provider.requests()[1]).contains("hunter2"));

    // Trust revoked through the workbench: the panel's own handler cuts it.
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    draw(cx);
    cx.update(|window, cx| window.dispatch_action(Box::new(RetryTurn), cx));
    draw(cx);
    let retried = sent(&provider.requests()[2]);
    assert!(!retried.contains("hunter2"), "{retried}");
    assert!(retried.contains("What is in it?"));
}

/// How many times [`flaky_open`] was called.
static OPENS: AtomicUsize = AtomicUsize::new(0);

/// A store that's unavailable the first time it's opened, as the Secret
/// Service may be.
fn flaky_open() -> Result<MemoryStore, SecretError> {
    match OPENS.fetch_add(1, Ordering::SeqCst) {
        0 => Err(SecretError::Unavailable("not yet".to_owned())),
        _ => Ok(MemoryStore::new()),
    }
}

#[test]
fn the_keychain_opens_again_after_failing() {
    let keychain = Keychain::new(flaky_open);
    assert_eq!(
        keychain.get("service", "account").unwrap_err(),
        SecretError::Unavailable("not yet".to_owned())
    );
    keychain.set("service", "account", "key").unwrap();
    let secret = keychain.get("service", "account").unwrap().unwrap();
    assert_eq!(secret.into_string(), "key");
    // Opened once it worked, and kept.
    assert_eq!(OPENS.load(Ordering::SeqCst), 2);
}

#[path = "chat_context_tests.rs"]
mod context;

#[path = "chat_rail_tests.rs"]
mod rail;
#[path = "chat_key_tests.rs"]
mod set_key;
