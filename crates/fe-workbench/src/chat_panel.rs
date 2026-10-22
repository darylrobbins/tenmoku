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
//! - **Context** (record § 13; `chat_context.rs`): the selection, the
//!   active file, its diagnostics and the open editors, gathered again as
//!   the active editor or its selection changes while the panel shows, and
//!   when it gains focus; and the @files the user picks. The view budgets
//!   them to `chat.context_budget`.
//! - **Replies** (record § 8; `chat_apply.rs`): Apply on a code block
//!   proposes it in its file, or over the selection its turn was sent with,
//!   by dispatching `chat.propose_edit` as the AI actor; a link to a file
//!   under the root opens it.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use fe_chat::{
    ChatProvider, ChatSettings, ContextItem, ContextKind, Conversation, Effort, Rows,
    ScriptedProvider,
};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent};
use fe_command::Invocation;
use fe_config::ChatEffort;
use fe_editor::EditorModel;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Task, WeakEntity, Window, div,
};
use kiln::{
    Cx,
    command::{Capability, Grants},
    core::secrets::{KeychainStore, SecretError, SecretStore},
    platform::command_actions::{dispatch_invocation, dispatch_window, typed_command},
    ui::{Button, TextField, TextFieldEvent, h_flex, ui_theme, v_flex},
};

use crate::{
    DockPanel, RootChanged, TrustChanged, Workbench,
    dock::DockOp,
    workbench::toast::{self, Toast},
};

#[path = "chat_apply.rs"]
mod chat_apply;
#[path = "chat_context.rs"]
mod chat_context;

use chat_context::MAX_FILE;

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
    static KEYCHAIN: OnceLock<Arc<Keychain<KeychainStore>>> = OnceLock::new();
    KEYCHAIN
        .get_or_init(|| Arc::new(Keychain::new(KeychainStore::new)))
        .clone()
}

/// A store opened by `open` on its first use. One that fails to open is
/// opened again on the next use, as the Secret Service may be back.
struct Keychain<S> {
    store: OnceLock<S>,
    open: fn() -> Result<S, SecretError>,
}

impl<S> Keychain<S> {
    fn new(open: fn() -> Result<S, SecretError>) -> Self {
        Self {
            store: OnceLock::new(),
            open,
        }
    }

    fn store(&self) -> Result<&S, SecretError> {
        if let Some(store) = self.store.get() {
            return Ok(store);
        }
        let store = (self.open)()?;
        // Another thread may have opened it meanwhile: either will do.
        Ok(self.store.get_or_init(|| store))
    }
}

impl<S: SecretStore> SecretStore for Keychain<S> {
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
    /// The workbench whose context it attaches: its window's, from its
    /// first draw.
    workbench: Option<WeakEntity<Workbench>>,
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
    /// The active editor followed, to gather again as it changes.
    watched: Option<(EntityId, Subscription)>,
    /// Whether a gathering is to run once the current update's out.
    gathering: bool,
    /// The active file as it was when its real path last passed
    /// [`chat_context::vet_text`]: what the automatic context may come from.
    vetted: Option<ActiveFile>,
    /// The vetting under way: dropping it drops its answer.
    vetting: Option<Task<()>>,
    /// Counts the root's changes and its trust's: a vetting made under an
    /// earlier count doesn't stand.
    generation: u64,
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
    /// Applying the replies' code blocks.
    applies: chat_apply::Applies,
    /// The view's events, its gaining focus, and its changes.
    _view: [Subscription; 3],
    /// The workbench's changes, and its root's and its trust's.
    _workbench: Vec<Subscription>,
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

