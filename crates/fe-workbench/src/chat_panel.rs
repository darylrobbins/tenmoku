//! The AI chat in the sidebar (the decision record:
//! `docs/decisions/5at38c8v89-ai-chat.md`, § 1): the Chat panel is made with
//! the dock and always on the rail, under Git Log, as it is; `chat.open`
//! ("Chat: Open") or its rail button shows it with the composer focused. The panel
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
//!   the view in its no-key state until it's read. It's read the first time
//!   the panel draws, not at launch, as the platform may ask the user;
//!   [`ChatPanel::reload_key`] reads it again. The view's Set API Key runs `chat.set_api_key`
//!   ("Chat: Set API Key…"), which shows a masked form in place of the chat
//!   (Enter or Save stores the trimmed text in the keychain, Escape or
//!   Cancel closes it); `chat.clear_api_key` deletes the entry. Its Open
//!   Settings runs `workbench.open_settings`.
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
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    Task, Window, div,
};
use kiln::{
    Cx,
    command::{Capability, Grants},
    core::secrets::{KeychainStore, SecretError, SecretStore},
    platform::command_actions::{dispatch_invocation, dispatch_window, typed_command},
    ui::{Button, TextField, TextFieldEvent, h_flex, ui_theme, v_flex},
};

use crate::{
    Workbench,
    dock::DockOp,
    workbench::toast::{self, Toast},
};

/// The AI chat in the sidebar: always on the rail, under Git Log; focused
/// when shown, as the composer takes the keys.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("chat"),
    name: "Chat",
    icon: fe_ui::Icon::MessageSquare,
    show_command: "chat.open",
    show_op: DockOp::ShowChat,
    order: 47,
    always_on_rail: true,
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
    /// Whether the key has been read, or a read asked for.
    key_read: bool,
    /// How many key forms have been shown.
    forms_made: u64,
    /// Keychain writes not yet run, oldest first: they run one at a time,
    /// in the order asked, whatever happens to the form.
    writes: std::collections::VecDeque<Write>,
    /// The write under way, which the panel owns, so a form closed
    /// meanwhile doesn't drop it.
    writing: Option<Task<()>>,
    /// The key form, while it shows in place of the chat.
    key_form: Option<KeyForm>,
    _view_events: Subscription,
}

/// A change to the keychain's key.
enum Write {
    /// The key, from the form with this id.
    Save(u64, String),
    Clear,
}

/// The form that takes a key.
struct KeyForm {
    field: Entity<TextField>,
    /// Which form this is, for the saves made from it.
    id: u64,
    /// Whether its key is queued or being stored: a second save waits.
    saving: bool,
    _field_events: Subscription,
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

    /// Chat: Set API Key…
    ///
    /// Asks for the chat's Anthropic API key, to keep in the system keychain.
    // As `chat.open`, the workbench's to do.
    #[command(id = "chat.set_api_key", context = "workbench")]
    fn set_api_key(&mut self, cx: &mut Cx<Self>) {
        cx.propagate();
    }

