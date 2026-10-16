// SPDX-License-Identifier: LicenseRef-Proprietary

//! A conversation view: the pane item the chat's tabs show. Its messages
//! above, a field to write the next one below. While the field holds
//! unsent text it's dirty, so a close asks first; a split shows the same
//! thread in a second view with a field of its own; and it saves in the
//! window's layout as the thread it shows and its unsent text.

use std::hash::{BuildHasher as _, BuildHasherDefault, DefaultHasher};

use kiln::gpui::{prelude::*, *};
use kiln::{Cx, Item, View, prelude::*, serde::*, shell::*, ui::*};

use crate::threads::{Threads, plural};

/// The app's table in the user's theme file (`Config::theme_extensions`).
pub(crate) const THEME_TABLE: &str = "chat";

/// A conversation in a saved layout: the thread it shows, by its ID, and
/// its unsent text. Two are alike when they show one thread.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
pub(crate) struct ConversationState {
    pub(crate) thread: usize,
    pub(crate) draft: String,
}

impl ConversationState {
    pub(crate) fn new(thread: usize) -> Self {
        Self {
            thread,
            ..Default::default()
        }
    }
}

impl PartialEq for ConversationState {
    fn eq(&self, other: &Self) -> bool {
        self.thread == other.thread
    }
}

#[kiln::item(kind = "chat.conversation")]
pub(crate) struct Conversation {
    pub(crate) state: ConversationState,
    input: Entity<TextField>,
    typed: Option<Subscription>,
    scrolled: (ScrollHandle, usize),
}

#[kiln::command(context = "conversation")]
impl Conversation {
    /// Send Message
    ///
    /// Sends the field's text, if there's any, and clears the field.
    #[command(id = "chat.send_message")]
    fn send_message(&mut self, cx: &mut Cx<Self>) {
        let (thread, text) = (self.state.thread, std::mem::take(&mut self.state.draft));
        if !text.trim().is_empty() {
            cx.update_model(|all: &mut Threads, _| all.0[thread].messages.push(text));
        }
        let input = &self.input;
        input.update(cx.cx, |input, cx| input.set_value("", cx));
    }
}

impl Item for Conversation {
    type State = ConversationState;

    fn new(state: ConversationState, window: &mut Window, cx: &mut App) -> Option<Self> {
        Threads::get(state.thread, cx)?;
        let input = cx.new(|cx| {
            TextField::new(window, cx)
                .placeholder("Write a message…")
                .text(&state.draft)
        });
        Some(Self {
            state,
            input,
            typed: None,
            scrolled: Default::default(),
        })
    }

    fn state(&self) -> ConversationState {
        self.state.clone()
    }

    fn title(&self, cx: &App) -> SharedString {
        Threads::get(self.state.thread, cx).unwrap().title.clone()
    }

    fn focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    /// Unsent text would be lost.
    fn is_dirty(&self, _: &App) -> bool {
        !self.state.draft.trim().is_empty()
    }

    /// Unsent text asks whether to discard it; Cancel keeps it.
    fn may_close(&mut self, window: &mut Window, cx: &mut App) -> CloseAnswer {
        if self.state.draft.trim().is_empty() {
            return CloseAnswer::Close;
        }
        let question = format!("Discard your message to {}?", self.title(cx));
        // Cancel comes first, so Return keeps the message, as Escape does.
        let buttons = [PromptButton::ok("Cancel"), PromptButton::new("Discard")];
        let detail = Some("It hasn't been sent.");
        let answer = window.prompt(PromptLevel::Warning, &question, detail, &buttons, cx);
        CloseAnswer::Ask(cx.spawn(async move |_| answer.await == Ok(1)))
    }

    /// Changes with every edit, so a close the user agreed to asks again
    /// if the message changed before it went ahead.
    fn close_revision(&self, _: &App) -> u64 {
        BuildHasherDefault::<DefaultHasher>::default().hash_one(&self.state.draft)
    }

    /// The message count, on the right while this is the active item.
    fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        let count = Threads::get(self.state.thread, cx).unwrap().messages.len();
        vec![StatusItem::new(
            "chat.messages",
            StatusSide::Right,
            0,
            plural(count, "message"),
        )]
    }

    /// The same thread, with a field of its own.
    fn split(&mut self, window: &mut Window, cx: &mut App) -> Option<Self> {
        Self::new(ConversationState::new(self.state.thread), window, cx)
    }
}

/// The fill behind a message: the theme file's `[chat] bubble`, or the
/// accent's tint in kiln's own themes, which have no `chat` table.
pub(crate) fn bubble_color(cx: &App) -> Hsla {
    let table = cx.theme().data().extension(THEME_TABLE);
    let written = table.and_then(|table| table.get("bubble")?.as_str()?.parse::<Color>().ok());
    written.map_or(cx.theme().colors.accent.subtle, kiln::ui::hsla)
}

impl View for Conversation {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
        // Every edit can change whether it's dirty, which the tab shows;
        // Enter in the field sends.
        let input = self.input.clone();
        self.typed.get_or_insert_with(|| {
            cx.cx.subscribe_in(
                &input,
                cx.window,
                |this, input, event: &TextFieldEvent, window, cx| {
                    this.state.draft = input.read(cx).value().to_owned();
                    cx.notify();
                    if let TextFieldEvent::Submitted = event {
                        kiln::with_cx(window, cx, |cx| this.send_message(cx));
                    }
                },
            )
        });
        let (c, bubble) = (cx.theme().colors, bubble_color(cx));
        let cell = || div().px_3().py_1().rounded_md().bg(bubble);
        let messages = &cx.watch::<Threads>().0[self.state.thread].messages;
        let seen = std::mem::replace(&mut self.scrolled.1, messages.len());
        if seen != messages.len() {
            self.scrolled.0.scroll_to_bottom();
        }
        let rows = messages.iter().map(|text| cell().child(text.clone()));
        let log = v_flex().id("messages").debug_selector(|| "messages".into());
        let log = log.flex_1().min_h_0().overflow_y_scroll();
        let log = log.track_scroll(&self.scrolled.0).p_4().gap_2();
        let bar = h_flex().p_2().gap_2().border_t_1();
        let bar = bar.border_color(c.border.default);
        let send = Button::new("send", "Send").small().command(SendMessage);
        let bar = bar
            .child(div().flex_1().child(self.input.clone()))
            .child(send);
        v_flex().size_full().child(log.children(rows)).child(bar)
    }
}
