//! The AI chat in the sidebar (the decision record:
//! `docs/decisions/5at38c8v89-ai-chat.md`, § 1): `chat.open` ("Chat: Open")
//! makes the Chat panel the first time it runs, after which it's on the
//! rail as History is, and shows it with the composer focused. The panel
//! holds `fe-chat-view`'s [`ChatView`] under a header with a New Chat
//! button (`chat.new`).
//!
//! - **Grants** (record § 9): the chat is a first-party plugin holding
//!   [`CHAT_CAPABILITIES`], granted without a prompt. Its provider is
//!   [`AnthropicProvider::with_client`] over them, so fe-net reaches
//!   `api.anthropic.com` only.
//! - **The key** (record § 7): `ANTHROPIC_API_KEY` if set, and the header
//!   then says so; else the keychain's [`KEY_ACCOUNT`] entry under the app
//!   id, read on the background executor (the platform may ask the user),
//!   the view in its no-key state until it's read. [`ChatPanel::reload_key`]
//!   reads it again. The view's Set API Key runs `chat.set_api_key` once a
//!   command of that ID exists, and is logged until then; its Open Settings
//!   runs `workbench.open_settings`.
//! - **New chats** take their model, effort, thinking, token limit and
//!   fallbacks from `[chat]` in the settings; a change to them leaves an
//!   open chat as it is.
//! - **The first-run notice** (record § 10) shows until Got It, which
//!   writes `<state_dir>/chats/notice-seen`.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use fe_chat::{ChatProvider, ChatSettings, Conversation, Effort, ScriptedProvider};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent};
use fe_command::Invocation;
use fe_config::ChatEffort;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window,
    div,
};
use kiln::{
    Cx,
    command::{Capability, Grants},
    core::secrets::{KeychainStore, SecretError, SecretStore},
    platform::command_actions::{dispatch_invocation, dispatch_window, typed_command},
    ui::{Button, h_flex, ui_theme, v_flex},
};

use crate::{Workbench, dock::DockOp};

/// The AI chat in the sidebar: on the rail once it has shown; focused when
/// shown, as the composer takes the keys.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("chat"),
    name: "Chat",
    icon: fe_ui::Icon::MessageSquare,
    show_command: "chat.open",
    show_op: DockOp::ShowChat,
    order: 90,
    always_on_rail: false,
    focus_on_show: true,
};

/// What the built-in chat may do, in the plugin record's strings (record
/// § 9): read buffers for context, and reach Anthropic's API.
pub(crate) const CHAT_CAPABILITIES: [&str; 2] = ["buffer.read", "net:api.anthropic.com"];

/// The environment variable a developer's key comes from (record § 7).
pub(crate) const KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";

/// The chat's key's account in the keychain, under the app id's service.
pub(crate) const KEY_ACCOUNT: &str = "chat.anthropic";

/// The command the view's Set API Key runs.
const SET_KEY_COMMAND: &str = "chat.set_api_key";

/// The command the view's Open Settings runs.
const SETTINGS_COMMAND: &str = "workbench.open_settings";

/// The file whose presence says the first-run notice was dismissed, in
/// the state directory.
const NOTICE_SEEN: &str = "chats/notice-seen";

/// The [`CHAT_CAPABILITIES`] as grants.
fn chat_grants() -> Grants {
    (CHAT_CAPABILITIES.iter())
        .map(|capability| Capability::parse(capability).expect("the chat's capabilities parse"))
        .collect()
}

/// The keychain service every key of the app's is under: the bare app id.
fn key_service() -> String {
    fe_config::paths::app_id().reverse_dns
}

/// Where the chat's key comes from: `env`, the environment's
/// [`KEY_VARIABLE`], first, else `store`'s [`KEY_ACCOUNT`] entry.
#[derive(Clone)]
pub(crate) struct KeySource {
    pub(crate) env: Option<String>,
    pub(crate) store: Arc<dyn SecretStore>,
}

impl Global for KeySource {}

impl KeySource {
    /// The app's, unless set otherwise: the process environment and the
    /// platform keychain. Under test, nothing unless set, so no test reads
    /// the developer's key.
    fn of(cx: &App) -> KeySource {
        if let Some(source) = cx.try_global::<KeySource>() {
            return source.clone();
        }
        if cfg!(test) {
            return KeySource {
                env: None,
                store: Arc::new(kiln::core::secrets::MemoryStore::new()),
            };
        }
        KeySource {
            env: std::env::var(KEY_VARIABLE).ok(),
            store: keychain(),
        }
    }
}

