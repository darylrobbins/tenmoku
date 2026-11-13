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
//! - **The model picker** (`chat-model-picker`, "Opus 5.5 · Medium") in the
//!   header lists, in place of the chat as History does, the models and
//!   efforts [`picker_choices`] offers, the chat's own marked. A pick sets
//!   what the chat's next turn asks for (an empty chat's own model or
//!   effort); earlier turns go back as they were sent (record § 4). It
//!   doesn't write the settings.
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
//! - **Chats** (record § 11): each is kept at
//!   `<state_dir>/chats/<layout_key(root)>/<id>.json` (`no-folder` with no
//!   root) by [`ChatStore`], written after each finished turn on the
//!   background executor, and the folder's newest reopens with the panel.
//!   The header's History lists the folder's chats and opens one;
//!   `chat.delete` deletes the shown one. In an untrusted folder (record
//!   decision 6) chats stay in memory: nothing is read or written, and
//!   History lists the window's own.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::SystemTime,
};

use fe_chat::{
    ChatId, ChatProvider, ChatSettings, ContextItem, ContextKind, Conversation, Effort, Rows,
    ScriptedProvider,
};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent};
use fe_command::Invocation;
use fe_config::ChatEffort;
use fe_editor::EditorModel;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable,
    Global, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PromptLevel,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    WeakEntity, Window, div,
};
use kiln::{
    Cx,
    command::{Capability, Grants},
    core::secrets::{KeychainStore, SecretError, SecretStore},
    platform::command_actions::{dispatch_invocation, dispatch_window, typed_command},
    ui::{Button, TextField, TextFieldEvent, h_flex, ui_theme, v_flex},
};
use kiln_l10n::{Text, text, tr, verbatim};

use crate::{
    DockPanel, RootChanged, TrustChanged, Workbench,
    dock::DockOp,
    workbench::toast::{self, Toast},
};

#[path = "chat_apply.rs"]
mod chat_apply;
#[path = "chat_context.rs"]
mod chat_context;
#[path = "chat_store_worker.rs"]
mod chat_store_worker;

use chat_store_worker::{SaveState, Saved, Target, Worker, WorkerError};

use chat_context::MAX_FILE;

/// The AI chat in the sidebar: always on the rail, under Git Log; focused
/// when shown, as the composer takes the keys.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("chat"),
    name: verbatim!("Chat"),
    icon: fe_ui::Icon::Bot,
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
    Some(state_dir(cx)?.join(NOTICE_SEEN))
}

/// The state directory, unless set otherwise. Under test, none unless set.
fn state_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<ChatStateDir>() {
        Some(ChatStateDir(dir)) => dir.clone(),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    }
}

/// Where a folder's chats are kept: `<state_dir>/chats/<layout_key>` for
/// the canonical `root`, else `<state_dir>/chats/no-folder`.
#[cfg(test)]
pub(crate) fn chats_dir(state_dir: &Path, root: Option<&Path>) -> PathBuf {
    let canonical = root.map(canonical_root);
    folder_dir(state_dir, canonical.as_deref())
}

/// `root` with its symlinks resolved, or as it is if that fails.
fn canonical_root(root: &Path) -> PathBuf {
    dunce::canonicalize(root).unwrap_or_else(|_| root.to_owned())
}

/// The chats directory of the folder at `canonical`, which is worked out
/// from the root once.
fn folder_dir(state_dir: &Path, canonical: Option<&Path>) -> PathBuf {
    let folder = match canonical {
        Some(root) => kiln::core::layout::layout_key(root),
        None => "no-folder".to_owned(),
    };
    state_dir.join("chats").join(folder)
}

/// A folder as the chat sees it, fixed at one moment: its canonical root
/// and, with a state directory, where its chats are. A chat keeps the
/// folder it began in, whatever its root's name points to later.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Folder {
    root: Option<PathBuf>,
    target: Option<Target>,
}

/// A model picker row's choice.
enum Pick {
    Model(String),
    Effort(Effort),
}

/// A chat in History.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryRow {
    pub(crate) id: ChatId,
    pub(crate) title: String,
    pub(crate) updated: SystemTime,
}

impl HistoryRow {
    fn label(&self) -> Text {
        let title = if self.title.is_empty() {
            tr!("workbench-chat-untitled").to_string()
        } else {
            self.title.clone()
        };
        let ago = SystemTime::now()
            .duration_since(self.updated)
            .unwrap_or_default()
            .as_secs();
        let when = match ago {
            0..60 => tr!("workbench-chat-ago-now").to_string(),
            60..3600 => tr!("workbench-chat-ago-minutes", count = ago / 60).to_string(),
            3600..86400 => tr!("workbench-chat-ago-hours", count = ago / 3600).to_string(),
            _ => tr!("workbench-chat-ago-days", count = ago / 86400).to_string(),
        };
        text!("workbench-chat-history-label", title = title, when = when)
    }
}

/// What the model picker offers: models and efforts, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PickerChoices {
    pub(crate) models: Vec<String>,
    pub(crate) efforts: Vec<Effort>,
}

/// The efforts, least first.
const EFFORTS: [Effort; 5] = [
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::XHigh,
    Effort::Max,
];

