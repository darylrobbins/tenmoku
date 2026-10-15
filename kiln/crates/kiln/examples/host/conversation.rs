// SPDX-License-Identifier: LicenseRef-Proprietary

//! A conversation view: the pane item the chat's tabs show. Its messages
//! above, a field to write the next one below. While the field holds
//! unsent text it's dirty, so a close asks first; a split shows the same
//! thread in a second view with a field of its own; and it saves in the
//! window's layout as the thread it shows and its unsent text
//! ([`ConversationState`]).

use std::hash::{BuildHasher as _, BuildHasherDefault, DefaultHasher};

use gpui::{App, Entity, FocusHandle, Focusable, Hsla, PromptButton, PromptLevel, prelude::*};
use gpui::{ScrollHandle, SharedString, Subscription, Task, Window, div};
use kiln::shell::{CloseAnswer, PaneItem, StatusItem, StatusSide, item::ItemState, item::state};
use kiln::ui::{Button, Color, TextField, TextFieldEvent, h_flex, hsla, v_flex};
use kiln::{command::Value, prelude::*, serde::Deserialize, serde::Serialize};

use crate::{app::SendMessage, threads::*};

/// The key context a conversation sets, where `chat.send_message` applies.
pub(crate) const KEY_CONTEXT: &str = "conversation";

/// The app's table in the user's theme file (`Config::theme_extensions`).
pub(crate) const THEME_TABLE: &str = "chat";

/// A conversation in a saved layout: the thread it shows, by its ID, and
/// its unsent text.
#[derive(Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
pub(crate) struct ConversationState {
    thread: usize,
    draft: String,
}

/// A conversation restored from its saved state, once it is.
type Restored = Task<Option<Entity<Conversation>>>;

pub(crate) struct Conversation {
    pub(crate) thread: Entity<Thread>,
    input: Entity<TextField>,
    scroll: ScrollHandle,
    _subscriptions: [Subscription; 2],
}

impl Conversation {
    pub(crate) fn new(thread: Entity<Thread>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextField::new(window, cx).placeholder("Write a message…"));
        // A message sent here or in a split of this view shows in both.
        let sent = cx.observe(&thread, |conversation: &mut Self, _, cx| {
            conversation.scroll.scroll_to_bottom();
            cx.notify();
        });
        // Every edit can change whether it's dirty, which the tab shows.
        let typed = cx.subscribe(&input, |conversation, _, event, cx| match event {
            TextFieldEvent::Submitted => conversation.send(cx),
            TextFieldEvent::Changed => cx.notify(),
            _ => {}
        });
        Self {
            thread,
            input,
            scroll: ScrollHandle::new(),
            _subscriptions: [sent, typed],
        }
    }

    /// The unsent text.
    pub(crate) fn draft<'a>(&self, cx: &'a App) -> &'a str {
        self.input.read(cx).value()
    }

    /// Sends the field's text, if there's any, and clears the field.
    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.draft(cx).trim().to_owned();
        if !text.is_empty() {
            // Views of the thread redraw, as they observe it.
            self.thread.update(cx, |thread, cx| {
                thread.messages.push(text.into());
                cx.notify();
            });
            self.input.update(cx, |input, cx| input.set_value("", cx));
        }
    }
}

/// The fill behind a message: the theme file's `[chat] bubble`, or the
/// accent's tint in kiln's own themes, which have no `chat` table.
pub(crate) fn bubble_color(cx: &App) -> Hsla {
    let table = cx.theme().data().extension(THEME_TABLE);
    let written = table.and_then(|table| table.get("bubble")?.as_str()?.parse::<Color>().ok());
    written.map_or(cx.theme().colors.accent.subtle, hsla)
}

impl Focusable for Conversation {
    /// The field: showing the tab puts the caret there.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for Conversation {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let bubble = bubble_color(cx);
        let messages = &self.thread.read(cx).messages;
        let hint = "No messages yet. Say hello.";
        let hint = div().text_color(c.text.muted).child(hint);
        let bubbles = (messages.iter().cloned())
            .map(|text| div().px_3().py_1().rounded_md().bg(bubble).child(text));
        v_flex()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|this, _: &SendMessage, _, cx| this.send(cx)))
            .size_full()
            .bg(c.surface.background)
            .child(
                v_flex()
                    .id("messages")
                    .debug_selector(|| "messages".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_4()
                    .gap_2()
                    .items_start()
                    .children(messages.is_empty().then_some(hint))
                    .children(bubbles),
            )
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(c.border.default)
                    .child(div().flex_1().child(self.input.clone()))
                    .child(Button::new("send", "Send").small().command(SendMessage)),
            )
    }
}

impl PaneItem for Conversation {
    const KIND: &'static str = "chat.conversation";

    fn tab_title(&self, cx: &App) -> SharedString {
        self.thread.read(cx).title.clone()
    }

    /// Unsent text would be lost.
    fn is_dirty(&self, cx: &App) -> bool {
        !self.draft(cx).trim().is_empty()
    }

    /// Unsent text asks whether to discard it; Cancel keeps it.
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.is_dirty(cx) {
            return CloseAnswer::Close;
        }
        let question = format!("Discard your message to {}?", self.tab_title(cx));
        let detail = Some("It hasn't been sent.");
        // Cancel comes first, so Return keeps the message, as Escape does;
        // only Discard, the second, closes.
        let buttons = [PromptButton::ok("Cancel"), PromptButton::new("Discard")];
        let answer = window.prompt(PromptLevel::Warning, &question, detail, &buttons, cx);
        CloseAnswer::Ask(cx.spawn(async move |_, _| answer.await == Ok(1)))
    }

    /// Changes with every edit, so a close the user agreed to asks again
    /// if the message changed before it went ahead.
    fn close_revision(&self, cx: &App) -> u64 {
        BuildHasherDefault::<DefaultHasher>::default().hash_one(self.draft(cx))
    }

    /// The message count, on the right while this is the active item.
    fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        let text = plural(self.thread.read(cx).messages.len(), "message");
        vec![StatusItem::new("chat.messages", StatusSide::Right, 0, text)]
    }

    /// The same thread, with a field of its own.
    fn split(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        Some(cx.new(|cx| Self::new(self.thread.clone(), window, cx)))
    }

    fn serialize(&self, cx: &App) -> Option<Value> {
        state::serialize(self, cx)
    }

    fn restore(state: Value, window: &mut Window, cx: &mut App) -> Restored {
        state::restore::<Self>(state, window, cx)
    }

    fn apply_state(&mut self, state: &Value, window: &mut Window, cx: &mut Context<Self>) {
        state::apply(self, state, window, cx)
    }
}

impl ItemState for Conversation {
    type State = ConversationState;

    fn state(&self, cx: &App) -> Option<ConversationState> {
        let thread = Threads::all(cx).iter().position(|t| *t == self.thread)?;
        let draft = self.draft(cx).to_owned();
        Some(ConversationState { thread, draft })
    }

    /// The saved thread, if it's still there, and its unsent text.
    fn from_state(state: Self::State, window: &mut Window, cx: &mut App) -> Restored {
        let thread = Threads::all(cx).get(state.thread).cloned();
        let conversation = thread.map(|thread| cx.new(|cx| Self::new(thread, window, cx)));
        if let Some(conversation) = &conversation {
            conversation.update(cx, |this, cx| this.apply(&state, window, cx));
        }
        Task::ready(conversation)
    }

    fn apply(&mut self, state: &ConversationState, _: &mut Window, cx: &mut Context<Self>) {
        (self.input).update(cx, |input, cx| input.set_value(&state.draft, cx));
    }
}