/// The platform keychain, opened on its first read: opening it connects
/// to the Secret Service on Linux, which is the background executor's to
/// do.
fn keychain() -> Arc<dyn SecretStore> {
    static KEYCHAIN: OnceLock<Arc<Keychain>> = OnceLock::new();
    KEYCHAIN.get_or_init(Arc::default).clone()
}

#[derive(Default)]
struct Keychain(OnceLock<Result<KeychainStore, SecretError>>);

impl Keychain {
    fn store(&self) -> Result<&KeychainStore, SecretError> {
        self.0
            .get_or_init(KeychainStore::new)
            .as_ref()
            .map_err(Clone::clone)
    }
}

impl SecretStore for Keychain {
    fn get(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<kiln::core::secrets::Secret>, SecretError> {
        self.store()?.get(service, account)
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.store()?.set(service, account, secret)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.store()?.delete(service, account)
    }
}

/// Where the key in use came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyOrigin {
    Environment,
    Keychain,
}

/// The state directory the notice's dismissal is kept in, when not
/// fe-config's.
pub(crate) struct ChatStateDir(pub(crate) Option<PathBuf>);

impl Global for ChatStateDir {}

/// `<state_dir>/chats/notice-seen`, unless the state directory is set
/// otherwise. Under test, none unless set, so no test touches the user's.
fn notice_file(cx: &App) -> Option<PathBuf> {
    let dir = match cx.try_global::<ChatStateDir>() {
        Some(ChatStateDir(dir)) => dir.clone(),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    };
    Some(dir?.join(NOTICE_SEEN))
}

/// Writes the empty `file`, its directory made private if it's new.
fn write_notice_seen(file: &Path) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }
    std::fs::write(file, "")
}

/// A new chat's settings and options, from `[chat]` in the settings.
fn new_chat_settings(cx: &App) -> (ChatSettings, ChatOptions) {
    let chat = kiln::platform::config::settings::<fe_config::Settings>(cx)
        .chat
        .clone();
    let effort = match chat.effort {
        ChatEffort::Low => Effort::Low,
        ChatEffort::Medium => Effort::Medium,
        ChatEffort::High => Effort::High,
        ChatEffort::Xhigh => Effort::XHigh,
        ChatEffort::Max => Effort::Max,
    };
    let settings = ChatSettings {
        model: chat.model,
        effort,
        show_thinking: chat.show_thinking,
        max_tokens: chat.max_tokens,
    };
    let options = ChatOptions {
        max_tokens: chat.max_tokens,
        fallbacks: chat.fallbacks,
    };
    (settings, options)
}

/// Makes the provider for a key in tests, in place of Anthropic's.
#[cfg(test)]
pub(crate) type ProviderFactory =
    std::rc::Rc<dyn Fn(&ApiKey, ChatOptions) -> Arc<dyn ChatProvider>>;

#[cfg(test)]
struct TestProviders(ProviderFactory);

#[cfg(test)]
impl Global for TestProviders {}

/// The AI chat in the sidebar: see the module docs.
pub(crate) struct ChatPanel {
    view: Entity<ChatView>,
    /// The current chat's options.
    options: ChatOptions,
    grants: Grants,
    /// The key in use and where it came from.
    key: Option<(KeyOrigin, ApiKey)>,
    /// The keychain read under way: dropping it drops its answer.
    reading: Option<Task<()>>,
    /// Whether the first-run notice shows, in a chat started now.
    notice: bool,
    _view_events: Subscription,
}

#[kiln::command(context = "chat")]
impl ChatPanel {
    /// Chat: Open
    ///
    /// Shows the AI chat in the sidebar, focused, making it the first time.
    // The workbench's to do: from inside the panel, it goes on to `init`'s
    // listener as it does from anywhere else.
    #[command(id = "chat.open", context = "workbench")]
    fn open_chat(&mut self, cx: &mut Cx<Self>) {
        cx.propagate();
    }

