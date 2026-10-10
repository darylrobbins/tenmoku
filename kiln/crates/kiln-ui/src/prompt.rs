// SPDX-License-Identifier: LicenseRef-Proprietary

use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, PromptButton, PromptHandle, PromptLevel,
    PromptResponse, Render, RenderablePromptHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::ActiveTheme as _;

/// The prompt box's width, or the window's less a margin if that's
/// narrower. The message and detail wrap to it.
pub const PROMPT_WIDTH: gpui::Pixels = px(420.);

/// Renders `window.prompt` in the window, in the active theme's colours
/// (`cx.theme()`), where the platform has no native alert (Linux, Windows):
/// a host passes it to `App::set_prompt_builder`. Enter answers with the
/// default button, the first that isn't a cancel button; Escape with the
/// cancel button, if there is one.
pub fn prompt_renderer(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let prompt = cx.new(|cx| Prompt {
        level,
        message: message.to_owned().into(),
        detail: detail.map(|detail| detail.to_owned().into()),
        buttons: buttons.to_vec(),
        focus: cx.focus_handle(),
    });
    handle.with_view(prompt, window, cx)
}

/// A question over the window, with a button per answer.
pub struct Prompt {
    level: PromptLevel,
    message: SharedString,
    detail: Option<SharedString>,
    buttons: Vec<PromptButton>,
    focus: FocusHandle,
}

impl Prompt {
    /// The button Enter presses: the first that isn't a cancel button.
    pub fn default_button(buttons: &[PromptButton]) -> Option<usize> {
        buttons.iter().position(|button| !button.is_cancel())
    }

    /// The button Escape presses.
    pub fn cancel_button(buttons: &[PromptButton]) -> Option<usize> {
        buttons.iter().position(PromptButton::is_cancel)
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let answer = match keystroke.key.as_str() {
            "enter" => Self::default_button(&self.buttons),
            "escape" => Self::cancel_button(&self.buttons),
            _ => None,
        };
        if let Some(ix) = answer {
            cx.emit(PromptResponse(ix));
            cx.stop_propagation();
        }
    }
}

impl EventEmitter<PromptResponse> for Prompt {}

impl Focusable for Prompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Prompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let default = Self::default_button(&self.buttons);
        let accent = match self.level {
            PromptLevel::Critical => colors.status.error,
            PromptLevel::Info | PromptLevel::Warning => colors.accent.base,
        };
        let hover = colors.list.row_hover;
        let buttons = self.buttons.iter().enumerate().map(|(ix, button)| {
            let is_default = Some(ix) == default;
            div()
                .id(ix)
                .flex()
                .items_center()
                .justify_center()
                .h(px(26.))
                .px_3()
                .rounded_md()
                .border_1()
                .cursor_pointer()
                .map(|div| {
                    if is_default {
                        div.bg(accent)
                            .border_color(accent)
                            .text_color(colors.tab.active_text)
                    } else {
                        div.border_color(colors.border.default)
                            .text_color(colors.text.primary)
                            .hover(move |style| style.bg(hover))
                    }
                })
                .child(button.label().clone())
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PromptResponse(ix));
                    cx.stop_propagation();
                }))
        });
        let prompt = div()
            .occlude()
            .w(PROMPT_WIDTH)
            .max_w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(colors.border.default)
            .rounded_md()
            .shadow_lg()
            .text_color(colors.text.primary)
            .text_size(px(13.))
            .child(
                div()
                    .w_full()
                    .text_size(px(14.))
                    .child(self.message.clone()),
            )
            .when_some(self.detail.clone(), |prompt, detail| {
                prompt.child(div().w_full().text_color(colors.text.muted).child(detail))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .pt_2()
                    .children(buttons),
            );
        div()
            .id("prompt")
            .key_context("prompt")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .bg(colors.surface.overlay)
            .child(prompt)
    }
}
