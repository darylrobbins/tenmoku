// SPDX-License-Identifier: LicenseRef-Proprietary

//! The in-window prompt: Enter answers with the default button, Escape with
//! the cancel button.

use gpui::{
    Context, FocusHandle, Focusable, IntoElement, PromptButton, PromptLevel, Render,
    TestAppContext, Window, div,
};

use crate::{Prompt, prompt_renderer};

struct Root(FocusHandle);

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.0.clone()
    }
}

/// The button pressing `keys` on a prompt with `buttons` answers with.
fn answer(cx: &mut TestAppContext, buttons: &[PromptButton], keys: &str) -> Option<usize> {
    cx.update(|cx| cx.set_prompt_builder(prompt_renderer));
    let (_, cx) = cx.add_window_view(|_, cx| Root(cx.focus_handle()));
    let mut answer = cx.update(|window, cx| {
        window.prompt(PromptLevel::Warning, "Trust?", Some("detail"), buttons, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    answer.try_recv().ok().flatten()
}

#[test]
fn default_and_cancel_buttons() {
    let ok_cancel = [PromptButton::ok("Trust"), PromptButton::cancel("Don't")];
    assert_eq!(Prompt::default_button(&ok_cancel), Some(0));
    assert_eq!(Prompt::cancel_button(&ok_cancel), Some(1));
    let cancel_first = [PromptButton::cancel("Cancel"), PromptButton::new("Delete")];
    assert_eq!(Prompt::default_button(&cancel_first), Some(1));
    assert_eq!(Prompt::cancel_button(&cancel_first), Some(0));
    assert_eq!(Prompt::cancel_button(&[PromptButton::ok("OK")]), None);
    // Cancel as the default: Enter and Escape both answer with it.
    let cancel_default = [PromptButton::ok("Cancel"), PromptButton::new("Discard")];
    assert_eq!(Prompt::default_button(&cancel_default), Some(0));
    assert_eq!(Prompt::cancel_button(&cancel_default), Some(0));
}

#[gpui::test]
fn enter_answers_with_the_default_button(cx: &mut TestAppContext) {
    let buttons = [PromptButton::ok("Trust"), PromptButton::cancel("Don't")];
    assert_eq!(answer(cx, &buttons, "enter"), Some(0));
}

#[gpui::test]
fn escape_answers_with_the_cancel_button(cx: &mut TestAppContext) {
    let buttons = [PromptButton::ok("Trust"), PromptButton::cancel("Don't")];
    assert_eq!(answer(cx, &buttons, "escape"), Some(1));
}

#[gpui::test]
fn escape_without_a_cancel_button_answers_nothing(cx: &mut TestAppContext) {
    assert_eq!(answer(cx, &[PromptButton::ok("OK")], "escape"), None);
}