    /// Chat: Clear API Key
    ///
    /// Deletes the chat's Anthropic API key from the system keychain.
    #[command(id = "chat.clear_api_key", context = "workbench")]
    fn clear_api_key(&mut self, cx: &mut Cx<Self>) {
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
    /// A panel with a new chat; the key is read when it first draws.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let notice = notice_file(cx).is_none_or(|file| !file.exists());
        let (view, options, view_events) = Self::chat_view(notice, window, cx);
        Self {
            view,
            options,
            grants: chat_grants(),
            key: None,
            reading: None,
            notice,
            key_read: false,
            forms_made: 0,
            writes: Default::default(),
            writing: None,
            key_form: None,
            _view_events: view_events,
        }
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

    /// Shows the key form in place of the chat, focused and empty.
    fn show_key_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.key_form.is_none() {
            self.forms_made += 1;
            let field = cx.new(|cx| {
                TextField::new(window, cx)
                    .placeholder("Anthropic API key")
                    .masked(true)
            });
            let events = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
                if *event == TextFieldEvent::Submitted {
                    this.save_key(window, cx);
                }
            });
            self.key_form = Some(KeyForm {
                field,
                id: self.forms_made,
                saving: false,
                _field_events: events,
            });
        }
        if let Some(form) = &self.key_form {
            let focus = form.field.focus_handle(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Closes the key form, forgetting what was typed, and focuses the
    /// composer.
    fn close_key_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.key_form.take().is_some() {
            let focus = self.view.focus_handle(cx);
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// Queues storing the form's text, trimmed, in the keychain; when it's
    /// stored the form closes and the key is read again, and an error is a
    /// toast, the form and its text kept.
    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.key_form.as_mut().filter(|form| !form.saving) else {
            return;
        };
        let key = form.field.read(cx).value().trim().to_owned();
        if key.is_empty() {
            return;
        }
        form.saving = true;
        self.writes.push_back(Write::Save(form.id, key));
        self.run_writes(window, cx);
    }

    /// Queues deleting the keychain's key.
    fn clear_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.writes.push_back(Write::Clear);
        self.run_writes(window, cx);
    }

    /// Starts the oldest queued write unless one is under way. Each ends
    /// by reading the key again and starting the next.
    fn run_writes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.writing.is_some() {
            return;
        }
        let Some(write) = self.writes.pop_front() else {
            return;
        };
        let (store, service) = (KeySource::of(cx).store, key_service());
        // The form a save was made from, which only it may change.
        let from = match &write {
            Write::Save(id, _) => Some(*id),
            Write::Clear => None,
        };
        let run = cx.background_spawn(async move {
            match write {
                Write::Save(_, key) => store.set(&service, KEY_ACCOUNT, &key),
                Write::Clear => store.delete(&service, KEY_ACCOUNT),
            }
        });
        self.writing = Some(cx.spawn_in(window, async move |this, cx| {
            let result = run.await;
            this.update_in(cx, |this, window, cx| {
                this.writing = None;
                let owns = |form: &KeyForm| Some(form.id) == from;
                match result {
                    Ok(()) => {
                        if (this.key_form.as_ref()).is_some_and(owns) {
                            this.close_key_form(window, cx);
                        }
                    }
                    Err(error) => {
                        let what = if from.is_some() { "save" } else { "clear" };
                        if let Some(form) = this.key_form.as_mut().filter(|form| owns(form)) {
                            form.saving = false;
                        }
                        toast::show(
                            cx,
                            Toast::error(format!("Couldn't {what} the API key: {error}")),
                        );
                    }
                }
                this.reload_key(cx);
                this.run_writes(window, cx);
            })
            .ok();
        }));
    }

    /// Escape closes the key form.
    fn on_form_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.close_key_form(window, cx);
            cx.stop_propagation();
        }
    }

    /// The key form: the field, Save and Cancel, and where the key goes.
    fn render_key_form(
        &self,
        form: &KeyForm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let _ = window;
        let ui = ui_theme(cx);
        let env_wins = (KeySource::of(cx).env).is_some_and(|key| !key.trim().is_empty());
        let note = |text: String| div().text_color(ui.muted_text).child(text);
        let save = cx.listener(|this, _, window, cx| this.save_key(window, cx));
        let cancel = cx.listener(|this, _, window, cx| this.close_key_form(window, cx));
        v_flex()
            .id("chat-key-form")
            .debug_selector(|| "chat-key-form".into())
            .size_full()
            .gap_2()
            .p_2()
            .on_key_down(cx.listener(Self::on_form_key))
            .child(div().child("Anthropic API key"))
            .child(form.field.clone())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("chat-key-save", "Save")
                            .small()
                            .debug_selector(|| "chat-key-save".into())
                            .on_click(save),
                    )
                    .child(
                        Button::new("chat-key-cancel", "Cancel")
                            .small()
                            .debug_selector(|| "chat-key-cancel".into())
                            .on_click(cancel),
                    ),
            )
            .child(note(
                "The key is kept in the system keychain and only sent to api.anthropic.com."
                    .to_owned(),
            ))
            .children(env_wins.then(|| {
                note(format!(
                    "{KEY_VARIABLE} is set, and is used instead of the stored key."
                ))
            }))
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
        self.key_read = true;
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The first draw reads the key, past the frame.
        if !self.key_read {
            self.key_read = true;
            cx.defer_in(window, |this, _, cx| this.reload_key(cx));
        }
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
            .child(match &self.key_form {
                Some(form) => div()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_key_form(form, window, cx)),
                None => div().flex_1().min_h_0().child(self.view.clone()),
            });
        <Self as kiln::ViewCommands>::attach(root, cx)
    }
}

impl Focusable for ChatPanel {
    /// The key form's field while it shows, else the composer's.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.key_form {
            Some(form) => form.field.focus_handle(cx),
            None => self.view.focus_handle(cx),
        }
    }
}

impl Workbench {
    /// Shows the Chat panel.
    pub(crate) fn open_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_dock_op(DockOp::ShowChat, window, cx);
    }

    /// Shows the Chat panel with its key form.
    fn set_chat_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_chat(window, cx);
        if let Some(panel) = self.dock().read(cx).chat().cloned() {
            panel.update(cx, |panel, cx| panel.show_key_form(window, cx));
        }
    }

    /// Clears the chat's key.
    fn clear_chat_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_chat(window, cx);
        if let Some(panel) = self.dock().read(cx).chat().cloned() {
            panel.update(cx, |panel, cx| panel.clear_key(window, cx));
        }
    }
}

/// Runs `chat.open`, `chat.set_api_key` and `chat.clear_api_key` in the
/// window they were invoked in, wherever focus is: the workbench's root
/// doesn't attach the panel's commands. After the dispatch, which has the
/// window out.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &OpenChat, cx| {
        on_workbench(cx, Workbench::open_chat);
    });
    cx.on_action(|_: &SetApiKey, cx| {
        on_workbench(cx, Workbench::set_chat_key);
    });
    cx.on_action(|_: &ClearApiKey, cx| {
        on_workbench(cx, Workbench::clear_chat_key);
    });
}

/// Runs `run` on the workbench of the window a command was invoked in.
fn on_workbench(cx: &mut App, run: fn(&mut Workbench, &mut Window, &mut Context<Workbench>)) {
    let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
        return;
    };
    cx.defer(move |cx| {
        handle
            .update(cx, |_, window, cx| {
                let Some(Some(workbench)) = window.root::<Workbench>() else {
                    return;
                };
                workbench.update(cx, |workbench, cx| run(workbench, window, cx));
            })
            .ok();
    });
}

#[cfg(test)]
#[path = "chat_panel_tests.rs"]
mod chat_panel_tests;