    /// New Chat
    ///
    /// Starts a chat with the chat settings as they are now.
    #[command(id = "chat.new")]
    fn new_chat(&mut self, cx: &mut Cx<Self>) {
        self.start_chat(cx.window, cx.cx);
        let focus = self.focus_handle(cx.cx);
        cx.window.focus(&focus, cx.cx);
    }
}

impl ChatPanel {
    /// A panel with a new chat, reading the key.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let notice = notice_file(cx).is_none_or(|file| !file.exists());
        let (view, options, view_events) = Self::chat_view(notice, window, cx);
        let mut panel = Self {
            view,
            options,
            grants: chat_grants(),
            key: None,
            reading: None,
            notice,
            _view_events: view_events,
        };
        panel.reload_key(cx);
        panel
    }

    /// A view of a new chat, from the settings now, with no provider until
    /// the key is applied.
    fn chat_view(
        notice: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<ChatView>, ChatOptions, Subscription) {
        let (settings, options) = new_chat_settings(cx);
        let view = cx.new(|cx| {
            // `ChatView::new` takes a provider; this one never runs, as
            // the key's provider, or none, replaces it at once.
            let placeholder = Arc::new(ScriptedProvider::new([]));
            let mut view = ChatView::new(
                placeholder,
                Conversation::new(&settings),
                options,
                window,
                cx,
            );
            view.set_provider(None, cx);
            view.set_notice(notice, cx);
            view
        });
        let events = cx.subscribe_in(&view, window, Self::view_event);
        (view, options, events)
    }

    /// Replaces the chat with a new one, from the settings now.
    fn start_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (view, options, events) = Self::chat_view(self.notice, window, cx);
        (self.view, self.options, self._view_events) = (view, options, events);
        self.apply_key(cx);
        cx.notify();
    }

    /// Reads the key again: the environment's at once, else the keychain's
    /// on the background executor, the key in use kept until it's read.
    pub(crate) fn reload_key(&mut self, cx: &mut Context<Self>) {
        let source = KeySource::of(cx);
        if let Some(key) = source.env.filter(|key| !key.trim().is_empty()) {
            self.reading = None;
            self.set_key(Some((KeyOrigin::Environment, ApiKey::new(key))), cx);
            return;
        }
        let (store, service) = (source.store, key_service());
        let read = cx.background_spawn(async move { store.get(&service, KEY_ACCOUNT) });
        self.reading = Some(cx.spawn(async move |this, cx| {
            let key = match read.await {
                Ok(secret) => secret.map(|secret| ApiKey::new(secret.into_string())),
                Err(error) => {
                    log::warn!("chat: couldn't read the API key from the keychain: {error}");
                    None
                }
            };
            this.update(cx, |this, cx| {
                this.reading = None;
                this.set_key(key.map(|key| (KeyOrigin::Keychain, key)), cx);
            })
            .ok();
        }));
    }

    fn set_key(&mut self, key: Option<(KeyOrigin, ApiKey)>, cx: &mut Context<Self>) {
        self.key = key;
        self.apply_key(cx);
        cx.notify();
    }

    /// Gives the view the key's provider, or none.
    fn apply_key(&mut self, cx: &mut Context<Self>) {
        let provider = (self.key.as_ref()).map(|(_, key)| self.provider(key, cx));
        self.view
            .update(cx, |view, cx| view.set_provider(provider, cx));
    }

    /// Claude under `key`, through fe-net with the chat's grants.
    #[cfg(not(test))]
    fn provider(&self, key: &ApiKey, _: &App) -> Arc<dyn ChatProvider> {
        let provider = fe_chat_anthropic::AnthropicProvider::with_client(&self.grants, key.clone());
        Arc::new(provider.with_fallbacks(self.options.fallbacks))
    }

    /// The test's provider for `key`, or one with no replies.
    #[cfg(test)]
    fn provider(&self, key: &ApiKey, cx: &App) -> Arc<dyn ChatProvider> {
        match cx.try_global::<TestProviders>() {
            Some(TestProviders(factory)) => factory(key, self.options),
            None => Arc::new(ScriptedProvider::new([])),
        }
    }

    /// Makes the providers of keys from now on with `factory`.
    #[cfg(test)]
    pub(crate) fn set_provider_factory(cx: &mut App, factory: ProviderFactory) {
        cx.set_global(TestProviders(factory));
    }

    /// The header's note of where the key comes from.
    fn key_note(&self) -> Option<SharedString> {
        match self.key {
            Some((KeyOrigin::Environment, _)) => Some(format!("Key from {KEY_VARIABLE}").into()),
            _ => None,
        }
    }

    fn view_event(
        &mut self,
        _: &Entity<ChatView>,
        event: &ChatViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Run once this event is out, as they may come back to the
            // panel.
            ChatViewEvent::SetKeyRequested => {
                window.defer(cx, |window, cx| run_command(SET_KEY_COMMAND, window, cx));
            }
            ChatViewEvent::SettingsRequested => {
                window.defer(cx, |window, cx| run_command(SETTINGS_COMMAND, window, cx));
            }
            ChatViewEvent::NoticeDismissed => self.dismiss_notice(cx),
            ChatViewEvent::TurnFinished => {}
        }
    }

    /// Remembers the notice was dismissed, for this panel's new chats and
    /// for the panels made from now on.
    fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = false;
        let Some(file) = notice_file(cx) else {
            return;
        };
        cx.background_spawn(async move {
            if let Err(error) = write_notice_seen(&file) {
                log::warn!("chat: couldn't write {}: {error}", file.display());
            }
        })
        .detach();
    }
}

