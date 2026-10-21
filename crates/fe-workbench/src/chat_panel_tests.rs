//! The Chat panel in a headless window: made and shown by `chat.open`, its
//! key from a [`KeySource`] (a `MemoryStore` for the keychain), new chats
//! from `[chat]` in kiln's settings service, and the first-run notice in a
//! temporary state directory. The provider is a [`ScriptedProvider`]
//! from the panel's test factory: no network.

use std::{cell::RefCell, fs, path::Path, rc::Rc, sync::Arc};

use fe_chat::{ChatProvider, Effort, ScriptedProvider};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::ChatOptions;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::SettingsStore;
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    command::Capability,
    core::secrets::{MemoryStore, SecretStore as _},
    platform::config::{BatchSender, batch_channel},
};

use super::{CHAT_CAPABILITIES, ChatPanel, ChatStateDir, KeyOrigin, KeySource};
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

#[path = "chat_rail_tests.rs"]
mod rail;
#[path = "chat_key_tests.rs"]
mod set_key;
