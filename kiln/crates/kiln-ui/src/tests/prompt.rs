// SPDX-License-Identifier: Apache-2.0

//! The in-window prompt: Enter answers with the default button, Escape with
//! the cancel button; Tab and Shift-Tab move between the buttons, and Enter
//! or Space presses the focused one.

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

/// Three buttons: Save (the default), Don't Save, and Cancel.
fn three() -> [PromptButton; 3] {
    [
        PromptButton::ok("Save"),
        PromptButton::new("Don't Save"),
        PromptButton::cancel("Cancel"),
    ]
}

#[gpui::test]
fn tab_then_enter_presses_the_next_button(cx: &mut TestAppContext) {
    assert_eq!(answer(cx, &three(), "tab enter"), Some(1));
}

#[gpui::test]
fn tab_wraps_past_the_last_button(cx: &mut TestAppContext) {
    assert_eq!(answer(cx, &three(), "tab tab tab enter"), Some(0));
}

#[gpui::test]
fn shift_tab_moves_back_and_wraps(cx: &mut TestAppContext) {
    assert_eq!(answer(cx, &three(), "shift-tab enter"), Some(2));
    assert_eq!(answer(cx, &three(), "shift-tab shift-tab enter"), Some(1));
}

#[gpui::test]
fn space_presses_the_focused_button(cx: &mut TestAppContext) {
    assert_eq!(answer(cx, &three(), "tab space"), Some(1));
}

#[gpui::test]
fn escape_still_cancels_after_tab(cx: &mut TestAppContext) {
    assert_eq!(answer(cx, &three(), "tab escape"), Some(2));
}

#[gpui::test]
fn the_third_button_is_reachable_by_keys(cx: &mut TestAppContext) {
    // A prompt with no cancel button: only Tab reaches the others.
    let buttons = [
        PromptButton::ok("Keep"),
        PromptButton::new("Reload"),
        PromptButton::new("Compare"),
    ];
    assert_eq!(answer(cx, &buttons, "tab tab enter"), Some(2));
}

/// A fake locale, `qps-ploc`, with kiln-ui's Cancel, for the locale
/// switch below. The process catalogue is global; nextest runs each test
/// in a process of its own, and the test switches back.
static FAKE_LOCALE: kiln_l10n::Resources = kiln_l10n::Resources {
    crate_name: "kiln-ui-prompt-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[kiln_l10n::ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/kiln-ui-prompt.ftl",
        source: "kiln-ui-cancel = Ćáñćéł\n",
    }],
    dynamic_prefixes: &[],
};

inventory::submit! { kiln_l10n::Registered(&FAKE_LOCALE) }

/// A prompt opened in a locale keeps its translated Cancel as the Escape
/// button when the locale changes before Escape (T550 F3).
#[gpui::test]
fn escape_cancels_a_translated_prompt_after_a_locale_change(cx: &mut TestAppContext) {
    let fake: Vec<kiln_l10n::LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    kiln_l10n::set_locales(&fake);
    let cancel = kiln_l10n::tr!("kiln-ui-cancel");
    assert_eq!(cancel, "Ćáñćéł");
    let buttons = [PromptButton::new("Discard"), PromptButton::new(cancel)];
    cx.update(|cx| cx.set_prompt_builder(prompt_renderer));
    let (_, cx) = cx.add_window_view(|_, cx| Root(cx.focus_handle()));
    let mut answer =
        cx.update(|window, cx| window.prompt(PromptLevel::Warning, "Discard?", None, &buttons, cx));
    cx.run_until_parked();
    kiln_l10n::set_locales(&[]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(answer.try_recv().ok().flatten(), Some(1));
}