/// Runs command `id`, if a command of that ID exists yet.
fn run_command(id: &str, window: &mut Window, cx: &mut App) {
    let invocation = Invocation::of(id).expect("a valid command ID");
    if typed_command(&invocation.command, cx).is_none() {
        log::info!("chat: {id} isn't available yet");
        return;
    }
    if let Err(error) = dispatch_invocation(invocation, window, cx) {
        log::warn!("{error}");
    }
}

#[cfg(test)]
impl ChatPanel {
    pub(crate) fn view(&self) -> &Entity<ChatView> {
        &self.view
    }

    pub(crate) fn key_origin(&self) -> Option<KeyOrigin> {
        self.key.as_ref().map(|(origin, _)| *origin)
    }

    pub(crate) fn options(&self) -> ChatOptions {
        self.options
    }

    pub(crate) fn grants(&self) -> &Grants {
        &self.grants
    }

    pub(crate) fn notice(&self) -> bool {
        self.notice
    }

    pub(crate) fn header_note(&self) -> Option<SharedString> {
        self.key_note()
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        let new_chat = cx.listener(|this, _, window, cx| {
            kiln::with_cx(window, cx, |cx| this.new_chat(cx));
        });
        let note = self.key_note().map(|note| {
            div()
                .debug_selector(|| "chat-key-note".into())
                .text_color(ui.muted_text)
                .child(note)
        });
        let header = h_flex()
            .gap_2()
            .px_2()
            .py_1()
            .items_center()
            .border_b_1()
            .border_color(ui.border)
            .child(div().flex_1().min_w_0().children(note))
            .child(
                Button::new("chat-new", "New Chat")
                    .small()
                    .debug_selector(|| "chat-new".into())
                    .on_click(new_chat),
            );
        let root = v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().child(self.view.clone()));
        <Self as kiln::ViewCommands>::attach(root, cx)
    }
}

impl Focusable for ChatPanel {
    /// The composer's.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.view.focus_handle(cx)
    }
}

impl Workbench {
    /// Shows the Chat panel, made now if it hasn't been.
    pub(crate) fn open_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.dock().clone();
        if dock.read(cx).chat().is_none() {
            let panel = cx.new(|cx| ChatPanel::new(window, cx));
            dock.update(cx, |dock, cx| dock.set_chat(panel, cx));
        }
        self.run_dock_op(DockOp::ShowChat, window, cx);
    }
}

/// Runs `chat.open` in the window it was invoked in, wherever focus is:
/// the workbench's root doesn't attach the panel's commands. After the
/// dispatch, which has the window out.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &OpenChat, cx| {
        let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
            return;
        };
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    let Some(Some(workbench)) = window.root::<Workbench>() else {
                        return;
                    };
                    workbench.update(cx, |workbench, cx| workbench.open_chat(window, cx));
                })
                .ok();
        });
    });
}

#[cfg(test)]
#[path = "chat_panel_tests.rs"]
mod chat_panel_tests;
