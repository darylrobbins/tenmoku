//! [`ChatView`]: the transcript above, the attachments and the composer
//! below, and the stream that fills the last turn.

use std::collections::{HashMap, HashSet};
use std::mem::{Discriminant, discriminant};
use std::sync::Arc;

use fe_buffer::{Buffer, BufferEvent, EditKind, Origin, Subscription as BufferSubscription};
use fe_chat::{
    Attachment, AttachmentKind, BlockKind, Budgeted, ChatEvent, ChatProvider, ChatSettings,
    ContextItem, ContextKind, Conversation, ErrorKind, Fit, ProviderError, Turn, TurnState, budget,
    build_request, render_user_content,
};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, editor_palette};
use futures::StreamExt as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, canvas, div,
    prelude::FluentBuilder as _, px,
};
use kiln::{Cx, ViewCommands};
use kiln_ui::{Button, ScrollArea, UiTheme, h_flex, ui_theme, v_flex};
use serde_json::Value;

use crate::{
    COMPOSER_CONTEXT,
    reply::{self, CodeBlock, Documents},
};

/// The context window the meter measures against.
const CONTEXT_WINDOW: u64 = 1_000_000;

/// How many rows the composer grows to before it scrolls.
const COMPOSER_ROWS: u32 = 10;

/// How near the bottom, in pixels, the transcript counts as at the tail.
const TAIL_SLACK: f32 = 4.;

/// The context budget until the host sets one: `chat.context_budget`'s
/// default.
const CONTEXT_BUDGET: u32 = 32_000;

/// What the view sends with each request that the conversation doesn't
/// fix itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatOptions {
    /// The most tokens a reply may take (`chat.max_tokens`).
    pub max_tokens: u32,
    /// Whether the provider may fall back to another model
    /// (`chat.fallbacks`), which the first-run notice says.
    pub fallbacks: bool,
}

impl Default for ChatOptions {
    fn default() -> ChatOptions {
        ChatOptions {
            max_tokens: ChatSettings::default().max_tokens,
            fallbacks: true,
        }
    }
}

/// What the view asks its host for, and tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatViewEvent {
    /// The user asked to set the API key: no key, or a 401.
    SetKeyRequested,
    /// The user asked for the chat settings: a 404 names a model the key
    /// can't use.
    SettingsRequested,
    /// The user dismissed the first-run notice.
    NoticeDismissed,
    /// A turn ended, however it did: done, stopped, refused or failed.
    TurnFinished,
    /// The user asked to attach a file: typed `@` at the start of a word,
    /// which the view takes back out, or pressed Attach.
    AttachFileRequested,
    /// The user clicked a link in a reply to `path`, relative to the
    /// project, with a `#L12` fragment naming `row`, 1-based.
    OpenPath { path: String, row: Option<u32> },
    /// The user pressed Apply on code block `block` of turn `turn`'s reply:
    /// see [`ChatView::code_block`].
    ApplyBlock { turn: usize, block: usize },
    /// The user pressed Review Changes on turn `turn`'s footer: its reply
    /// holds two or more code blocks for files.
    ReviewChanges { turn: usize },
}

/// How a turn was made in a view: what [`ChatView::was_sent`] and
/// [`ChatView::retry_of`] answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TurnOrigin {
    /// Sent from the composer, with the chips there then.
    Sent,
    /// Retry sent turn `of` again, with its context as still authorized.
    Retry(usize),
}

/// The record's § 14 line under a turn, and what its button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    pub action: Option<BannerAction>,
    /// Whether it reports an error, shown in the error colour.
    pub error: bool,
}

/// A banner's button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerAction {
    /// Set API Key: emits [`ChatViewEvent::SetKeyRequested`].
    SetKey,
    /// Open Settings: emits [`ChatViewEvent::SettingsRequested`].
    OpenSettings,
    /// Retry: [`RetryTurn`].
    Retry,
    /// Edit: the turn's question back in the composer and its attachments
    /// back as chips, to change and send again.
    Edit,
    /// Remove Attachments: as Edit, for a request too large, so that some
    /// of its attachments can be taken off before it goes again.
    RemoveAttachments,
}

impl BannerAction {
    fn label(self) -> &'static str {
        match self {
            BannerAction::SetKey => "Set API Key",
            BannerAction::OpenSettings => "Open Settings",
            BannerAction::Retry => "Retry",
            BannerAction::Edit => "Edit",
            BannerAction::RemoveAttachments => "Remove Attachments",
        }
    }
}

/// What a turn shows, as the transcript draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnDisplay {
    /// The user's question: the last block of what was sent.
    pub question: String,
    /// The attachments' labels.
    pub attachments: Vec<String>,
    /// The summarised reasoning, when the chat shows thinking and the
    /// reply has some.
    pub thinking: Option<String>,
    /// The reply's text as it streamed.
    pub reply: String,
    /// The model that answered, when it isn't the one asked for.
    pub answered_by: Option<String>,
    pub banner: Option<Banner>,
}

/// A chat: its transcript, the attachments waiting to be sent and a
/// composer, over `provider`.
pub struct ChatView {
    provider: Option<Arc<dyn ChatProvider>>,
    conversation: Conversation,
    options: ChatOptions,
    composer: Entity<EditorView>,
    /// The composer buffer's edits, to tell an `@` typed.
    composer_edits: BufferSubscription,
    /// What the user attached, in the order attached, and those still
    /// being read.
    attachments: Vec<Slot>,
    /// The next [`ChatView::reserve`]'s id.
    next_slot: u64,
    /// What the host gathers by itself: see [`ChatView::set_context`].
    context: Vec<ContextItem>,
    /// The kinds of the host's items the user took off, until the next
    /// send.
    dismissed: HashSet<Discriminant<ContextKind>>,
    /// The user's items the user took off, until the next send: the host's
    /// they stand for stay off with them, gathered then or later.
    removed: Vec<ContextItem>,
    /// For each turn sent from this view, by index, which of its items
    /// the user attached, rather than the host gathered: Edit brings back
    /// only those.
    attached: HashMap<usize, Vec<bool>>,
    /// The authorization generation the host set ([`ChatView::set_authorization`])
    /// when each turn, by index, was sent: its context is only good for a
    /// Retry under the same one.
    sent_under: HashMap<usize, u64>,
    /// How each turn made in this view, by index, was made: see
    /// [`ChatView::was_sent`] and [`ChatView::retry_of`].
    origins: HashMap<usize, TurnOrigin>,
    /// The authorization generation now: the host counts a change of root,
    /// of trust or of `buffer.read` grant.
    authorization: u64,
    /// The tokens a turn's context may take.
    context_budget: u32,
    notice: bool,
    /// The reply streaming in: dropping it drops the stream, which cancels
    /// the request.
    stream: Option<Task<()>>,
    scroll: ScrollHandle,
    tail: Tail,
    /// The turns whose thinking is open.
    thinking_open: HashSet<usize>,
    /// The replies and thinking as Markdown, parsed as they're drawn.
    documents: Documents,
    _composer_changed: Subscription,
}