    /// Chat: Ask About Selection
    ///
    /// Opens the chat with the active editor's selection attached, or its
    /// current row if nothing is selected, and focuses the composer. Works in
    /// an untrusted folder: the selection is the user's own.
    // As `chat.open`, the workbench's to do.
    #[command(id = "chat.ask_about_selection", context = "editor")]
    fn ask_about_selection(&mut self, cx: &mut Cx<Self>) {
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
    /// A panel with a new chat; the key is read, and the context of its
    /// window's workbench followed, when it first draws.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let notice = notice_file(cx).is_none_or(|file| !file.exists());
        let (view, options, view_subscriptions) = Self::chat_view(notice, window, cx);
        Self {
            workbench: None,
            view,
            options,
            grants: chat_grants(),
            key: None,
            reading: None,
            notice,
            watched: None,
            gathering: false,
            vetted: None,
            vetting: None,
            generation: 0,
            key_read: false,
            forms_made: 0,
            writes: Default::default(),
            writing: None,
            key_form: None,
            applies: chat_apply::Applies::new(),
            _view: view_subscriptions,
            _workbench: Vec::new(),
        }
    }

    /// A view of a new chat, from the settings now, with no provider until
    /// the key is applied, and the subscriptions to its events, its gaining
    /// focus and its changes.
    fn chat_view(
        notice: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<ChatView>, ChatOptions, [Subscription; 3]) {
        let (settings, options) = new_chat_settings(cx);
        let budget = kiln::platform::config::settings::<fe_config::Settings>(cx)
            .chat
            .context_budget;
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
            view.set_context_budget(budget, cx);
            view
        });
        let events = cx.subscribe_in(&view, window, Self::view_event);
        let focus = view.focus_handle(cx);
        let focused = cx.on_focus_in(&focus, window, |this, _, cx| {
            // What was a file may be a link now.
            this.vetted = None;
            this.gather(cx);
        });
        // A turn sent keeps the selection it was sent with.
        let changed = cx.observe(&view, |this, _, cx| this.note_turns(cx));
        (view, options, [events, focused, changed])
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
        let (view, options, subscriptions) = Self::chat_view(self.notice, window, cx);
        (self.view, self.options, self._view) = (view, options, subscriptions);
        self.applies.new_chat();
        self.apply_key(cx);
        self.gather_soon(cx);
        cx.notify();
    }

    /// Gathers the context once the current update's out, as the workbench
    /// or the active editor changed: at most once an update.
    fn gather_soon(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.gathering, true) {
            return;
        }
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            (panel.update(cx, |panel, cx| {
                panel.gathering = false;
                panel.gather(cx);
            }))
            .ok();
        });
    }

    /// Follows the active editor, and while the panel shows gives the view
    /// the context gathered now. Nothing comes from the active file until
    /// its real path is vetted for its text as it is: until then the view
    /// keeps what it had from the same file, or has none from a file newly
    /// active.
    fn gather(&mut self, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let editor = workbench.read(cx).active_editor(cx).cloned();
        let id = editor.as_ref().map(Entity::entity_id);
        if self.watched.as_ref().map(|(watched, _)| *watched) != id {
            self.watched = editor.map(|editor| {
                let changed = cx.observe(&editor, |this, _, cx| this.gather_soon(cx));
                (editor.entity_id(), changed)
            });
        }
        let dock = workbench.read(cx).dock().read(cx);
        if !dock.is_visible() || dock.active() != DockPanel::Chat {
            return;
        }
        let active = ActiveFile::of(workbench.read(cx), self.generation, cx);
        let Some(active) = active else {
            // No file to vet.
            (self.vetted, self.vetting) = (None, None);
            return self.set_context(None, cx);
        };
        if self.vetted.as_ref() == Some(&active) {
            // Vetted as it is: a selection moved.
            self.vetting = None;
            return self.set_context(Some(&active.path), cx);
        }
        // Nothing from it until it's vetted as it is now.
        self.vetted = None;
        self.set_context(None, cx);
        let (root, path) = (active.root.clone(), active.path.clone());
        // Its path, resolved, past the deny list, and its text from the
        // file there.
        let stamp = active.disk;
        let vet = cx.background_spawn(async move {
            root.is_some_and(|root| {
                chat_context::vet_text(&root, &path, stamp.as_ref(), || {}).is_ok()
            })
        });
        self.vetting = Some(cx.spawn(async move |panel, cx| {
            let passed = vet.await;
            (panel.update(cx, |panel, cx| {
                let Some(workbench) = panel.workbench() else {
                    return;
                };
                // Changed meanwhile: vetted again.
                let now = ActiveFile::of(workbench.read(cx), panel.generation, cx);
                if now.as_ref() != Some(&active) {
                    return panel.gather_soon(cx);
                }
                panel.vetted = passed.then_some(active);
                let vetted = panel.vetted.as_ref().map(|vetted| vetted.path.clone());
                panel.set_context(vetted.as_deref(), cx);
            }))
            .ok();
        }));
    }

    /// Follows `workbench`'s context: gathers again as it changes, and as
    /// its root or trust does.
    fn follow(&mut self, workbench: &Entity<Workbench>, cx: &mut Context<Self>) {
        self.workbench = Some(workbench.downgrade());
        self._workbench = vec![
            cx.observe(workbench, |this, _, cx| this.gather_soon(cx)),
            cx.subscribe(workbench, |this, _, _: &RootChanged, cx| {
                this.root_changed(cx)
            }),
            cx.subscribe(workbench, |this, _, _: &TrustChanged, cx| {
                this.root_changed(cx)
            }),
        ];
        self.gather_soon(cx);
    }

    /// The workbench followed, if it's still there.
    fn workbench(&self) -> Option<Entity<Workbench>> {
        self.workbench.as_ref().and_then(WeakEntity::upgrade)
    }

    /// The root, or its trust, changed: nothing vetted before stands, and
    /// what the view has is cut to what the root's trust allows at once,
    /// whether the panel shows or not, before the active file is vetted
    /// again.
    fn root_changed(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        // What the user attached under the old root or trust goes, being
        // read or not, and no retry sends what was sent under them.
        let generation = self.generation;
        self.view.update(cx, |view, cx| {
            view.clear_attachments(cx);
            view.set_authorization(generation);
        });
        // Nor is a block applied, or a link opened, that was asked for
        // under them.
        self.applies.stale();
        (self.vetted, self.vetting) = (None, None);
        self.set_context(None, cx);
        self.gather(cx);
    }

    /// Gives the view the context gathered now, from the active file only
    /// if it's at `vetted`.
    fn set_context(&mut self, vetted: Option<&Path>, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench() else {
            return;
        };
        let items = chat_context::gather(workbench.read(cx), &self.grants, vetted, cx);
        // A turn sent with the chip the view has now keeps it.
        self.note_turns(cx);
        self.note_chip(&items, workbench.read(cx), cx);
        self.view.update(cx, |view, cx| view.set_context(items, cx));
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
            ChatViewEvent::AttachFileRequested => {
                let panel = cx.weak_entity();
                window.defer(cx, move |window, cx| {
                    (panel.update(cx, |panel, cx| panel.pick_file(window, cx))).ok();
                });
            }
            ChatViewEvent::OpenPath { path, row } => {
                let (panel, path, row) = (cx.weak_entity(), path.clone(), *row);
                window.defer(cx, move |window, cx| {
                    (panel.update(cx, |panel, cx| panel.open_link(&path, row, window, cx))).ok();
                });
            }
            ChatViewEvent::ApplyBlock { turn, block } => {
                let (panel, turn, block) = (cx.weak_entity(), *turn, *block);
                window.defer(cx, move |window, cx| {
                    (panel.update(cx, |panel, cx| panel.apply_block(turn, block, window, cx))).ok();
                });
            }
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

/// The active editor's file as it is: under which root and root
/// generation, in which editor, at which path, at which version of its
/// text, read or saved when.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveFile {
    root: Option<PathBuf>,
    generation: u64,
    editor: EntityId,
    path: PathBuf,
    buffer: fe_buffer::BufferId,
    version: fe_buffer::Version,
    disk: Option<fe_buffer::DiskStamp>,
}

