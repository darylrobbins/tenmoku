// SPDX-License-Identifier: LicenseRef-Proprietary

//! A conversation view: the pane item the chat's tabs show. Its messages
//! above, a field to write the next one below. While the field holds
//! unsent text it's dirty, so a close asks first; a split shows the same
//! thread in a second view with a field of its own; and it saves in the
//! window's layout as the thread it shows.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, PromptButton, PromptLevel, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};
use kiln::{
    command::Value,
    prelude::*,
    shell::{CloseAnswer, PaneItem, StatusItem, StatusSide},
    ui::{Button, Color, TextField, TextFieldEvent, h_flex, hsla, v_flex},
};

use crate::{
    commands::SendMessage,
    setup::APP_NAME,
    threads::{Thread, Threads},
};

/// The key context a conversation sets, where `chat.send_message` applies.
pub(crate) const KEY_CONTEXT: &str = "conversation";

/// The app's table in the user's theme file (`Config::theme_extensions`).
pub(crate) const THEME_TABLE: &str = "chat";

/// The answers to the close prompt. Cancel comes first, so Return keeps
/// the message, as Escape does; only Discard closes.
const DISCARD_BUTTONS: [PromptButton; 2] = [
    PromptButton::Ok(SharedString::new_static("Cancel")),
    PromptButton::Other(SharedString::new_static("Discard")),
];
const DISCARD: usize = 1;

pub(crate) struct Conversation {
    thread: Entity<Thread>,
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

    pub(crate) fn thread(&self) -> &Entity<Thread> {
        &self.thread
    }

    /// The unsent text.
    pub(crate) fn draft<'a>(&self, cx: &'a App) -> &'a str {
        self.input.read(cx).value()
    }

    #[allow(dead_code, reason = "for tests/host.rs")]
    pub(crate) fn input(&self) -> &Entity<TextField> {
        &self.input
    }

    /// Sends the field's text, if there's any, and clears the field.
    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.draft(cx).trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.thread.update(cx, |thread, cx| thread.send(text, cx));
        self.input.update(cx, |input, cx| input.set_value("", cx));
        cx.notify();
    }

    fn send_message(&mut self, _: &SendMessage, _: &mut Window, cx: &mut Context<Self>) {
        self.send(cx);
    }
}

/// The fill behind a message: the theme file's `[chat] bubble`, or the
/// accent's tint in kiln's own themes, which have no `chat` table.
pub(crate) fn bubble_color(cx: &App) -> Hsla {
    let theme = cx.theme();
    let table = theme.data().extension(THEME_TABLE);
    let written = table.and_then(|table| table.get("bubble")?.as_str()?.parse::<Color>().ok());
    written.map_or(theme.colors.accent.subtle, hsla)
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
        let thread = self.thread.read(cx);
        let messages = thread.messages.iter().map(|text| {
            div()
                .max_w(px(520.))
                .px_3()
                .py_1()
                .rounded_md()
                .bg(bubble)
                .child(text.clone())
        });
        let empty = thread.messages.is_empty().then(|| {
            div()
                .text_color(c.text.muted)
                .child("No messages yet. Say hello.")
        });
        v_flex()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::send_message))
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
                    .children(empty)
                    .children(messages),
            )
            .child(
                h_flex()
                    .flex_none()
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

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — {APP_NAME}", self.tab_title(cx)).into()
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
        let detail = "It hasn't been sent.";
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(detail),
            &DISCARD_BUTTONS,
            cx,
        );
        CloseAnswer::Ask(cx.spawn(async move |_, _| answer.await == Ok(DISCARD)))
    }

    /// Changes with every edit, so a close the user agreed to asks again
    /// if the message changed before it went ahead.
    fn close_revision(&self, cx: &App) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.draft(cx).hash(&mut hasher);
        hasher.finish()
    }

    /// The message count, on the right while this is the active item.
    fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        let count = self.thread.read(cx).messages.len();
        let text = match count {
            1 => "1 message".to_owned(),
            count => format!("{count} messages"),
        };
        vec![StatusItem::new("chat.messages", StatusSide::Right, 0, text)]
    }

    /// The same thread, with a field of its own.
    fn split(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let thread = self.thread.clone();
        Some(cx.new(|cx| Self::new(thread, window, cx)))
    }

    fn serialize(&self, cx: &App) -> Option<Value> {
        Some(Value::from_iter([
            ("thread", Value::from(self.thread.read(cx).id)),
            ("draft", Value::from(self.draft(cx))),
        ]))
    }

    /// The saved thread, if it's still there, and its unsent text.
    fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let thread = state["thread"]
            .as_u64()
            .and_then(|id| Threads::find(id, cx));
        Task::ready(thread.map(|thread| {
            cx.new(|cx| {
                let mut conversation = Self::new(thread, window, cx);
                conversation.apply_state(&state, window, cx);
                conversation
            })
        }))
    }

    fn apply_state(&mut self, state: &Value, _: &mut Window, cx: &mut Context<Self>) {
        let draft = state["draft"].as_str().unwrap_or_default();
        self.input
            .update(cx, |input, cx| input.set_value(draft, cx));
    }
}
