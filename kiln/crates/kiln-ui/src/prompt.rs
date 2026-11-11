// SPDX-License-Identifier: Apache-2.0

use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, PromptButton, PromptHandle, PromptLevel,
    PromptResponse, Render, RenderablePromptHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use kiln_l10n::tr;

use crate::ActiveTheme as _;

/// The prompt box's width, or the window's less a margin if that's
/// narrower. The message and detail wrap to it.
pub const PROMPT_WIDTH: gpui::Pixels = px(420.);

/// Renders `window.prompt` in the window, in the active theme's colours
/// (`cx.theme()`), where the platform has no native alert (Linux, Windows):
/// a host passes it to `App::set_prompt_builder`. Its keys follow macOS's
/// alerts as GPUI draws them: Enter answers with the default button, the
/// first that isn't a cancel button; Escape with the cancel button, or
/// without one a button titled "Cancel", as AppKit gives that title Escape,
/// or titled `kiln-ui-cancel` in the locale active when it opens. So
/// `[ok(tr!("kiln-ui-cancel")), new("Discard")]` makes Cancel the default
/// for both, in any locale. Tab and Shift-Tab move focus between the
/// buttons, from the default one, and Enter or Space then presses the
/// focused one, which draws a focus ring.
pub fn prompt_renderer(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let prompt = cx.new(|cx| Prompt::new(level, message, detail, buttons, cx));
    handle.with_view(prompt, window, cx)
}

/// A question over the window, with a button per answer.
pub struct Prompt {
    level: PromptLevel,
    message: SharedString,
    detail: Option<SharedString>,
    buttons: Vec<PromptButton>,
    /// The button Escape presses, resolved when the prompt opens: a locale
    /// change while it's open leaves the labels it was given, so it must
    /// not change which of them is Cancel.
    cancel: Option<usize>,
    /// The button Enter presses: the default until Tab moves it.
    focused: Option<usize>,
    /// Whether Tab or Shift-Tab has moved focus, so the focused button
    /// draws a ring.
    moved: bool,
    focus: FocusHandle,
}

impl Prompt {
    fn new(
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        buttons: &[PromptButton],
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            level,
            message: message.to_owned().into(),
            detail: detail.map(|detail| detail.to_owned().into()),
            cancel: Self::cancel_button(buttons),
            focused: Self::default_button(buttons).or_else(|| (!buttons.is_empty()).then_some(0)),
            moved: false,
            buttons: buttons.to_vec(),
            focus: cx.focus_handle(),
        }
    }

    /// The button Enter and Space press now: the default, or the one Tab
    /// or Shift-Tab moved to.
    pub fn focused_button(&self) -> Option<usize> {
        self.focused
    }

    /// Moves focus to the next button, or with `back` the previous one,
    /// wrapping at the ends.
    fn move_focus(&mut self, back: bool) {
        let count = self.buttons.len();
        if count == 0 {
            return;
        }
        let at = self.focused.unwrap_or(0);
        self.focused = Some(if back {
            (at + count - 1) % count
        } else {
            (at + 1) % count
        });
        self.moved = true;
    }

    /// The button Enter presses: the first that isn't a cancel button.
    pub fn default_button(buttons: &[PromptButton]) -> Option<usize> {
        buttons.iter().position(|button| !button.is_cancel())
    }

    /// The button Escape presses: the cancel button, or without one a button
    /// titled "Cancel", or `kiln-ui-cancel` in the active locale.
    pub fn cancel_button(buttons: &[PromptButton]) -> Option<usize> {
        buttons
            .iter()
            .position(PromptButton::is_cancel)
            .or_else(|| {
                let cancel = tr!("kiln-ui-cancel");
                (buttons.iter()).position(|button| {
                    let label = button.label();
                    label == "Cancel" || *label == *cancel
                })
            })
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        if keystroke.key == "tab" && !(modifiers.control || modifiers.alt || modifiers.platform) {
            self.move_focus(modifiers.shift);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if modifiers.modified() {
            return;
        }
        let answer = match keystroke.key.as_str() {
            "enter" | "space" => self.focused,
            "escape" => self.cancel,
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
        let ring = self.moved.then_some(self.focused).flatten();
        let ring_color = colors.state.focus_ring;
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
                .when(Some(ix) == ring, |div| {
                    div.border_2().border_color(ring_color)
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