impl EventEmitter<ChatViewEvent> for ChatView {}

/// One of the user's attachments: ready, or still being read.
#[derive(Clone, Debug)]
enum Slot {
    Ready(ContextItem),
    Reading {
        id: u64,
        kind: ContextKind,
        label: String,
    },
}

impl Slot {
    fn ready(&self) -> Option<&ContextItem> {
        match self {
            Slot::Ready(item) => Some(item),
            Slot::Reading { .. } => None,
        }
    }
}

#[kiln::command(context = "chat")]
impl ChatView {
    /// Send Message
    ///
    /// Sends the composer's text with the chips' items as the context
    /// budget leaves them, and empties the composer and the attachments.
    /// The host's items the user took off come back. Does nothing with no
    /// text, no provider, while a reply streams or while an attachment is
    /// being read.
    #[command(id = "chat.send")]
    pub fn send_message(&mut self, cx: &mut Cx<Self>) {
        let text = self.composer_text(cx);
        if text.trim().is_empty()
            || self.is_streaming()
            || self.provider.is_none()
            || self.reading()
        {
            return;
        }
        let hosts = self.chip_items().len() - self.attachments().len();
        let (items, mine): (Vec<ContextItem>, Vec<bool>) = (self.chips().into_iter())
            .filter(Budgeted::sent)
            .map(|chip| (chip.item, chip.index >= hosts))
            .unzip();
        self.attachments.clear();
        self.dismissed.clear();
        self.removed.clear();
        let user_content = render_user_content(&text, &items);
        let attachments = items.iter().map(attachment_of).collect();
        self.set_composer_text("", cx);
        self.attached.insert(self.conversation.turns.len(), mine);
        self.sent_under
            .insert(self.conversation.turns.len(), self.authorization);
        self.start_turn(TurnOrigin::Sent, user_content, attachments, cx);
    }

    /// Stop Reply
    ///
    /// Stops the reply streaming in, keeping what came.
    #[command(id = "chat.stop")]
    pub fn stop_reply(&mut self, cx: &mut Cx<Self>) {
        if !self.is_streaming() {
            return;
        }
        self.stream = None;
        self.conversation.stop();
        self.end_turn(cx);
    }

    /// Retry
    ///
    /// Sends the last turn again as a new turn, if it failed or was
    /// refused.
    #[command(id = "chat.retry")]
    pub fn retry_turn(&mut self, cx: &mut Cx<Self>) {
        if self.is_streaming() || self.provider.is_none() {
            return;
        }
        let Some(turn) = self
            .conversation
            .turns
            .last()
            .filter(|turn| retryable(turn))
        else {
            return;
        };
        // The context is rebuilt from what is authorized now, not copied
        // from the failed turn: the host's items stand only while the host
        // still gathers them, and the user's only while the host hasn't
        // cleared them (a root, trust or `buffer.read` change does).
        let last = self.conversation.turns.len() - 1;
        let question = question_of(turn);
        let flags = self.attached.get(&last).cloned().unwrap_or_default();
        // Nothing sent under another authorization generation is sent
        // again, whatever it matches now: the same relative path may be
        // another root's file.
        let current = self.sent_under.get(&last) == Some(&self.authorization);
        let (items, mine): (Vec<ContextItem>, Vec<bool>) = items_of(turn)
            .into_iter()
            .filter(|_| current)
            .zip(flags.into_iter().chain(std::iter::repeat(false)))
            .filter(|(item, mine)| {
                *mine
                    || (self.context.iter())
                        .any(|host| host.kind == item.kind || covers(item, host))
            })
            .unzip();
        let user_content = render_user_content(&question, &items);
        let attachments = items.iter().map(attachment_of).collect();
        self.attached.insert(last + 1, mine);
        self.sent_under.insert(last + 1, self.authorization);
        self.start_turn(TurnOrigin::Retry(last), user_content, attachments, cx);
    }
}

