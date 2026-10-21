//! [`ChatView`]: the transcript above, the attachments and the composer
//! below, and the stream that fills the last turn.

use std::collections::HashSet;
use std::sync::Arc;

use fe_buffer::{Buffer, Origin};
use fe_chat::{
    Attachment, AttachmentKind, BlockKind, ChatEvent, ChatProvider, ChatSettings, ContextItem,
    ContextKind, Conversation, ErrorKind, ProviderError, Turn, TurnState, build_request,
    render_user_content,
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

use crate::COMPOSER_CONTEXT;

/// The context window the meter measures against.
const CONTEXT_WINDOW: u64 = 1_000_000;

/// How many rows the composer grows to before it scrolls.
const COMPOSER_ROWS: u32 = 10;

/// How near the bottom, in pixels, the transcript counts as at the tail.
const TAIL_SLACK: f32 = 4.;

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
    attachments: Vec<ContextItem>,
    notice: bool,
    /// The reply streaming in: dropping it drops the stream, which cancels
    /// the request.
    stream: Option<Task<()>>,
    scroll: ScrollHandle,
    tail: Tail,
    /// The turns whose thinking is open.
    thinking_open: HashSet<usize>,
    _composer_changed: Subscription,
}

impl EventEmitter<ChatViewEvent> for ChatView {}

#[kiln::command(context = "chat")]
impl ChatView {
    /// Send Message
    ///
    /// Sends the composer's text with the attachments, and empties both.
    /// Does nothing with no text, no provider, or while a reply streams.
    #[command(id = "chat.send")]
    pub fn send_message(&mut self, cx: &mut Cx<Self>) {
        let text = self.composer_text(cx);
        if text.trim().is_empty() || self.is_streaming() || self.provider.is_none() {
            return;
        }
        let items = std::mem::take(&mut self.attachments);
        let user_content = render_user_content(&text, &items);
        let attachments = items.iter().map(attachment_of).collect();
        self.set_composer_text("", cx);
        self.start_turn(user_content, attachments, cx);
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
        let (user_content, attachments) = (turn.user_content.clone(), turn.attachments.clone());
        self.start_turn(user_content, attachments, cx);
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
        // It grows as it's typed into.
        let composer_changed = cx.observe(&composer, |_, _, cx| cx.notify());
        ChatView {
            provider: Some(provider),
            conversation,
            options,
            composer,
            attachments: Vec::new(),
            notice: false,
            stream: None,
            scroll: ScrollHandle::new(),
            tail: Tail::Off,
            thinking_open: HashSet::new(),
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

    /// Adds `item` to what the next message sends, as a chip.
    pub fn attach(&mut self, item: ContextItem, cx: &mut Context<Self>) {
        self.attachments.push(item);
        cx.notify();
    }

    /// What the next message sends with it.
    pub fn attachments(&self) -> &[ContextItem] {
        &self.attachments
    }

    /// Takes the attachment at `index` off, as its chip's remove button
    /// does.
    pub fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
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
    }

    /// Whether a reply is streaming in.
    pub fn is_streaming(&self) -> bool {
        self.stream.is_some()
    }

    /// Whether turn `index`'s thinking is open.
    pub fn thinking_open(&self, index: usize) -> bool {
        self.thinking_open.contains(&index)
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

    /// Begins a turn of `user_content` and streams its reply.
    fn start_turn(
        &mut self,
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
        self.conversation.begin_turn(user_content, attachments);
        let mut stream = provider.stream(request);
        self.tail = Tail::Jump;
        self.stream = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = stream.next().await {
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
            drop(stream);
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
                self.set_composer_text(&question, cx.cx);
                for item in items {
                    if !self.attachments.contains(&item) {
                        self.attachments.push(item);
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

    fn render_turn(&self, index: usize, ui: &UiTheme, cx: &mut Context<Self>) -> AnyElement {
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
        let thinking = shown.thinking.clone().map(|text| {
            let open = self.thinking_open(index);
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
                .when(open, |element| {
                    element.child(
                        div()
                            .debug_selector(move || format!("chat-thinking-text-{index}"))
                            .pl_3()
                            .border_l_1()
                            .border_color(ui.border)
                            .text_color(ui.muted_text)
                            .child(text),
                    )
                })
        });
        let reply = if shown.reply.is_empty() && streaming {
            "…".to_owned()
        } else {
            shown.reply.clone()
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
        v_flex()
            .debug_selector(move || format!("chat-turn-{index}"))
            .gap_2()
            .child(user)
            .children(thinking)
            .child(
                div()
                    .debug_selector(move || format!("chat-reply-{index}"))
                    .text_color(ui.text)
                    .child(reply),
            )
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
                .disabled(no_key || empty)
                .debug_selector(|| "chat-send".into())
                .on_click(send)
        };
        h_flex()
            .gap_2()
            .items_end()
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
        if self.attachments.is_empty() {
            return None;
        }
        let chips = self.attachments.iter().enumerate().map(|(index, item)| {
            let estimate = estimate(&item.text);
            let remove = cx.listener(move |this, _, _, cx| this.remove_attachment(index, cx));
            h_flex()
                .debug_selector(move || format!("chat-chip-{index}"))
                .gap_1()
                .items_center()
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(ui.border)
                .text_color(ui.text)
                .child(format!("{} · {}", item.label, tokens(estimate)))
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
        let turns: Vec<AnyElement> = (0..self.conversation.turns.len())
            .map(|index| self.render_turn(index, &ui, cx))
            .collect();
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
        ContextKind::File { path, rows } => (AttachmentKind::File, Some(path.clone()), *rows),
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
        estimate: estimate(&item.text),
    }
}

/// The record's § 13 estimate: a token for every 3 bytes, rounded up.
fn estimate(text: &str) -> u32 {
    u32::try_from(text.len().div_ceil(3)).unwrap_or(u32::MAX)
}

fn tokens(estimate: u32) -> String {
    if estimate < 1000 {
        format!("{estimate} tokens")
    } else {
        format!("{}k tokens", estimate.div_ceil(1000))
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