impl ActiveFile {
    /// The active editor's, if it has a file.
    fn of(workbench: &Workbench, generation: u64, cx: &App) -> Option<ActiveFile> {
        let editor = workbench.active_editor(cx)?;
        let model = editor.read(cx).model();
        let buffer = model.buffer();
        Some(ActiveFile {
            root: workbench.root().map(Path::to_owned),
            generation,
            editor: editor.entity_id(),
            path: model.path()?.to_owned(),
            buffer: buffer.id(),
            version: buffer.version(),
            disk: buffer.disk_stamp(),
        })
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

    pub(crate) fn set_grants(&mut self, grants: Grants, cx: &mut Context<Self>) {
        self.grants = grants;
        self.generation += 1;
        self.applies.stale();
        let (generation, read) = (self.generation, self.grants.allows(&Capability::BufferRead));
        self.view.update(cx, |view, cx| {
            if !read {
                view.clear_attachments(cx);
            }
            view.set_authorization(generation);
        });
        self.set_context(None, cx);
        self.gather(cx);
    }

    pub(crate) fn key_form_closed(&self) -> bool {
        self.key_form.is_none()
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
        if self.workbench.is_none()
            && let Some(Some(workbench)) = window.root::<Workbench>()
        {
            self.follow(&workbench, cx);
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

    /// Shows the Chat panel with the active editor's selection attached, or
    /// its current row if empty, and the composer focused.
    fn ask_about_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let item = self.active_editor(cx).and_then(|editor| {
            let model = editor.read(cx).model();
            selection_or_row(self.root(), model)
        });
        self.open_chat(window, cx);
        let Some(panel) = self.dock().read(cx).chat().cloned() else {
            return;
        };
        panel.update(cx, |panel, cx| {
            // The key form stands in for the chat while open.
            panel.close_key_form(window, cx);
            if let Some(item) = item {
                panel.view.update(cx, |view, cx| view.attach(item, cx));
            }
            let focus = panel.focus_handle(cx);
            window.focus(&focus, cx);
        });
    }

    /// Clears the chat's key.
    fn clear_chat_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_chat(window, cx);
        if let Some(panel) = self.dock().read(cx).chat().cloned() {
            panel.update(cx, |panel, cx| panel.clear_key(window, cx));
        }
    }
}

/// `model`'s primary selection as a chip's item, or its caret's row if the
/// selection is empty; `None` if that is empty or over [`MAX_FILE`].
fn selection_or_row(root: Option<&Path>, model: &EditorModel) -> Option<ContextItem> {
    let range = model.primary_selection().range;
    let text = model.text();
    let (start, end) = (
        text.offset_to_point(range.start),
        text.offset_to_point(range.end),
    );
    let (range, first, last) = if range.is_empty() {
        let at = text.row_start(start.row);
        (
            at..at + text.row_len(start.row) as usize,
            start.row,
            start.row,
        )
    } else {
        // A selection ending at a row's start doesn't take that row.
        let last = if end.column == 0 && end.row > start.row {
            end.row - 1
        } else {
            end.row
        };
        (range, start.row, last)
    };
    if range.is_empty() || range.len() > MAX_FILE {
        return None;
    }
    let path = match model.path() {
        Some(path) => root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string(),
        None => "untitled".to_owned(),
    };
    let rows = Rows {
        first: first + 1,
        last: last + 1,
    };
    Some(ContextItem {
        label: format!("{path}:{rows}"),
        kind: ContextKind::Selection { path, rows },
        text: text.slice_to_string(range),
    })
}

/// Runs `chat.open`, `chat.set_api_key` and `chat.clear_api_key` in the
/// window they were invoked in, wherever focus is: the workbench's root
/// doesn't attach the panel's commands. After the dispatch, which has the
/// window out.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &OpenChat, cx| {
        on_workbench(cx, Workbench::open_chat);
    });
    cx.on_action(|_: &AskAboutSelection, cx| {
        on_workbench(cx, Workbench::ask_about_selection);
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