impl ChatView {
    /// A view of `conversation` over `provider`.
    pub fn new(
        provider: Arc<dyn ChatProvider>,
        conversation: Conversation,
        options: ChatOptions,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ChatView {
        let theme = EditorTheme::from_palette(&editor_palette(cx), &EditorSettings::default(), cx);
        let composer = cx.new(|cx| {
            let mut model = EditorModel::new(Buffer::new(""), None);
            model.set_soft_wrap(true);
            let mut view = EditorView::new(model, theme, cx);
            view.set_gutter(false, cx);
            view.set_vim_off(true, cx);
            view
        });
        let composer_edits = composer.update(cx, |composer, _| {
            composer.model_mut().buffer_mut().subscribe()
        });
        // It grows as it's typed into.
        let composer_changed = cx.observe(&composer, |this, _, cx| {
            this.composer_edited(cx);
            cx.notify();
        });
        ChatView {
            provider: Some(provider),
            conversation,
            options,
            composer,
            composer_edits,
            attachments: Vec::new(),
            next_slot: 0,
            context: Vec::new(),
            dismissed: HashSet::new(),
            removed: Vec::new(),
            attached: HashMap::new(),
            sent_under: HashMap::new(),
            origins: HashMap::new(),
            authorization: 0,
            context_budget: CONTEXT_BUDGET,
            notice: false,
            stream: None,
            scroll: ScrollHandle::new(),
            tail: Tail::Off,
            thinking_open: HashSet::new(),
            documents: Documents::default(),
            _composer_changed: composer_changed,
        }
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// Sets the provider, or with `None` shows the no-key state: the
    /// composer disabled and a Set API Key button. A reply already
    /// streaming goes on from the provider it came from.
    pub fn set_provider(
        &mut self,
        provider: Option<Arc<dyn ChatProvider>>,
        cx: &mut Context<Self>,
    ) {
        let read_only = provider.is_none();
        self.provider = provider;
        self.composer
            .update(cx, |composer, cx| composer.set_read_only(read_only, cx));
        cx.notify();
    }

    /// Adds `item` to what the next message sends, as a chip, until it's
    /// sent or taken off.
    pub fn attach(&mut self, item: ContextItem, cx: &mut Context<Self>) {
        self.attachments.push(Slot::Ready(item));
        cx.notify();
    }

    /// Keeps the place of an attachment the host is still reading, its
    /// chip saying so under `label`, and gives back the id to
    /// [`ChatView::fill`] it by. Nothing is sent until it's filled.
    pub fn reserve(&mut self, kind: ContextKind, label: String, cx: &mut Context<Self>) -> u64 {
        let id = self.next_slot;
        self.next_slot += 1;
        self.attachments.push(Slot::Reading { id, kind, label });
        cx.notify();
        id
    }

    /// Puts `item` in reservation `id`'s place, or with `None` takes the
    /// reservation off. Does nothing if it's gone: taken off meanwhile.
    pub fn fill(&mut self, id: u64, item: Option<ContextItem>, cx: &mut Context<Self>) {
        let at = (self.attachments.iter())
            .position(|slot| matches!(slot, Slot::Reading { id: reading, .. } if *reading == id));
        let Some(at) = at else {
            return;
        };
        match item {
            Some(item) => self.attachments[at] = Slot::Ready(item),
            None => {
                self.attachments.remove(at);
            }
        }
        cx.notify();
    }

    /// Whether an attachment is still being read.
    pub fn reading(&self) -> bool {
        (self.attachments.iter()).any(|slot| matches!(slot, Slot::Reading { .. }))
    }

    /// What the user attached to the next message, read.
    pub fn attachments(&self) -> Vec<ContextItem> {
        (self.attachments.iter())
            .filter_map(Slot::ready)
            .cloned()
            .collect()
    }

    /// Sets what the host gathered by itself for the next message, in place
    /// of what it gathered before. Each shows as a chip unless the user
    /// took one of its kind off since the last send.
    pub fn set_context(&mut self, items: Vec<ContextItem>, cx: &mut Context<Self>) {
        if self.context != items {
            self.context = items;
            cx.notify();
        }
    }

    /// What the host gathered, whether its chips show or not.
    pub fn context(&self) -> &[ContextItem] {
        &self.context
    }

    /// Sets the tokens a turn's context may take (`chat.context_budget`).
    pub fn set_context_budget(&mut self, limit: u32, cx: &mut Context<Self>) {
        self.context_budget = limit;
        cx.notify();
    }

    /// The chips, in the order sent: the host's items the user hasn't
    /// taken off, then the user's, as the context budget leaves them.
    /// Each one's `index` is into that list, the host's first.
    pub fn chips(&self) -> Vec<Budgeted> {
        budget(&self.chip_items(), self.context_budget)
    }

    /// What chip `index` says: its label, its estimate, and whether it's
    /// cut or left out.
    pub fn chip_text(&self, index: usize) -> Option<String> {
        self.chips().get(index).map(chip_text)
    }

    /// The host's items the user hasn't taken off nor attached themselves,
    /// then the user's read ones.
    fn chip_items(&self) -> Vec<ContextItem> {
        let attached = self.attachments();
        let mut items: Vec<ContextItem> = (self.context.iter())
            .filter(|item| !self.dismissed.contains(&discriminant(&item.kind)))
            .filter(|item| !attached.iter().any(|mine| covers(mine, item)))
            .filter(|item| !self.removed.iter().any(|mine| covers(mine, item)))
            .cloned()
            .collect();
        items.extend(attached);
        items
    }

    /// Takes chip `index` off, as its remove button does: one of the
    /// user's goes, and with it until the next send the host's it stands
    /// for, gathered now or later; one of the host's goes with its kind
    /// until the next send.
    pub fn remove_chip(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(chip) = self.chips().into_iter().nth(index) else {
            return;
        };
        let items = self.chip_items();
        let hosts = items.len() - self.attachments().len();
        if chip.index < hosts {
            self.dismissed.insert(discriminant(&items[chip.index].kind));
        } else {
            let mine = chip.index - hosts;
            let at = (self.attachments.iter().enumerate())
                .filter(|(_, slot)| slot.ready().is_some())
                .nth(mine)
                .map(|(at, _)| at);
            if let Some(at) = at {
                self.attachments.remove(at);
            }
            self.removed.push(items[chip.index].clone());
        }
        cx.notify();
    }

    /// Takes off the attachment still being read under chip
    /// `chat-chip-reading-{index}`: as for one read, the host's it stands
    /// for stay off with it until the next send.
    fn remove_reading(&mut self, index: usize, cx: &mut Context<Self>) {
        let at = (self.attachments.iter().enumerate())
            .filter(|(_, slot)| slot.ready().is_none())
            .nth(index)
            .map(|(at, _)| at);
        let Some(at) = at else {
            return;
        };
        if let Slot::Reading { kind, label, .. } = self.attachments.remove(at) {
            let text = String::new();
            self.removed.push(ContextItem { kind, label, text });
        }
        cx.notify();
    }

    /// Takes off everything the user attached, read or being read, as
    /// when what they were read under no longer stands; and what earlier
    /// turns were sent with, which Edit and Remove Attachments then don't
    /// bring back.
    pub fn clear_attachments(&mut self, cx: &mut Context<Self>) {
        self.attachments.clear();
        self.attached.clear();
        cx.notify();
    }

    /// Sets the authorization generation, which the host changes whenever
    /// the root, its trust or the `buffer.read` grant does. A Retry sends
    /// no context a turn was sent with under another one.
    pub fn set_authorization(&mut self, generation: u64) {
        self.authorization = generation;
    }

    /// Shows or hides the first-run notice.
    pub fn set_notice(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.notice = shown;
        cx.notify();
    }

    /// The composer, for the host to apply the editor's settings to.
    pub fn composer(&self) -> &Entity<EditorView> {
        &self.composer
    }

    /// The composer's text.
    pub fn composer_text(&self, cx: &App) -> String {
        self.composer.read(cx).model().text().to_string()
    }

    /// Replaces the composer's text with `text`, as an undo step of its own.
    pub fn set_composer_text(&mut self, text: &str, cx: &mut App) {
        self.composer.update(cx, |composer, cx| {
            let len = composer.model().text().len();
            let mut buffer = composer.model_mut().buffer_mut();
            buffer.with_transaction_aside(|buffer| buffer.edit([(0..len, text)], Origin::User));
            drop(buffer);
            cx.notify();
        });
        // Not typed.
        self.composer_edits.drain();
    }

    /// After the composer changed: an `@` typed at the start of a word,
    /// over a selection or not, asks the host for a file to attach, and is
    /// taken back out within the edit that typed it, so undo neither brings
    /// it back nor asks again. Undo and redo aren't typing.
    fn composer_edited(&mut self, cx: &mut Context<Self>) {
        let Some(BufferEvent::Edited {
            transaction,
            kind: EditKind::Edit,
            edits,
            text,
            ..
        }) = self.composer_edits.drain().pop()
        else {
            return;
        };
        let [edit] = &edits[..] else {
            return;
        };
        let at = edit.new.start;
        if edit.new.len() != 1 || !text.range_eq(edit.new.clone(), "@") || !word_start(&text, at) {
            return;
        }
        let removed = self.composer.update(cx, |composer, cx| {
            let mut buffer = composer.model_mut().buffer_mut();
            // Only if it's still there: the last edit was this one.
            let removed = buffer.text().range_eq(at..at + 1, "@")
                && buffer.edit_in(transaction, [(at..at + 1, "")]).is_ok();
            drop(buffer);
            cx.notify();
            removed
        });
        self.composer_edits.drain();
        if removed {
            cx.emit(ChatViewEvent::AttachFileRequested);
        }
    }

    /// Whether a reply is streaming in.
    pub fn is_streaming(&self) -> bool {
        self.stream.is_some()
    }

    /// Whether turn `index`'s thinking is open.
    pub fn thinking_open(&self, index: usize) -> bool {
        self.thinking_open.contains(&index)
    }

    /// Opens or closes turn `index`'s thinking, as clicking its toggle does.
    pub fn set_thinking_open(&mut self, index: usize, open: bool, cx: &mut Context<Self>) {
        if open {
            self.thinking_open.insert(index);
        } else {
            self.thinking_open.remove(&index);
        }
        cx.notify();
    }

    /// What turn `index` shows.
    pub fn turn_display(&self, index: usize) -> Option<TurnDisplay> {
        let turn = self.conversation.turns.get(index)?;
        let thinking = turn.thinking();
        let answered_by =
            serving_model(turn).filter(|model| *model != self.conversation.model.as_str());
        Some(TurnDisplay {
            question: question_of(turn),
            attachments: turn.attachments.iter().map(|a| a.label.clone()).collect(),
            thinking: (self.conversation.show_thinking && !thinking.is_empty()).then_some(thinking),
            reply: turn.text(),
            answered_by: answered_by.map(str::to_owned),
            banner: self.banner(index, turn),
        })
    }

    /// Whether turn `index` was sent from this view's composer, with the
    /// chips it showed then: by `chat.send`, from a key, a command or the
    /// Send button. Recorded as the turn is made; `false` for a retry, and
    /// for a turn the view didn't make, such as one read back from disk.
    pub fn was_sent(&self, index: usize) -> bool {
        self.origins.get(&index) == Some(&TurnOrigin::Sent)
    }

    /// The turn that turn `index` sent again, if a Retry made it, from a
    /// key, a command or the banner's button. Recorded as the turn is made.
    pub fn retry_of(&self, index: usize) -> Option<usize> {
        match self.origins.get(&index)? {
            TurnOrigin::Retry(of) => Some(*of),
            TurnOrigin::Sent => None,
        }
    }

    /// Code block `block` of turn `index`'s reply, as its header shows it:
    /// blocks are counted in document order, those in lists and quotes
    /// too, as [`ChatViewEvent::ApplyBlock`] counts them.
    pub fn code_block(&self, index: usize, block: usize) -> Option<CodeBlock> {
        let turn = self.conversation.turns.get(index)?;
        reply::code_blocks(&fe_markdown::parse(&turn.text()))
            .into_iter()
            .nth(block)
    }

    /// How many times the view has parsed a reply or a thinking summary
    /// as Markdown: at most once a frame for the turn streaming, since it
    /// parses as it draws, and never again for a turn whose text hasn't
    /// changed.
    pub fn parses(&self) -> usize {
        self.documents.parses()
    }

    /// The footer's meter, from the last reply's prompt size: `<n>k of 1M
    /// tokens`, and a nudge to start again past 80 %. `None` before any
    /// reply reported its usage.
    pub fn meter(&self) -> Option<String> {
        let usage = self
            .conversation
            .turns
            .iter()
            .rev()
            .find_map(|turn| turn.usage)?;
        let total = usage.total_input();
        let mut meter = format!("{}k of 1M tokens", total.div_ceil(1000));
        if total * 5 >= CONTEXT_WINDOW * 4 {
            meter.push_str(" · Long chat: start a new one for better answers");
        }
        Some(meter)
    }

    /// The record's § 14 banner for turn `index`. Retry and Edit are offered
    /// on the last turn only, since they send it again.
    fn banner(&self, index: usize, turn: &Turn) -> Option<Banner> {
        let last = index + 1 == self.conversation.turns.len();
        let only_last = |action: BannerAction| last.then_some(action);
        let banner = |text: String, action, error| Banner {
            text,
            action,
            error,
        };
        match &turn.state {
            TurnState::Streaming | TurnState::Stopped => None,
            TurnState::Done if turn.cut_off() => Some(banner(
                "Reply cut off at the token limit".to_owned(),
                None,
                false,
            )),
            TurnState::Done => None,
            TurnState::Refused => {
                let category = (turn.stop_details.as_ref())
                    .and_then(|details| details.get("category"))
                    .and_then(|category| category.as_str());
                let text = match category {
                    Some(category) => format!("Claude declined this request ({category})"),
                    None => "Claude declined this request".to_owned(),
                };
                Some(banner(text, only_last(BannerAction::Edit), false))
            }
            TurnState::Failed(error) => {
                let (text, action) = self.failure(error);
                let action = action.filter(|action| {
                    last || matches!(action, BannerAction::SetKey | BannerAction::OpenSettings)
                });
                Some(banner(text, action, true))
            }
        }
    }

    /// A failure's line and action, by its kind (the record's § 14).
    fn failure(&self, error: &ProviderError) -> (String, Option<BannerAction>) {
        let message = error.message.clone();
        match error.kind {
            ErrorKind::Auth => (
                "Your API key was rejected".to_owned(),
                Some(BannerAction::SetKey),
            ),
            ErrorKind::Billing | ErrorKind::Permission => (message, None),
            ErrorKind::NotFound => (
                format!(
                    "Model `{}` isn't available to this key",
                    self.conversation.model
                ),
                Some(BannerAction::OpenSettings),
            ),
            ErrorKind::TooLarge => (
                "Too much context".to_owned(),
                Some(BannerAction::RemoveAttachments),
            ),
            ErrorKind::InvalidRequest => (message, Some(BannerAction::Edit)),
            ErrorKind::Network
            | ErrorKind::RateLimited
            | ErrorKind::Overloaded
            | ErrorKind::Server => (message, Some(BannerAction::Retry)),
            ErrorKind::Stream => (format!("Interrupted: {message}"), Some(BannerAction::Retry)),
        }
    }

    /// Begins a turn of `user_content`, made as `origin` says, and streams
    /// its reply.
    fn start_turn(
        &mut self,
        origin: TurnOrigin,
        user_content: Vec<Value>,
        attachments: Vec<Attachment>,
        cx: &mut Cx<Self>,
    ) {
        let Some(provider) = self.provider.clone() else {
            return;
        };
        let settings = ChatSettings {
            model: self.conversation.model.clone(),
            effort: self.conversation.effort,
            show_thinking: self.conversation.show_thinking,
            max_tokens: self.options.max_tokens,
        };
        let request = build_request(&self.conversation, user_content.clone(), &settings);
        self.origins.insert(self.conversation.turns.len(), origin);
        self.conversation.begin_turn(user_content, attachments);
        // The request is made and its stream polled on the background
        // executor, as connecting and checking certificates can block, and
        // the events come back over a channel. Dropping the task below
        // drops this one, and with it the stream, which cancels.
        let (sender, mut events) = futures::channel::mpsc::unbounded();
        let pump = cx.background_spawn(async move {
            let mut stream = provider.stream(request);
            while let Some(event) = stream.next().await {
                if sender.unbounded_send(event).is_err() {
                    break;
                }
            }
        });
        self.tail = Tail::Jump;
        self.stream = Some(cx.spawn(async move |this, cx| {
            let pump = pump;
            while let Some(event) = events.next().await {
                let streaming = this.update(cx, |this, cx| {
                    this.conversation.apply(event);
                    cx.notify();
                    this.conversation.streaming_turn().is_some()
                });
                match streaming {
                    Ok(true) => {}
                    Ok(false) => break,
                    // The view is gone.
                    Err(_) => return,
                }
            }
            drop(pump);
            this.update(cx, |this, cx| {
                // A stream that ends without saying how was cut off.
                let error = ProviderError::new(ErrorKind::Stream, "the reply ended early");
                this.conversation.apply(ChatEvent::Failed(error));
                this.stream = None;
                this.end_turn(cx);
            })
            .ok();
        }));
        cx.notify();
    }

    fn end_turn(&mut self, cx: &mut Context<Self>) {
        cx.emit(ChatViewEvent::TurnFinished);
        cx.notify();
    }

    fn run_banner_action(&mut self, action: BannerAction, index: usize, cx: &mut Cx<Self>) {
        match action {
            BannerAction::SetKey => cx.emit(ChatViewEvent::SetKeyRequested),
            BannerAction::OpenSettings => cx.emit(ChatViewEvent::SettingsRequested),
            BannerAction::Retry => self.retry_turn(cx),
            BannerAction::Edit | BannerAction::RemoveAttachments => {
                let Some(turn) = self.conversation.turns.get(index) else {
                    return;
                };
                let question = question_of(turn);
                let items = items_of(turn);
                // Only what the user attached comes back: what the host
                // gathered is gathered again, if it still may be. Unknown
                // for a turn this view didn't send, so none.
                let mine = self.attached.get(&index).cloned().unwrap_or_default();
                self.set_composer_text(&question, cx.cx);
                for (item, mine) in items.into_iter().zip(mine) {
                    let gathered = self.context.iter().any(|host| covers(&item, host));
                    if mine && !gathered && !self.attachments().contains(&item) {
                        self.attachments.push(Slot::Ready(item));
                    }
                }
                cx.notify();
                let focus = self.composer.focus_handle(cx.cx);
                cx.window.focus(&focus, cx.cx);
            }
        }
    }

    /// Follows the tail while a reply streams, unless the user scrolled
    /// up, through the frame that draws how it ended.
    fn follow_tail(&mut self) {
        match self.tail {
            Tail::Off => return,
            Tail::Jump => self.scroll.scroll_to_bottom(),
            Tail::Follow => {
                // Where the last frame left it: the user scrolled away, or
                // it's still at the bottom it was scrolled to.
                let (offset, max) = (self.scroll.offset().y, self.scroll.max_offset().y);
                if -offset < max - px(TAIL_SLACK) {
                    self.tail = Tail::Off;
                    return;
                }
                self.scroll.scroll_to_bottom();
            }
        }
        self.tail = if self.is_streaming() {
            Tail::Follow
        } else {
            Tail::Off
        };
    }

    fn render_turn(&mut self, index: usize, ui: &UiTheme, cx: &mut Context<Self>) -> AnyElement {
        let Some(shown) = self.turn_display(index) else {
            return div().into_any_element();
        };
        let streaming = self.conversation.turns[index].state == TurnState::Streaming;
        let chips = shown
            .attachments
            .iter()
            .map(|label| chip(label.clone(), ui).into_any_element());
        let user = v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(ui.row_selected_inactive)
            .text_color(ui.tab_active_text)
            .when(!shown.attachments.is_empty(), |user| {
                user.child(h_flex().flex_wrap().gap_1().children(chips))
            })
            .child(shown.question.clone());
        let style = reply::reply_style(self.composer.read(cx).theme(), ui);
        let thinking = shown.thinking.clone().map(|text| {
            let open = self.thinking_open(index);
            let document = open.then(|| self.documents.thinking(index, &text, !streaming));
            let toggle = cx.listener(move |this, _, _, cx| {
                if !this.thinking_open.remove(&index) {
                    this.thinking_open.insert(index);
                }
                cx.notify();
            });
            v_flex()
                .gap_1()
                .child(
                    div()
                        .id(("chat-thinking", index))
                        .debug_selector(move || format!("chat-thinking-{index}"))
                        .cursor_pointer()
                        .text_color(ui.muted_text)
                        .child(if open { "▾ Thinking" } else { "▸ Thinking" })
                        .on_click(toggle),
                )
                .children(document.map(|document| {
                    let style = reply::thinking_style(&style);
                    // Its own id scope, as the reply's: each document numbers
                    // its texts and code blocks from 0.
                    div()
                        .id(("chat-thinking-text", index))
                        .debug_selector(move || format!("chat-thinking-text-{index}"))
                        .pl_3()
                        .border_l_1()
                        .border_color(ui.border)
                        .child(reply::thinking_element(&document, &style, cx.weak_entity()))
                }))
        });
        let reply = if shown.reply.is_empty() && streaming {
            div().text_color(ui.text).child("…").into_any_element()
        } else {
            let document = self.documents.reply(index, &shown.reply, !streaming);
            let view = cx.weak_entity();
            reply::reply_element(index, &document, &style, streaming, *ui, view)
        };
        let answered_by = shown.answered_by.clone().map(|model| {
            div()
                .debug_selector(move || format!("chat-answered-by-{index}"))
                .text_color(ui.muted_text)
                .child(format!("Answered by {model}"))
        });
        let banner = shown
            .banner
            .clone()
            .map(|banner| self.render_banner(index, banner, ui, cx));
        let review = if streaming || shown.reply.is_empty() {
            None
        } else {
            let document = self.documents.reply(index, &shown.reply, true);
            let files = reply::path_blocks(&document);
            (files >= 2).then(|| {
                let ask = cx.listener(move |_, _, _, cx| {
                    cx.emit(ChatViewEvent::ReviewChanges { turn: index })
                });
                h_flex().child(
                    Button::new(
                        ("chat-review-changes", index),
                        format!("Review {files} changes"),
                    )
                    .small()
                    .debug_selector(move || format!("chat-review-changes-{index}"))
                    .on_click(ask),
                )
            })
        };
        v_flex()
            .debug_selector(move || format!("chat-turn-{index}"))
            .gap_2()
            .child(user)
            .children(thinking)
            .child(
                // Its own id scope: every document numbers its texts and code
                // blocks from 0, and their click and scroll state is kept by
                // id.
                div()
                    .id(("chat-reply", index))
                    .debug_selector(move || format!("chat-reply-{index}"))
                    .text_color(ui.text)
                    .child(reply),
            )
            .children(review)
            .children(answered_by)
            .children(banner)
            .into_any_element()
    }

    fn render_banner(
        &self,
        index: usize,
        banner: Banner,
        ui: &UiTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let color = if banner.error {
            ui.error_text
        } else {
            ui.muted_text
        };
        let button = banner.action.map(|action| {
            let run = cx.listener(move |this, _, window, cx| {
                kiln::with_cx(window, cx, |cx| this.run_banner_action(action, index, cx));
            });
            Button::new(("chat-banner-action", index), action.label())
                .small()
                .debug_selector(move || format!("chat-banner-action-{index}"))
                .on_click(run)
        });
        h_flex()
            .debug_selector(move || format!("chat-banner-{index}"))
            .gap_2()
            .items_center()
            .text_color(color)
            .child(div().flex_1().min_w_0().child(banner.text))
            .children(button)
            .into_any_element()
    }

    fn render_composer(&self, ui: &UiTheme, cx: &mut Context<Self>) -> AnyElement {
        let editor = self.composer.read(cx);
        let theme = editor.theme();
        let line_height = (theme.font_size * theme.line_height).round();
        let rows = editor.model().display().row_count().clamp(1, COMPOSER_ROWS);
        let height = line_height * rows as f32 + px(2.);
        let empty = editor.model().text().to_string().trim().is_empty();
        let no_key = self.provider.is_none();
        let button = if self.is_streaming() {
            let stop = cx.listener(|this, _, window, cx| {
                kiln::with_cx(window, cx, |cx| this.stop_reply(cx));
            });
            Button::new("chat-stop", "Stop")
                .small()
                .debug_selector(|| "chat-stop".into())
                .on_click(stop)
        } else {
            let send = cx.listener(|this, _, window, cx| {
                kiln::with_cx(window, cx, |cx| this.send_message(cx));
            });
            Button::new("chat-send", "Send")
                .small()
                .primary()
                .disabled(no_key || empty || self.reading())
                .debug_selector(|| "chat-send".into())
                .on_click(send)
        };
        let attach = cx.listener(|_, _, _, cx| cx.emit(ChatViewEvent::AttachFileRequested));
        let attach = Button::new("chat-attach", "@")
            .small()
            .disabled(no_key)
            .debug_selector(|| "chat-attach".into())
            .on_click(attach);
        h_flex()
            .gap_2()
            .items_end()
            .child(attach)
            .child(
                div()
                    .key_context(COMPOSER_CONTEXT)
                    .debug_selector(|| "chat-composer".into())
                    .flex_1()
                    .min_w_0()
                    .h(height)
                    .border_1()
                    .border_color(ui.border)
                    .rounded_md()
                    .overflow_hidden()
                    .when(no_key, |composer| composer.opacity(0.6))
                    .child(self.composer.clone())
                    .child(rewrap_check(&self.composer, rows, cx)),
            )
            .child(button)
            .into_any_element()
    }

    fn render_attachments(&self, ui: &UiTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chips = self.chips();
        let reading: Vec<String> = (self.attachments.iter())
            .filter_map(|slot| match slot {
                Slot::Reading { label, .. } => Some(label.clone()),
                Slot::Ready(_) => None,
            })
            .collect();
        if chips.is_empty() && reading.is_empty() {
            return None;
        }
        let reading = reading.into_iter().enumerate().map(|(index, label)| {
            let remove = cx.listener(move |this, _, _, cx| this.remove_reading(index, cx));
            h_flex()
                .debug_selector(move || format!("chat-chip-reading-{index}"))
                .gap_1()
                .items_center()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(ui.border)
                .text_color(ui.muted_text)
                .child(format!("{label} · reading…"))
                .child(
                    div()
                        .id(("chat-chip-reading-remove", index))
                        .debug_selector(move || format!("chat-chip-reading-remove-{index}"))
                        .cursor_pointer()
                        .text_color(ui.muted_text)
                        .child("×")
                        .on_click(remove),
                )
                .into_any_element()
        });
        let chips = chips.iter().enumerate().map(|(index, chip)| {
            let remove = cx.listener(move |this, _, _, cx| this.remove_chip(index, cx));
            let color = match chip.fit {
                Fit::Whole => ui.text,
                Fit::Truncated { .. } | Fit::Dropped => ui.muted_text,
            };
            h_flex()
                .debug_selector(move || format!("chat-chip-{index}"))
                .gap_1()
                .items_center()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(ui.border)
                .text_color(color)
                .child(chip_text(chip))
                .child(
                    div()
                        .id(("chat-chip-remove", index))
                        .debug_selector(move || format!("chat-chip-remove-{index}"))
                        .cursor_pointer()
                        .text_color(ui.muted_text)
                        .child("×")
                        .on_click(remove),
                )
                .into_any_element()
        });
        Some(
            h_flex()
                .debug_selector(|| "chat-attachments".into())
                .flex_wrap()
                .gap_1()
                .children(chips)
                .children(reading)
                .into_any_element(),
        )
    }

    fn render_notice(&self, ui: &UiTheme, cx: &mut Context<Self>) -> AnyElement {
        let dismiss = cx.listener(|this, _, _, cx| {
            this.notice = false;
            cx.emit(ChatViewEvent::NoticeDismissed);
            cx.notify();
        });
        let mut lines = vec![
            "What you send goes to Anthropic (api.anthropic.com) under your API key: your \
             messages, what you attach to them, the chat's earlier turns and its instructions. \
             Nothing is sent until you press Send."
                .to_owned(),
        ];
        if self.options.fallbacks {
            lines.push(
                "When the model asked for is unavailable, another Claude model may answer; the \
                 reply says which."
                    .to_owned(),
            );
        }
        lines.push(
            "Chats are kept on this computer, per folder, until you delete them; in a folder \
             you haven't trusted, only until the window closes."
                .to_owned(),
        );
        v_flex()
            .debug_selector(|| "chat-notice".into())
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(ui.border)
            .text_color(ui.text)
            .children(lines.into_iter().map(|line| div().child(line)))
            .child(
                h_flex().justify_end().child(
                    Button::new("chat-notice-dismiss", "Got It")
                        .small()
                        .debug_selector(|| "chat-notice-dismiss".into())
                        .on_click(dismiss),
                ),
            )
            .into_any_element()
    }

    fn render_no_key(&self, ui: &UiTheme, cx: &mut Context<Self>) -> AnyElement {
        let set_key = cx.listener(|_, _, _, cx| cx.emit(ChatViewEvent::SetKeyRequested));
        v_flex()
            .debug_selector(|| "chat-no-key".into())
            .gap_2()
            .p_3()
            .items_center()
            .text_color(ui.text)
            .child("Add your Anthropic API key")
            .child(
                Button::new("chat-set-key", "Set API Key")
                    .small()
                    .primary()
                    .debug_selector(|| "chat-set-key".into())
                    .on_click(set_key),
            )
            .into_any_element()
    }
}

impl Render for ChatView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        self.follow_tail();
        let mut turns: Vec<AnyElement> = Vec::new();
        for index in 0..self.conversation.turns.len() {
            turns.push(self.render_turn(index, &ui, cx));
        }
        let transcript = ScrollArea::new("chat-transcript")
            .track_scroll(&self.scroll)
            .debug_selector(|| "chat-transcript".into())
            .flex_1()
            .min_h_0()
            .child(v_flex().p_3().gap_4().children(turns));
        let meter = self.meter().map(|meter| {
            div()
                .debug_selector(|| "chat-meter".into())
                .text_color(ui.muted_text)
                .child(SharedString::from(meter))
        });
        let notice = self.notice.then(|| self.render_notice(&ui, cx));
        let no_key = self.provider.is_none().then(|| self.render_no_key(&ui, cx));
        let attachments = self.render_attachments(&ui, cx);
        let composer = self.render_composer(&ui, cx);
        let root = v_flex()
            .size_full()
            .bg(ui.panel_background)
            .text_color(ui.text)
            .children(notice)
            .children(no_key)
            .child(transcript)
            .child(
                v_flex()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(ui.border)
                    .children(attachments)
                    .child(composer)
                    .children(meter),
            );
        <Self as ViewCommands>::attach(root, cx)
    }
}

