// SPDX-License-Identifier: LicenseRef-Proprietary

//! TextField (caret, selection, IME, undo) and the older TextInput, which
//! appends only and leaves its editing keys to the app: the gallery binds
//! `backspace` in the `text_input` context to its own commands, handled
//! here.

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, Styled as _, Subscription, Window, div, px,
};
use kiln::{
    prelude::*,
    ui::{TextField, TextFieldEvent, TextInput, TextInputEvent, ui_theme, v_flex},
};

use super::{labelled, page_frame, section};
use crate::commands::{InputBackspace, InputDeleteWord};

pub(crate) struct TextPage {
    field: Entity<TextField>,
    input: Entity<TextInput>,
    /// The field's text when Enter was last pressed in it.
    submitted: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl TextPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            TextField::new(window, cx).placeholder("Type, select with shift-arrows, undo…")
        });
        // TextInput copies the theme when it's made, so it doesn't follow a
        // theme switch.
        let input = cx.new(|cx| TextInput::new("Type; backspace deletes", &ui_theme(cx), cx));
        let field_events = cx.subscribe_in(&field, window, |page, field, event, _, cx| {
            if *event == TextFieldEvent::Submitted {
                page.submitted = Some(field.read(cx).value().to_owned());
            }
            cx.notify();
        });
        let input_events = cx.subscribe(&input, |_, _, _: &TextInputEvent, cx| cx.notify());
        Self {
            field,
            input,
            submitted: None,
            _subscriptions: [field_events, input_events],
        }
    }
}

impl Render for TextPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let value = self.field.read(cx).value().to_owned();
        let selected = self.field.read(cx).selected_range();
        let input_text = self.input.read(cx).text().to_owned();
        let focus = self.input.focus_handle(cx);
        let field = v_flex()
            .gap_2()
            .w(px(420.))
            .child(self.field.clone())
            .child(
                div()
                    .text_color(c.text.muted)
                    .child(format!("Value: {value:?}, selection {selected:?}")),
            )
            .child(div().text_color(c.text.muted).child(match &self.submitted {
                Some(text) => format!("Submitted: {text:?}"),
                None => "Press Enter to submit".to_owned(),
            }));
        let input = v_flex()
            .gap_2()
            .w(px(420.))
            .child(
                // TextInput has no border and doesn't take focus on a click
                // by itself.
                div()
                    .border_1()
                    .border_color(c.border.default)
                    .rounded_md()
                    .bg(c.surface.panel)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&focus, cx)
                    })
                    .on_action(cx.listener(|page, _: &InputBackspace, _, cx| {
                        page.input.update(cx, |input, cx| input.backspace(cx))
                    }))
                    .on_action(cx.listener(|page, _: &InputDeleteWord, _, cx| {
                        page.input
                            .update(cx, |input, cx| input.delete_word_left(cx))
                    }))
                    .child(self.input.clone()),
            )
            .child(
                div()
                    .text_color(c.text.muted)
                    .child(format!("Text: {input_text:?}")),
            );
        page_frame(
            "Text",
            "TextField is a full single-line editor; TextInput appends only.",
            cx,
        )
        .child(section("TEXTFIELD", labelled("Field", field, cx), cx))
        .child(section("TEXTINPUT", labelled("Input", input, cx), cx))
    }
}