/// What the picker offers for `conversation`: the models the app knows,
/// then the chat's own if it isn't one of them (typed into the settings),
/// and the five efforts. The one place the lists come from, so another
/// kind of chat can offer its own.
pub(crate) fn picker_choices(conversation: &Conversation) -> PickerChoices {
    let mut models: Vec<String> = (fe_config::chat_models::KNOWN.iter())
        .map(|model| (*model).to_owned())
        .collect();
    if !models.contains(&conversation.model) {
        models.push(conversation.model.clone());
    }
    PickerChoices {
        models,
        efforts: EFFORTS.to_vec(),
    }
}

/// An effort's name, such as "Medium".
fn effort_label(effort: Effort) -> Text {
    match effort {
        Effort::Low => text!("workbench-chat-effort-low"),
        Effort::Medium => text!("workbench-chat-effort-medium"),
        Effort::High => text!("workbench-chat-effort-high"),
        Effort::XHigh => text!("workbench-chat-effort-xhigh"),
        Effort::Max => text!("workbench-chat-effort-max"),
    }
}

/// The picker button's label for `conversation`: its model and effort, as
/// "Opus 5.5 · Medium".
pub(crate) fn picker_label(conversation: &Conversation) -> Text {
    let model = fe_config::chat_models::label(&conversation.model).to_owned();
    let effort = effort_label(conversation.effort).resolve().to_string();
    text!(
        "workbench-chat-model-effort",
        model = model,
        effort = effort
    )
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
        effort_message_models: fe_config::chat_models::EFFORT_MESSAGE_MODELS,
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
    /// The provider the key makes, which the view and inline edits use.
    current: Option<Arc<dyn ChatProvider>>,
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
    /// The window, for the replies that need it.
    window: AnyWindowHandle,
    /// Whether the folder's newest chat was opened, or given up on.
    restored: bool,
    /// The restoring under way: dropping it drops its answer.
    restoring: Option<Task<()>>,
    /// Counts the views made: an answer for an earlier one doesn't stand.
    views_made: u64,
    /// Chats finished while the folder is untrusted, or has no state
    /// directory, oldest first.
    memory: Vec<(Folder, Conversation, SystemTime)>,
    /// The root the shown chat belongs to.
    home: Folder,
    /// The shown chat's lease on its folder, which it gives up on leaving.
    lease: Option<u64>,
    /// Whether the shown chat was written out on leaving its folder.
    departed: bool,
    /// The chats whose failed save was shown.
    save_failed: HashSet<ChatId>,
    /// Each chat as the store last held it for this window: what it
    /// loaded, or acknowledged writing. A chat that differs has turns the
    /// store hasn't acknowledged (pending, or rejected), and is the only
    /// kind written.
    acked: HashMap<ChatId, Conversation>,
    /// Each chat's revision in the store: the one its snapshot derives
    /// from, which a save must still find there. Shared with the saves
    /// queued, which run in order and move it on.
    bases: HashMap<ChatId, Arc<SaveState>>,
    /// History, while it shows in place of the chat.
    history: Option<Vec<HistoryRow>>,
    /// Whether the model picker shows in place of the chat.
    picker: bool,
    /// The listing under way: dropping it drops its answer.
    listing: Option<Task<()>>,
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

    /// Delete Chat
    ///
    /// Asks, then deletes the shown chat's file, or drops it if it is kept
    /// in memory only, and starts a new chat.
    #[command(id = "chat.delete")]
    fn delete_chat(&mut self, cx: &mut Cx<Self>) {
        self.confirm_delete(cx.window, cx.cx);
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
        let (view, options, view_subscriptions) = Self::chat_view(notice, None, window, cx);
        Self {
            workbench: None,
            view,
            options,
            grants: chat_grants(),
            key: None,
            current: None,
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
            window: window.window_handle(),
            restored: false,
            restoring: None,
            views_made: 0,
            memory: Vec::new(),
            home: Folder::default(),
            lease: None,
            departed: false,
            save_failed: HashSet::new(),
            acked: HashMap::new(),
            bases: HashMap::new(),
            history: None,
            picker: false,
            listing: None,
            _view: view_subscriptions,
            _workbench: Vec::new(),
        }
    }

    /// A view of a new chat, from the settings now, with no provider until
    /// the key is applied, and the subscriptions to its events, its gaining
    /// focus and its changes.
    fn chat_view(
        notice: bool,
        conversation: Option<Conversation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<ChatView>, ChatOptions, [Subscription; 3]) {
        let (settings, options) = new_chat_settings(cx);
        let notice = notice && conversation.is_none();
        let budget = kiln::platform::config::settings::<fe_config::Settings>(cx)
            .chat
            .context_budget;
        let view = cx.new(|cx| {
            // `ChatView::new` takes a provider; this one never runs, as
            // the key's provider, or none, replaces it at once.
            let placeholder = Arc::new(ScriptedProvider::new([]));
            let mut view = ChatView::new(
                placeholder,
                conversation.unwrap_or_else(|| Conversation::new(&settings)),
                options,
                window,
                cx,
            );
            view.set_provider(None, cx);
            view.set_effort_message_models(settings.effort_message_models);
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

    /// The shown folder now: its root canonical, its chats' directory
    /// worked out from that.
    fn current_folder(&self, cx: &App) -> Folder {
        let root = self.current_root(cx).map(|root| canonical_root(&root));
        let target = state_dir(cx).map(|state| Target {
            dir: folder_dir(&state, root.as_deref()),
            root: root.clone(),
        });
        Folder { root, target }
    }

    /// Has the worker track the shown folder, trusted as the decisions say
    /// now, which are the app's and not this window's: a folder no window
    /// shows is followed too.
    fn track_current(&self, cx: &mut Context<Self>) -> Folder {
        let folder = self.current_folder(cx);
        if let Some(target) = &folder.target {
            let trusted = folder_trusted(target.root.as_deref(), cx);
            Worker::of(cx).track(target, trusted);
        }
        folder
    }

    /// Makes `folder` the shown chat's. Within one folder the lease, and
    /// what was asked under it, stands; a new folder has a lease of its own.
    fn adopt(&mut self, folder: Folder, cx: &mut Context<Self>) {
        let worker = Worker::of(cx);
        let same = folder == self.home && self.lease.is_some() && !self.departed;
        if !same {
            if let Some(lease) = self.lease.take() {
                worker.release(lease);
            }
            self.lease = folder.target.as_ref().map(|_| worker.lease());
            self.home = folder;
            self.departed = false;
        }
    }

    /// Permission for the chats of the folder the shown chat is from, held
    /// by its lease.
    fn home_token(&self, cx: &mut Context<Self>) -> Option<chat_store_worker::Token> {
        let target = self.home.target.clone()?;
        Worker::of(cx).token_leased(&target, self.lease?)
    }

    /// Permission for the shown folder, as the shown chat's lease holds it:
    /// none while the chat is another folder's.
    fn current_token(&self, cx: &mut Context<Self>) -> Option<chat_store_worker::Token> {
        self.workbench()?;
        if self.track_current(cx) != self.home {
            return None;
        }
        self.home_token(cx)
    }

    /// Opens the folder's newest chat, once, if the shown one is untouched:
    /// waits for the workbench, and for its root's trust to be known.
    fn restore(&mut self, cx: &mut Context<Self>) {
        if self.restored {
            return;
        }
        let Some(token) = self.current_token(cx) else {
            // Memory-only, or not known yet: nothing to read.
            return;
        };
        self.restored = true;
        let (views, generation) = (self.views_made, self.generation);
        let read = Worker::of(cx).newest(token);
        let handle = self.window;
        self.restoring = Some(cx.spawn(async move |panel, cx| {
            let Ok(Some((conversation, revision))) = read.await else {
                return;
            };
            cx.update_window(handle, |_, window, cx| {
                panel
                    .update(cx, |panel, cx| {
                        panel.restoring = None;
                        let view = panel.view.read(cx);
                        let untouched = view.conversation().turns.is_empty()
                            && view.composer_text(cx).is_empty();
                        if panel.views_made == views && panel.generation == generation && untouched
                        {
                            panel.adopt_disk(&conversation, revision);
                            panel.show_chat(Some(conversation), window, cx);
                        }
                    })
                    .ok();
            })
            .ok();
        }));
    }

    /// Keeps the shown chat, if it has finished turns, under the folder it
    /// began in: written if that folder is trusted, else in the window's
    /// memory.
    fn remember_chat(&mut self, cx: &mut Context<Self>) {
        let conversation = self.view.read(cx).conversation();
        if conversation.turns.is_empty() || self.departed {
            return;
        }
        let conversation = conversation.clone();
        // The window keeps its own snapshot whatever becomes of the write.
        self.keep_in_memory(conversation.clone());
        if let Some(token) = self.home_token(cx) {
            let origin = self.home.clone();
            self.save_chat(token, origin, conversation, cx);
        }
    }

    /// Whether `conversation` has turns the store hasn't acknowledged. A
    /// chat only looked at isn't written: another window may have gone on
    /// with it.
    fn dirty(&self, conversation: &Conversation) -> bool {
        self.acked.get(&conversation.id) != Some(conversation)
    }

    /// The store now holds `conversation` at `revision`, as it was loaded.
    fn adopt_disk(&mut self, conversation: &Conversation, revision: u64) {
        // A state of its own: saves queued under the old one keep to it.
        self.bases.insert(conversation.id, SaveState::new(revision));
        self.acked.insert(conversation.id, conversation.clone());
    }

    /// Saves `conversation` of folder `origin` under `token` if it has
    /// unacknowledged turns. The write is compare-and-swap on the revision
    /// the chat derives from; one that finds the file written meanwhile
    /// writes nothing, and the chat goes on as a copy.
    fn save_chat(
        &mut self,
        token: chat_store_worker::Token,
        origin: Folder,
        conversation: Conversation,
        cx: &mut Context<Self>,
    ) {
        let worker = Worker::of(cx);
        if !self.dirty(&conversation) {
            worker.release_token(token);
            return;
        }
        let base = (self.bases.entry(conversation.id))
            .or_insert_with(|| SaveState::new(0))
            .clone();
        let saved = worker.save(token, conversation.clone(), base);
        cx.spawn(async move |panel, cx| {
            let result = saved.await;
            panel
                .update(cx, |panel, cx| {
                    panel.saved(conversation, origin, result, cx)
                })
                .ok();
        })
        .detach();
    }

    /// What became of a save: acknowledged, refused for trust or quitting
    /// (the chat stays unacknowledged), failed (shown once per chat), or
    /// diverged.
    fn saved(
        &mut self,
        conversation: Conversation,
        origin: Folder,
        result: Result<Saved, WorkerError>,
        cx: &mut Context<Self>,
    ) {
        let id = conversation.id;
        match result {
            Ok(Saved::Written {
                id: at,
                title,
                revision,
            }) => {
                self.acked.insert(id, conversation.clone());
                if at != id {
                    // Written in the copy the chat went on in.
                    self.note_copy(conversation, origin, at, title, revision);
                }
            }
            Ok(Saved::Copied {
                id: at,
                title,
                revision,
            }) => {
                // The file stands, the turns are safe in the copy (written by
                // the save itself), and the chat in view is left as it is.
                self.acked.insert(id, conversation.clone());
                self.note_copy(conversation, origin, at, title, revision);
                toast::show(
                    cx,
                    Toast::warning(tr!("workbench-chat-changed-elsewhere").to_string()),
                );
            }
            Err(WorkerError::Store(error)) => {
                log::warn!("chat: couldn't save chat {id}: {error}");
                // Once a chat, not on every turn.
                if self.save_failed.insert(id) {
                    toast::show(
                        cx,
                        Toast::error(
                            tr!("workbench-chat-cant-save", error = error.to_string()).to_string(),
                        ),
                    );
                }
            }
            Err(WorkerError::Diverged) => {
                log::warn!("chat: the copy of chat {id} was changed elsewhere");
            }
            Err(_) => {}
        }
    }

    /// The window's own record of copy `at`, which holds `conversation` and
    /// is on disk at `revision`: as good as loaded, so it opens from the
    /// file, and its retained snapshot is the latest.
    fn note_copy(
        &mut self,
        conversation: Conversation,
        origin: Folder,
        at: ChatId,
        title: String,
        revision: u64,
    ) {
        let mut copy = conversation;
        (copy.id, copy.title) = (at, title);
        self.bases.insert(at, SaveState::new(revision));
        self.acked.insert(at, copy.clone());
        self.memory.retain(|(_, chat, _)| chat.id != at);
        self.memory.push((origin, copy, SystemTime::now()));
    }

    /// Keeps `conversation` in the window's memory under its folder, newest
    /// last.
    fn keep_in_memory(&mut self, conversation: Conversation) {
        self.memory
            .retain(|(_, chat, _)| chat.id != conversation.id);
        let home = self.home.clone();
        self.memory.push((home, conversation, SystemTime::now()));
    }

    /// The chat in view is leaving its folder: what it holds is written once
    /// under a token of its own (or kept in memory), and what was asked
    /// under its lease before is refused.
    fn depart(&mut self, cx: &mut Context<Self>) {
        let conversation = self.view.read(cx).conversation().clone();
        if conversation.turns.is_empty() || self.departed {
            return;
        }
        let worker = Worker::of(cx);
        if let Some(lease) = self.lease.take() {
            worker.release(lease);
        }
        self.departed = true;
        self.keep_in_memory(conversation.clone());
        let origin = self.home.clone();
        let once = origin
            .target
            .as_ref()
            .and_then(|target| worker.token_once(target));
        if let Some(token) = once {
            self.save_chat(token, origin, conversation, cx);
        }
    }

    /// The root now, which the shown chat belongs to unless it changed.
    fn current_root(&self, cx: &App) -> Option<PathBuf> {
        let workbench = self.workbench()?;
        workbench.read(cx).root().map(Path::to_owned)
    }

    /// Shows or closes History: the folder's chats newest first, from disk
    /// where it is trusted, with the window's own not written, so that
    /// those chats are listed whatever the trust has been since.
    fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.picker = false;
        if self.history.take().is_some() || self.listing.take().is_some() {
            cx.notify();
            return;
        }
        let Some(token) = self.current_token(cx) else {
            self.history = Some(self.memory_rows(&[], cx));
            cx.notify();
            return;
        };
        let generation = self.generation;
        let list = Worker::of(cx).list(token);
        self.listing = Some(cx.spawn(async move |panel, cx| {
            let rows: Vec<HistoryRow> = match list.await {
                Ok(summaries) => (summaries.into_iter())
                    .map(|summary| HistoryRow {
                        id: summary.id,
                        title: summary.title,
                        updated: summary.updated,
                    })
                    .collect(),
                Err(WorkerError::Store(error)) => {
                    log::warn!("chat: couldn't list the chats: {error}");
                    Vec::new()
                }
                Err(_) => return,
            };
            panel
                .update(cx, |panel, cx| {
                    panel.listing = None;
                    // The root or its trust changed meanwhile.
                    if panel.generation == generation {
                        let ids: Vec<ChatId> = rows.iter().map(|row| row.id).collect();
                        let mut rows = rows;
                        rows.extend(panel.memory_rows(&ids, cx));
                        rows.sort_by_key(|row| std::cmp::Reverse(row.updated));
                        panel.history = Some(rows);
                        cx.notify();
                    }
                })
                .ok();
        }));
    }

    /// The window's own chats of the shown folder, newest first, but for
    /// those in `except`.
    fn memory_rows(&self, except: &[ChatId], cx: &App) -> Vec<HistoryRow> {
        let folder = self.current_folder(cx);
        (self.memory.iter().rev())
            .filter(|(at, chat, _)| *at == folder && !except.contains(&chat.id))
            .map(|(_, chat, updated)| HistoryRow {
                id: chat.id,
                title: chat.title.clone(),
                updated: *updated,
            })
            .collect()
    }

    /// Opens chat `id` from History: a streaming reply is stopped and kept
    /// first.
    fn open_history_chat(&mut self, id: ChatId, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_and_remember(window, cx);
        let shown = self.view.read(cx).conversation().clone();
        // This window's own copy: the shown chat, or what it retained. It is
        // all untrusted History has.
        let folder = self.current_folder(cx);
        let local = if shown.id == id && !shown.turns.is_empty() {
            Some(shown.clone())
        } else {
            (self.memory.iter())
                .find(|(at, chat, _)| *at == folder && chat.id == id)
                .map(|(_, chat, _)| chat.clone())
        };
        let Some(token) = self.current_token(cx) else {
            self.open_chosen(local, id, window, cx);
            return;
        };
        let load = Worker::of(cx).load(token, id);
        let (handle, views, generation) = (self.window, self.views_made, self.generation);
        self.listing = Some(cx.spawn(async move |panel, cx| {
            let result = load.await;
            cx.update_window(handle, |_, window, cx| {
                panel
                    .update(cx, |panel, cx| {
                        panel.listing = None;
                        if panel.views_made != views || panel.generation != generation {
                            return;
                        }
                        // The user went on in the chat meanwhile: it stays as
                        // it is, with its draft.
                        if *panel.view.read(cx).conversation() != shown {
                            return;
                        }
                        match result {
                            Ok((disk, revision)) => {
                                panel.open_loaded(local, disk, revision, folder, window, cx)
                            }
                            Err(WorkerError::Store(error)) if local.is_none() => {
                                toast::show(
                                    cx,
                                    Toast::error(
                                        tr!("workbench-chat-cant-open", error = error.to_string())
                                            .to_string(),
                                    ),
                                );
                            }
                            Err(_) => panel.open_chosen(local, id, window, cx),
                        }
                    })
                    .ok();
            })
            .ok();
        }));
    }

    /// Opens a chat whose file is `disk` at `revision`, given this window's
    /// own copy `local`:
    /// - with no turns the store hasn't acknowledged, the file is the chat,
    ///   even at the same length;
    /// - with some, derived from this revision, the copy is the chat and is
    ///   written;
    /// - with some, derived from another, the file stands and the copy goes
    ///   on as a chat of its own.
    fn open_loaded(
        &mut self,
        local: Option<Conversation>,
        disk: Conversation,
        revision: u64,
        origin: Folder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = disk.id;
        let Some(local) = local else {
            self.adopt_disk(&disk, revision);
            self.show_chat(Some(disk), window, cx);
            return;
        };
        if !self.dirty(&local) {
            self.adopt_disk(&disk, revision);
            if local == disk {
                self.open_chosen(Some(local), id, window, cx);
            } else {
                self.show_chat(Some(disk), window, cx);
            }
            return;
        }
        let state = self.bases.get(&id).cloned();
        let derived = state.as_ref().map_or(0, |state| state.revision());
        let gone_to_copy = state.as_ref().is_some_and(|state| state.has_copy());
        if derived == revision && !gone_to_copy {
            self.open_chosen(Some(local.clone()), id, window, cx);
            if let Some(token) = self.home_token(cx) {
                self.save_chat(token, origin, local, cx);
            }
        } else {
            // Derived from another revision: the file stands. The turns
            // not acknowledged are written, by the worker, as a copy.
            if !gone_to_copy && let Some(token) = self.home_token(cx) {
                self.save_chat(token, origin, local, cx);
            }
            self.adopt_disk(&disk, revision);
            self.show_chat(Some(disk), window, cx);
        }
    }

    /// Shows `chosen` for chat `id`; if that is the chat already shown it
    /// stays as it is, and History closes.
    fn open_chosen(
        &mut self,
        chosen: Option<Conversation>,
        id: ChatId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.view.read(cx).conversation();
        if shown.id == id
            && chosen
                .as_ref()
                .is_some_and(|c| c.turns.len() <= shown.turns.len())
        {
            self.history = None;
            cx.notify();
            return;
        }
        self.show_chat(chosen, window, cx);
    }

    /// Stops a streaming reply, then keeps the chat as it is.
    fn stop_and_remember(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.read(cx).is_streaming() {
            self.view.update(cx, |view, cx| {
                kiln::with_cx(window, cx, |cx| view.stop_reply(cx));
            });
        }
        self.remember_chat(cx);
    }

    /// Asks whether to delete the shown chat, and does.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conversation = self.view.read(cx).conversation();
        if conversation.turns.is_empty() {
            return;
        }
        let (id, title) = (conversation.id, conversation.title.clone());
        // Deleted where it began, whatever has happened to the window since.
        let origin = self.home.clone();
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("workbench-chat-delete-title"),
            Some(&tr!("workbench-chat-delete-detail", title = title.as_str())),
            &[
                tr!("workbench-chat-delete").to_string().as_str(),
                tr!("workbench-chat-cancel").to_string().as_str(),
            ],
            cx,
        );
        let handle = self.window;
        cx.spawn(async move |panel, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            cx.update_window(handle, |_, window, cx| {
                panel
                    .update(cx, |panel, cx| {
                        panel.delete_chat_now(id, origin, window, cx)
                    })
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    /// Deletes chat `id`'s file under `origin`, the folder it began in, or
    /// its in-memory copy, and starts a new chat if it's the shown one. The
    /// worker checks the folder's trust when it runs.
    fn delete_chat_now(
        &mut self,
        id: ChatId,
        origin: Folder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.memory.retain(|(_, chat, _)| chat.id != id);
        if let Some(target) = &origin.target {
            let worker = Worker::of(cx);
            // Under the lease while the chat is still in its folder; else
            // as the user's confirmation, once. Files already written stay
            // when the root turned untrusted, until the user deletes them in
            // a trusted folder.
            let held = (origin == self.home && !self.departed)
                .then_some(self.lease)
                .flatten();
            let token = match held {
                Some(lease) => worker.token_leased(target, lease),
                None => worker.token_once(target),
            };
            if let Some(token) = token {
                let deleted = worker.delete(token, id);
                cx.spawn(async move |panel, cx| {
                    let failure = match deleted.await {
                        Ok(()) => {
                            panel
                                .update(cx, |panel, _| {
                                    panel.acked.remove(&id);
                                    panel.bases.remove(&id);
                                })
                                .ok();
                            return;
                        }
                        Err(WorkerError::Store(error)) => {
                            log::warn!("chat: couldn't delete chat {id}: {error}");
                            error.to_string()
                        }
                        Err(WorkerError::Stale) => tr!("workbench-chat-untrusted").to_string(),
                        Err(_) => return,
                    };
                    panel
                        .update(cx, |_, cx| {
                            toast::show(
                                cx,
                                Toast::error(
                                    tr!("workbench-chat-cant-delete", failure = failure.as_str())
                                        .to_string(),
                                ),
                            );
                        })
                        .ok();
                })
                .detach();
            }
        }
        if self.view.read(cx).conversation().id == id {
            self.start_chat(window, cx);
        }
    }

    /// Shows the key form in place of the chat, focused and empty.
    fn show_key_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.key_form.is_none() {
            self.forms_made += 1;
            let field = cx.new(|cx| {
                TextField::new(window, cx)
                    .placeholder(text!("workbench-chat-key-placeholder"))
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
                        let saving = from.is_some();
                        if let Some(form) = this.key_form.as_mut().filter(|form| owns(form)) {
                            form.saving = false;
                        }
                        toast::show(
                            cx,
                            Toast::error(match saving {
                                true => {
                                    tr!("workbench-chat-cant-save-key", error = error.to_string())
                                        .to_string()
                                }
                                false => {
                                    tr!("workbench-chat-cant-clear-key", error = error.to_string())
                                        .to_string()
                                }
                            }),
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
            .child(div().child(tr!("workbench-chat-key-placeholder").to_string()))
            .child(form.field.clone())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(verbatim!("chat-key-save"), text!("workbench-chat-save"))
                            .small()
                            .debug_selector(|| "chat-key-save".into())
                            .on_click(save),
                    )
                    .child(
                        Button::new(verbatim!("chat-key-cancel"), text!("workbench-chat-cancel"))
                            .small()
                            .debug_selector(|| "chat-key-cancel".into())
                            .on_click(cancel),
                    ),
            )
            .child(note(tr!("workbench-chat-key-note").to_string()))
            .children(env_wins.then(|| {
                note(tr!("workbench-chat-key-env-wins", variable = KEY_VARIABLE).to_string())
            }))
    }

    /// History: the chats, newest first, each a button that opens it.
    fn render_history(&self, rows: &[HistoryRow], cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        let list = v_flex()
            .id("chat-history-list")
            .debug_selector(|| "chat-history-list".into())
            .size_full()
            .gap_1()
            .p_2()
            .overflow_y_scroll();
        if rows.is_empty() {
            return list.child(
                div()
                    .text_color(ui.muted_text)
                    .child(tr!("workbench-chat-no-chats").to_string()),
            );
        }
        list.children(rows.iter().enumerate().map(|(i, row)| {
            let id = row.id;
            let open =
                cx.listener(move |this, _, window, cx| this.open_history_chat(id, window, cx));
            Button::new((verbatim!("chat-history-row"), i), row.label())
                .small()
                .ghost()
                .debug_selector(move || format!("chat-history-row-{i}"))
                .on_click(open)
        }))
    }

    /// The model picker: the models, then the efforts, each a button that
    /// picks it, the chat's own marked.
    fn render_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        let conversation = self.view.read(cx).conversation();
        let choices = picker_choices(conversation);
        let (model_now, effort_now) = (conversation.model.clone(), conversation.effort);
        let heading = |label: Text| {
            div()
                .px_1()
                .pt_1()
                .text_color(ui.muted_text)
                .child(label.resolve())
        };
        let mark = |button: Button, label: String, current: bool| {
            if current {
                button
                    .icon(fe_ui::Icon::Check)
                    .name(tr!("workbench-chat-picker-current", name = label).to_string())
            } else {
                button
            }
        };
        let models = choices.models.into_iter().enumerate().map(|(i, model)| {
            let label = fe_config::chat_models::label(&model).to_owned();
            let current = model == model_now;
            let pick = cx.listener(move |this, _, window, cx| {
                this.pick(Pick::Model(model.clone()), window, cx);
            });
            let button = Button::new(
                (verbatim!("chat-model-row"), i),
                Text::verbatim(label.clone()),
            )
            .small()
            .ghost()
            .debug_selector(move || format!("{}-{i}", verbatim!("chat-model-row")))
            .on_click(pick);
            mark(button, label, current)
        });
        let efforts = choices.efforts.into_iter().enumerate().map(|(i, effort)| {
            let label = effort_label(effort);
            let current = effort == effort_now;
            let pick = cx.listener(move |this, _, window, cx| {
                this.pick(Pick::Effort(effort), window, cx);
            });
            let button = Button::new((verbatim!("chat-effort-row"), i), label.clone())
                .small()
                .ghost()
                .debug_selector(move || format!("{}-{i}", verbatim!("chat-effort-row")))
                .on_click(pick);
            mark(button, label.resolve().to_string(), current)
        });
        v_flex()
            .id("chat-picker-list")
            .debug_selector(|| "chat-picker-list".into())
            .size_full()
            .gap_1()
            .p_2()
            .overflow_y_scroll()
            .child(heading(text!("workbench-chat-picker-model")))
            .children(models)
            .child(heading(text!("workbench-chat-picker-effort")))
            .children(efforts)
    }

    /// Shows or closes the model picker, in place of the chat.
    fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = !self.picker;
        if self.picker {
            (self.history, self.listing) = (None, None);
        }
        cx.notify();
    }

    /// Picks a model or effort for the chat's next turn, closes the picker
    /// and gives the composer focus.
    fn pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| match pick {
            Pick::Model(model) => view.set_model(model, cx),
            Pick::Effort(effort) => view.set_effort(effort, cx),
        });
        self.picker = false;
        let focus = self.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Replaces the chat with a new one, from the settings now.
    fn start_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_chat(None, window, cx);
    }

    /// Replaces the chat with `conversation`, or a new one, and closes
    /// History.
    fn show_chat(
        &mut self,
        conversation: Option<Conversation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.views_made += 1;
        let folder = self.current_folder(cx);
        self.adopt(folder, cx);
        // The list's buttons or the old composer had focus: the new view's
        // composer takes it.
        let refocus = self.history.is_some()
            || self.picker
            || self.view.focus_handle(cx).contains_focused(window, cx);
        self.history = None;
        self.picker = false;
        self.listing = None;
        let (view, options, subscriptions) = Self::chat_view(self.notice, conversation, window, cx);
        (self.view, self.options, self._view) = (view, options, subscriptions);
        self.applies.new_chat();
        self.apply_key(cx);
        self.gather_soon(cx);
        if refocus {
            let focus = self.focus_handle(cx);
            window.focus(&focus, cx);
        }
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
        let folder = self.track_current(cx);
        self.adopt(folder, cx);
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
        self.restore(cx);
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
        // What was being read for the old root or trust doesn't show.
        (self.history, self.listing, self.restoring) = (None, None, None);
        self.picker = false;
        let folder = self.track_current(cx);
        if folder != self.home {
            if self.view.read(cx).conversation().turns.is_empty() {
                self.adopt(folder, cx);
            } else {
                // The chat in view is another folder's: it is written out
                // under its own once, whatever was asked before is
                // refused, and this folder's newest opens in its place.
                self.depart(cx);
                let handle = self.window;
                cx.spawn(async move |panel, cx| {
                    cx.update_window(handle, |_, window, cx| {
                        panel
                            .update(cx, |panel, cx| {
                                panel.show_chat(None, window, cx);
                                panel.restored = false;
                                panel.restore(cx);
                            })
                            .ok();
                    })
                    .ok();
                })
                .detach();
                return;
            }
        }
        if self.view.read(cx).conversation().turns.is_empty() {
            self.restored = false;
        }
        self.restore(cx);
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
        self.current = provider.clone();
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
            Some((KeyOrigin::Environment, _)) => Some(
                tr!("workbench-chat-key-from-env", variable = KEY_VARIABLE)
                    .to_string()
                    .into(),
            ),
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
            ChatViewEvent::TurnFinished => self.remember_chat(cx),
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
            ChatViewEvent::ReviewChanges { turn } => {
                let (panel, turn) = (cx.weak_entity(), *turn);
                window.defer(cx, move |window, cx| {
                    (panel.update(cx, |panel, cx| panel.review_turn(turn, window, cx))).ok();
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

    /// The titles History shows, if it does.
    pub(crate) fn history_titles(&self) -> Option<Vec<String>> {
        let rows = self.history.as_ref()?;
        Some(rows.iter().map(|row| row.title.clone()).collect())
    }

    pub(crate) fn header_note(&self) -> Option<SharedString> {
        self.key_note()
    }

    /// The model picker button's label now.
    pub(crate) fn picker_label(&self, cx: &App) -> String {
        picker_label(self.view.read(cx).conversation())
            .resolve()
            .to_string()
    }

    /// Whether the model picker shows.
    pub(crate) fn picker_open(&self) -> bool {
        self.picker
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
        // Under the buttons, on a line of its own, so the picker beside
        // them never squeezes it into a column.
        let note = self.key_note().map(|note| {
            div()
                .debug_selector(|| "chat-key-note".into())
                .px_2()
                .pb_1()
                .text_color(ui.muted_text)
                .child(note)
        });
        let history = cx.listener(|this, _, _, cx| this.toggle_history(cx));
        let picker = cx.listener(|this, _, _, cx| this.toggle_picker(cx));
        let picker_label = picker_label(self.view.read(cx).conversation());
        let picker_name = tr!(
            "workbench-chat-picker-name",
            current = picker_label.resolve().to_string()
        )
        .to_string();
        let buttons = h_flex()
            .gap_2()
            .px_2()
            .py_1()
            .items_center()
            .child(
                // It takes the room the others leave, and in a narrow
                // panel its label ends in an ellipsis.
                h_flex().flex_1().min_w_0().child(
                    Button::new(verbatim!("chat-model-picker"), picker_label)
                        .small()
                        .flex_shrink(1.)
                        .min_w_0()
                        .name(picker_name)
                        .debug_selector(|| "chat-model-picker".into())
                        .on_click(picker),
                ),
            )
            .child(
                Button::new(verbatim!("chat-history"), text!("workbench-chat-history"))
                    .small()
                    .debug_selector(|| "chat-history".into())
                    .on_click(history),
            )
            .child(
                Button::new(verbatim!("chat-new"), text!("workbench-chat-new"))
                    .small()
                    .debug_selector(|| "chat-new".into())
                    .on_click(new_chat),
            );
        let header = v_flex()
            .border_b_1()
            .border_color(ui.border)
            .child(buttons)
            .children(note);
        let root = v_flex()
            .size_full()
            .child(header)
            .child(match &self.key_form {
                Some(form) => div()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_key_form(form, window, cx)),
                None => match &self.history {
                    Some(rows) => div()
                        .flex_1()
                        .min_h_0()
                        .child(self.render_history(rows, cx)),
                    None if self.picker => div().flex_1().min_h_0().child(self.render_picker(cx)),
                    None => div().flex_1().min_h_0().child(self.view.clone()),
                },
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
    let path = chat_context::shown_path(root, model.path());
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

/// Whether the app's decisions trust the folder at canonical `root`, its own
/// or an ancestor's; a window with no folder has no trust to ask.
fn folder_trusted(root: Option<&Path>, cx: &App) -> bool {
    root.is_none_or(|root| kiln_shell::trust::decided(root, cx) == Some(true))
}

/// Sets the trust of every folder the chat store tracks from the app's
/// decisions, shown in a window or not.
fn refresh_trust(cx: &mut App) {
    let worker = Worker::of(cx);
    let cx: &App = cx;
    worker.refresh(|target| folder_trusted(target.root.as_deref(), cx));
}

/// Has the worker track `workbench`'s folder.
fn track_workbench(workbench: &Workbench, cx: &mut App) {
    let Some(state) = state_dir(cx) else {
        return;
    };
    let root = workbench.root().map(canonical_root);
    let trusted = folder_trusted(root.as_deref(), cx);
    let target = Target {
        dir: folder_dir(&state, root.as_deref()),
        root,
    };
    Worker::of(cx).track(&target, trusted);
}

/// The chat's provider and settings for the window's `workbench`, the same
/// Claude client and key the chat uses; `None` without a key. The key is
/// read first if the chat never drew, which an environment key answers at
/// once and the keychain's does a moment later.
pub(crate) fn provider(
    workbench: &Workbench,
    cx: &mut App,
) -> Option<(Arc<dyn ChatProvider>, ChatSettings)> {
    let panel = workbench.dock().read(cx).chat().cloned()?;
    panel.update(cx, |panel, cx| {
        if !panel.key_read {
            panel.reload_key(cx);
        }
    });
    let provider = panel.read(cx).current.clone()?;
    Some((provider, new_chat_settings(cx).0))
}

/// Runs `chat.open`, `chat.set_api_key` and `chat.clear_api_key` in the
/// window they were invoked in, wherever focus is: the workbench's root
/// doesn't attach the panel's commands. After the dispatch, which has the
/// window out.
pub(crate) fn init(cx: &mut App) {
    // The worker's view of each folder's trust follows the app's decisions,
    // not a window's: an unresolved window is no revocation, and a folder
    // no window shows is followed too.
    cx.observe_new(|workbench: &mut Workbench, _, cx| {
        let this = cx.entity();
        cx.subscribe(&this, |workbench, _, _: &RootChanged, cx| {
            track_workbench(workbench, cx);
        })
        .detach();
        kiln_shell::trust::observe(cx, |_, cx| refresh_trust(cx)).detach();
        track_workbench(workbench, cx);
    })
    .detach();
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

#[cfg(test)]
#[path = "chat_picker_tests.rs"]
mod chat_picker_tests;