impl Focusable for ChatView {
    /// The composer's: focusing the view puts the caret in it.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.composer.focus_handle(cx)
    }
}

/// An empty element after the composer's editor that asks for another
/// frame when the editor, laid out at this frame's width, wraps to a row
/// count other than the `rows` its height was made for: a narrower or
/// wider panel rewraps it in prepaint, after this view rendered.
fn rewrap_check(
    composer: &Entity<EditorView>,
    rows: u32,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let (composer, chat) = (composer.downgrade(), cx.weak_entity());
    canvas(
        move |_, window, cx| {
            let now = (composer.upgrade())
                .map(|composer| composer.read(cx).model().display().row_count())
                .map(|now| now.clamp(1, COMPOSER_ROWS));
            if now.is_some_and(|now| now != rows) {
                let chat = chat.clone();
                window.defer(cx, move |_, cx| {
                    chat.update(cx, |_, cx| cx.notify()).ok();
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// Whether the transcript follows the reply's tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tail {
    Off,
    /// A turn began: to the bottom, wherever the user had scrolled.
    Jump,
    /// At the bottom while the reply grows, until the user scrolls up.
    Follow,
}

/// The model serving `turn`'s reply: the last `fallback` block's, after a
/// fallback mid-output, else the one that started it.
fn serving_model(turn: &Turn) -> Option<&str> {
    let fallback = turn.reply.iter().rev().find_map(|block| match &block.kind {
        BlockKind::Fallback { to, .. } if !to.is_empty() => Some(to.as_str()),
        _ => None,
    });
    fallback.or(turn.model.as_deref())
}

/// Whether a turn may be sent again.
fn retryable(turn: &Turn) -> bool {
    matches!(turn.state, TurnState::Failed(_) | TurnState::Refused)
}

/// What the user asked: the last text block of what was sent, after the
/// attached context.
fn question_of(turn: &Turn) -> String {
    (turn.user_content.last())
        .and_then(|block| block["text"].as_str())
        .unwrap_or_default()
        .to_owned()
}

/// The context items `turn` was sent with, read back from its user
/// content: [`render_user_content`] makes one tagged block per attachment,
/// in order, then the question, and the items read back render to the
/// same blocks. Empty when the content isn't laid out so.
fn items_of(turn: &Turn) -> Vec<ContextItem> {
    if turn.user_content.len() != turn.attachments.len() + 1 {
        return Vec::new();
    }
    let items: Option<Vec<ContextItem>> = (turn.attachments.iter())
        .zip(&turn.user_content)
        .map(|(attachment, block)| item_of(attachment, block["text"].as_str()?))
        .collect();
    items.unwrap_or_default()
}

fn item_of(attachment: &Attachment, tagged: &str) -> Option<ContextItem> {
    let path = || attachment.path.clone();
    let kind = match attachment.kind {
        AttachmentKind::File => ContextKind::File {
            path: path()?,
            rows: attachment.rows,
        },
        AttachmentKind::Selection => ContextKind::Selection {
            path: path()?,
            rows: attachment.rows?,
        },
        AttachmentKind::Diagnostics => ContextKind::Diagnostics,
        AttachmentKind::OpenEditors => ContextKind::OpenEditors,
    };
    let mut item = ContextItem {
        kind,
        label: attachment.label.clone(),
        text: String::new(),
    };
    // The item with no text renders as its opening tag, a newline and its
    // closing tag: what's between them in `tagged` is its text.
    let empty = render_user_content("", std::slice::from_ref(&item));
    let empty = empty.first()?["text"].as_str()?;
    let close = &empty[empty.rfind("</")?..];
    let open = empty.strip_suffix(close)?;
    item.text = tagged.strip_prefix(open)?.strip_suffix(close)?.to_owned();
    Some(item)
}

/// An attachment's metadata, for the transcript's chips.
fn attachment_of(item: &ContextItem) -> Attachment {
    let (kind, path, rows) = match &item.kind {
        ContextKind::File { path, rows } | ContextKind::ActiveFile { path, rows } => {
            (AttachmentKind::File, Some(path.clone()), *rows)
        }
        ContextKind::Selection { path, rows } => {
            (AttachmentKind::Selection, Some(path.clone()), Some(*rows))
        }
        ContextKind::Diagnostics => (AttachmentKind::Diagnostics, None, None),
        ContextKind::OpenEditors => (AttachmentKind::OpenEditors, None, None),
    };
    Attachment {
        kind,
        label: item.label.clone(),
        path,
        rows,
        estimate: item.estimate(),
    }
}

/// Whether `at` in `text` starts a word: at the start, or after
/// whitespace.
fn word_start(text: &fe_buffer::Text, at: usize) -> bool {
    let point = text.offset_to_point(at);
    let row = text.row_to_string(point.row);
    (row.get(..point.column as usize))
        .and_then(|before| before.chars().next_back())
        .is_none_or(char::is_whitespace)
}

/// Whether `mine`, an item the user attached, stands for the host's
/// `host`: the same item, the active file's file, the same rows selected,
/// or another copy of the diagnostics or the open editors, which a turn's
/// Edit brings back. (A turn's items come back with a newline their tags
/// added, so a selection is known by its path and rows.)
fn covers(mine: &ContextItem, host: &ContextItem) -> bool {
    match (&mine.kind, &host.kind) {
        (ContextKind::File { path, .. }, ContextKind::ActiveFile { path: active, .. }) => {
            path == active
        }
        (ContextKind::Selection { .. }, ContextKind::Selection { .. }) => mine.kind == host.kind,
        (ContextKind::Diagnostics, ContextKind::Diagnostics)
        | (ContextKind::OpenEditors, ContextKind::OpenEditors) => true,
        _ => mine == host,
    }
}

/// What a chip says: its label, the whole item's estimate, and whether
/// the budget cut it or left it out.
fn chip_text(chip: &Budgeted) -> String {
    let mut text = format!("{} · {}", chip.item.label, tokens(chip.estimate));
    match chip.fit {
        Fit::Whole => {}
        Fit::Truncated { .. } => text.push_str(" · truncated"),
        Fit::Dropped => text.push_str(" · won't fit"),
    }
    text
}

/// An estimate as a chip shows it: `~850`, `~1.2k`, `~32k`.
fn tokens(estimate: u32) -> String {
    if estimate < 1000 {
        return format!("~{estimate}");
    }
    let tenths = (estimate + 50) / 100;
    match tenths {
        ..100 if !tenths.is_multiple_of(10) => format!("~{}.{}k", tenths / 10, tenths % 10),
        ..100 => format!("~{}k", tenths / 10),
        _ => format!("~{}k", (estimate + 500) / 1000),
    }
}

/// A sent attachment's label, in the user's message.
fn chip(label: String, ui: &UiTheme) -> impl IntoElement {
    div()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(ui.border)
        .text_color(ui.muted_text)
        .child(label)
}
